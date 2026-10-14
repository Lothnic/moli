use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use moli_core::page::{
    ChildFrameTreeSnapshot, Page, RendererPageDumpFormat, RendererPageDumpOptions,
    RendererPageDumpStripOptions, SubresourceResponseWaitCriteria,
};
use serde_json::{Map, Value, json};

use crate::{
    cli::StripOptions,
    network_trace::{NetworkTraceConfigSummary, NetworkTraceOptions, render_network_trace},
};

pub(super) async fn render_json(
    page: &mut Page,
    strip: StripOptions,
    with_base: bool,
    with_frames: bool,
    trace_network: bool,
    response_wait: Option<&SubresourceResponseWaitCriteria>,
    network_trace_config: Option<&NetworkTraceConfigSummary>,
    network_trace_options: NetworkTraceOptions,
) -> Result<String> {
    let html = render_html(page, strip, with_base, with_frames).await?;
    let network = if trace_network {
        let main_document_html = if html_dump_needs_dom_postprocess(strip, with_base, with_frames) {
            page.serialize_html_async().await?
        } else {
            html.clone()
        };
        Some(render_network_trace(
            page,
            &main_document_html,
            response_wait,
            network_trace_config,
            network_trace_options,
        ))
    } else {
        None
    };
    let title = page.document_title();
    render_json_payload(
        page.final_url().as_str(),
        page.status(),
        Some(&title),
        page.headers(),
        page.navigation_redirect_chain(),
        &html,
        network,
    )
}

pub(super) fn render_json_payload(
    final_url: &str,
    status: u16,
    title: Option<&str>,
    headers: &[(String, Vec<u8>)],
    redirect_chain: &[moli_core::page::NavigationRedirect],
    html: &str,
    network: Option<Value>,
) -> Result<String> {
    // Keep --dump json as a stable machine interface for scrapling-style callers.
    let mut payload = response_metadata(final_url, status, title, headers, redirect_chain);
    payload.insert("html".to_owned(), json!(html));
    if let Some(network) = network {
        payload.insert("network".to_owned(), network);
    }
    Ok(serde_json::to_string_pretty(&Value::Object(payload))?)
}

pub(super) fn render_raw_json_payload(
    final_url: &str,
    status: u16,
    headers: &[(String, Vec<u8>)],
    redirect_chain: &[moli_core::page::NavigationRedirect],
    body: &[u8],
) -> Result<String> {
    let mut payload = response_metadata(final_url, status, None, headers, redirect_chain);
    payload.insert("html".to_owned(), Value::Null);
    // Raw responses may contain arbitrary bytes. Always use one lossless
    // representation instead of changing the schema based on UTF-8 validity.
    payload.insert(
        "body_base64".to_owned(),
        Value::String(BASE64_STANDARD.encode(body)),
    );
    Ok(serde_json::to_string_pretty(&Value::Object(payload))?)
}

fn response_metadata(
    final_url: &str,
    status: u16,
    title: Option<&str>,
    headers: &[(String, Vec<u8>)],
    redirect_chain: &[moli_core::page::NavigationRedirect],
) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert("final_url".to_owned(), json!(final_url));
    payload.insert("status".to_owned(), json!(status));
    payload.insert("title".to_owned(), json!(title));
    payload.insert("headers".to_owned(), render_headers(headers));
    payload.insert(
        "redirect_chain".to_owned(),
        Value::Array(
            redirect_chain
                .iter()
                .map(render_navigation_redirect)
                .collect(),
        ),
    );
    payload
}

fn render_headers(headers: &[(String, Vec<u8>)]) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|(name, value)| json!({ "name": name, "value": moli_fetch::decode_header_value(value) }))
            .collect(),
    )
}

fn render_navigation_redirect(redirect: &moli_core::page::NavigationRedirect) -> Value {
    json!({
        "from_url": redirect.from_url.as_str(),
        "to_url": redirect.to_url.as_str(),
        "status": redirect.status,
        "headers": render_headers(&redirect.headers),
    })
}

pub(super) async fn render_html(
    page: &mut Page,
    strip: StripOptions,
    with_base: bool,
    with_frames: bool,
) -> Result<String> {
    if !html_dump_needs_dom_postprocess(strip, with_base, with_frames) {
        return page.serialize_html_async().await;
    }

    page.render_page_dump_async(RendererPageDumpOptions {
        format: RendererPageDumpFormat::Html,
        strip: renderer_strip_options(strip),
        with_base,
        with_frames,
    })
    .await
}

fn html_dump_needs_dom_postprocess(
    strip: StripOptions,
    with_base: bool,
    with_frames: bool,
) -> bool {
    strip.js || strip.ui || strip.css || with_base || with_frames
}

pub(super) async fn render_markdown(
    page: &mut Page,
    strip: StripOptions,
    with_frames: bool,
) -> Result<String> {
    // Markdown represents the rendered document, including DOM populated by
    // layout observers. Publish current geometry before taking the live DOM
    // snapshot so queued ResizeObserver work can settle first.
    page.publish_layout_async().await?;
    let mut markdown = page
        .render_page_dump_async(RendererPageDumpOptions {
            format: RendererPageDumpFormat::Markdown,
            strip: renderer_strip_options(strip),
            with_base: false,
            with_frames,
        })
        .await?;
    if with_frames {
        append_embedded_pdf_markdown(page, &mut markdown).await?;
    }
    Ok(markdown)
}

async fn append_embedded_pdf_markdown(page: &mut Page, markdown: &mut String) -> Result<()> {
    const EMBEDDED_PDF_URLS: &str = r#"JSON.stringify([...document.querySelectorAll('embed[src],object[data],iframe[src]')].map(node => { const raw = node.getAttribute(node.localName === 'object' ? 'data' : 'src'); if (!raw) return null; let url; try { url = new URL(raw, node.baseURI || document.baseURI); } catch { return null; } const declaredPdf = (node.getAttribute('type') || '').split(';', 1)[0].trim().toLowerCase() === 'application/pdf'; return declaredPdf || /\.pdf$/i.test(url.pathname) ? url.href : null; }).filter((url, index, urls) => url && urls.indexOf(url) === index))"#;
    let value = page
        .evaluate_runtime_expression_async(EMBEDDED_PDF_URLS)
        .await?;
    let Some(value) = value.get("value").and_then(Value::as_str) else {
        return Ok(());
    };
    let urls: Vec<String> = serde_json::from_str(value)?;
    let frames = page.child_frame_tree_snapshot_async().await?;
    let mut appended_urls = Vec::new();
    for url in urls {
        if appended_urls
            .iter()
            .any(|appended| same_resource_url(appended, &url))
        {
            continue;
        }
        let Some(network) = find_loaded_child_document(&frames, &url) else {
            continue;
        };
        if !(200..400).contains(&network.status) {
            continue;
        }
        let Some(body) = network.response_body.as_ref() else {
            continue;
        };
        let Ok(body) = body.try_bytes() else {
            continue;
        };
        if !body.starts_with(b"%PDF-") {
            continue;
        }
        let Ok(text) = super::render_raw_document_markdown(&body) else {
            continue;
        };
        let text = String::from_utf8(text)?;
        markdown.push_str("\n\n## Embedded document\n\n");
        markdown.push_str(text.trim());
        appended_urls.push(url);
    }
    Ok(())
}

fn find_loaded_child_document<'a>(
    frames: &'a [ChildFrameTreeSnapshot],
    url: &str,
) -> Option<&'a moli_core::page::ChildFrameDocumentNetworkSnapshot> {
    for frame in frames {
        if let Some(network) = frame.document_network.as_ref()
            && (same_resource_url(&frame.url, url)
                || same_resource_url(&network.request_url, url)
                || same_resource_url(&network.final_url, url))
        {
            return Some(network);
        }
        if let Some(network) = find_loaded_child_document(&frame.child_frames, url) {
            return Some(network);
        }
    }
    None
}

fn same_resource_url(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let (Ok(mut left), Ok(mut right)) = (url::Url::parse(left), url::Url::parse(right)) else {
        return false;
    };
    left.set_fragment(None);
    right.set_fragment(None);
    left == right
}

fn renderer_strip_options(strip: StripOptions) -> RendererPageDumpStripOptions {
    RendererPageDumpStripOptions {
        js: strip.js,
        ui: strip.ui,
        css: strip.css,
    }
}
