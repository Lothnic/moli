use anyhow::{Context, Result, bail};
use moli_core::page::{
    Page, RendererCaptureScreenshotReply, RendererCaptureScreenshotRequest,
    RendererPrintToPdfReply, RendererPrintToPdfRequest, RendererScreenshotRegion,
};

struct CapturedRaster {
    mime_type: String,
    bytes: Vec<u8>,
}

pub(super) async fn render_screenshot(
    page: &mut Page,
    region: RendererScreenshotRegion,
    dump_mode: &str,
) -> Result<Vec<u8>> {
    let mut request = RendererCaptureScreenshotRequest::viewport_png();
    request.region = region;
    let image = capture_page_raster(page, request, dump_mode).await?;
    if image.mime_type != "image/png" {
        bail!(
            "screenshot renderer returned `{}` instead of PNG",
            image.mime_type
        );
    }
    Ok(image.bytes)
}

pub(super) async fn render_pdf(page: &mut Page) -> Result<Vec<u8>> {
    let pending = page
        .start_print_to_pdf(RendererPrintToPdfRequest {
            // CLI exports preserve page artwork, including white-on-color controls.
            print_background: true,
            ..Default::default()
        })
        .context("failed to start --dump pdf capture")?;
    let completion = pending
        .wait()
        .await
        .context("failed to complete --dump pdf capture")?;
    match page.finish_print_to_pdf(completion)? {
        RendererPrintToPdfReply::Printed(bytes) => Ok(bytes),
        RendererPrintToPdfReply::Failed(error) => bail!("{error}"),
        RendererPrintToPdfReply::LayoutDisabled => {
            bail!("--dump pdf requires --layout or MOLI_LAYOUT=true")
        }
        RendererPrintToPdfReply::NoDocument => {
            bail!("--dump pdf requires a loaded HTML document")
        }
    }
}

async fn capture_page_raster(
    page: &mut Page,
    request: RendererCaptureScreenshotRequest,
    dump_mode: &str,
) -> Result<CapturedRaster> {
    let pending = page
        .start_capture_screenshot_with_request(request)
        .with_context(|| format!("failed to start --dump {dump_mode} capture"))?;
    let completion = pending
        .wait()
        .await
        .with_context(|| format!("failed to complete --dump {dump_mode} capture"))?;
    match page.finish_capture_screenshot(completion)? {
        RendererCaptureScreenshotReply::Captured(image) => Ok(CapturedRaster {
            mime_type: image.mime_type,
            bytes: image.bytes.to_vec(),
        }),
        RendererCaptureScreenshotReply::LayoutDisabled => {
            bail!("--dump {dump_mode} requires --layout or MOLI_LAYOUT=true")
        }
        RendererCaptureScreenshotReply::NoDocument => {
            bail!("--dump {dump_mode} requires a loaded HTML document")
        }
    }
}
