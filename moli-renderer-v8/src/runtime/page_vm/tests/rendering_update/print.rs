use super::*;
use crate::runtime::{
    RendererCaptureScreenshotReply, RendererCaptureScreenshotRequest, RendererPrintToPdfReply,
    RendererPrintToPdfRequest, RendererScreenshotPurpose,
};

#[tokio::test(flavor = "current_thread")]
async fn native_pdf_restores_media_on_success_and_failure_and_keeps_iframe_text() {
    run_page_vm_async_test(async move {
        let loader = crate::network::ResourceRequestClient::new(&FetchConfig::default())?;
        let mut page_vm = test_page_vm_with_loader_and_document_url(
            &loader, Vec::new(), Url::parse("https://example.com/native-print.html")?,
        );
        page_vm.vm_mut().eval(r#"
document.head.innerHTML = `<style>
.print { display:none } @media print { .screen { display:none } .print { display:block } }
</style>`;
document.body.innerHTML = '<p class=screen>Screen parent</p><p class=print>Print parent</p><iframe id=child></iframe>';
child.contentDocument.body.innerHTML = '<p>Iframe text</p>';
'installed'
"#)?;
        let extract = |reply| -> anyhow::Result<String> {
            let RendererPrintToPdfReply::Printed(bytes) = reply else { anyhow::bail!("expected PDF, got {reply:?}"); };
            let doc = lopdf::Document::load_mem(&bytes)?;
            Ok(doc.extract_text(&doc.get_pages().keys().copied().collect::<Vec<_>>())?
                .split_whitespace().collect::<Vec<_>>().join(" "))
        };
        let cold = page_vm.vm().layout_snapshot_cache_observability_for_test().2;
        assert_eq!(extract(page_vm.print_to_pdf(Default::default())?)?, "Print parent Iframe text");
        assert_eq!(page_vm.vm().layout_snapshot_cache_observability_for_test().2, cold);
        assert_eq!(page_vm.vm_mut().eval("matchMedia('print').matches")?, "false");

        page_vm.capture_screenshot(RendererCaptureScreenshotRequest::viewport_png())?;
        let warm = page_vm.vm().layout_snapshot_cache_observability_for_test().2;
        let mut invalid = RendererPrintToPdfRequest::default();
        invalid.options.width = 0.0;
        assert!(matches!(page_vm.print_to_pdf(invalid)?, RendererPrintToPdfReply::Failed(_)));
        assert_eq!(page_vm.vm().layout_snapshot_cache_observability_for_test().2, warm);
        assert_eq!(page_vm.vm_mut().eval("matchMedia('print').matches")?, "false");

        let mut media = page_vm.document_settings.emulated_media.clone();
        media.media = Some("screen".to_owned());
        page_vm.set_emulated_media(&media)?;
        assert_eq!(extract(page_vm.print_to_pdf(Default::default())?)?, "Screen parent Iframe text");
        assert_eq!(page_vm.document_settings.emulated_media, media);
        assert_eq!(page_vm.vm().layout_snapshot_cache_observability_for_test().2, warm);
        Ok::<_, anyhow::Error>(())
    }).await.expect("native printing preserves media and interactive state");
}

#[tokio::test(flavor = "current_thread")]
async fn print_capture_preserves_published_geometry_frame_viewports_and_mouse_targets() {
    run_page_vm_async_test(async move {
        let loader =
            crate::network::ResourceRequestClient::new(&FetchConfig::default()).expect("loader");
        let mut page_vm = test_page_vm_with_loader_and_document_url(
            &loader,
            Vec::new(),
            Url::parse("https://example.com/print-layout.html")?,
        );
        page_vm.set_viewport_surface(Some(crate::protocol_types::ViewportSurface {
            inner_width: 320,
            inner_height: 100,
            ..Default::default()
        }))?;
        page_vm.vm_mut().eval(
            r#"
document.head.innerHTML = `<style>
html,body { margin:0; background:white }
#target,iframe { position:absolute; left:0; top:0; width:40px; height:40px; border:0 }
#target { background:red }
iframe { top:50px }
.changed #target,.changed iframe { width:60px }
@media print {
  #target,iframe { left:160px; width:80px !important }
  #target { background:lime }
}
</style>`;
document.body.innerHTML = '<div id=target></div><iframe id=frame></iframe>';
const child = frame.contentDocument;
child.documentElement.style.cssText = 'margin:0';
child.body.style.cssText = 'margin:0;background:blue;height:40px';
globalThis.clicks = 0;
target.onclick = () => clicks++;
'installed'
"#,
        )?;
        page_vm.vm_mut().sync_live_document_style_sources();
        page_vm.capture_screenshot(RendererCaptureScreenshotRequest::viewport_png())?;
        let geometry = "JSON.stringify([target.getBoundingClientRect().width,frame.getBoundingClientRect().width,frame.contentDocument.body.getBoundingClientRect().width,frame.contentWindow.innerWidth])";
        assert_eq!(page_vm.vm_mut().eval(geometry)?, "[40,40,40,40]");
        let before = page_vm.vm().layout_pass_observability_for_test();
        let published_before = page_vm.vm().layout_snapshot_cache_observability_for_test().2;

        // Restoring the screen media after printing must not rebuild the live
        // DOM either: geometry continues to describe the earlier screenshot.
        page_vm.vm_mut().eval("document.body.classList.add('changed')")?;
        let RendererCaptureScreenshotReply::Captured(print) = page_vm.capture_screenshot(
            RendererCaptureScreenshotRequest {
                purpose: RendererScreenshotPurpose::Print { print_background: true },
                ..RendererCaptureScreenshotRequest::viewport_png()
            },
        )? else {
            panic!("print capture should produce an image");
        };
        let raster = moli_image::decode_png(&print.bytes)?;
        let pixel = |x: usize, y: usize| {
            let offset = (y * raster.width as usize + x) * 4;
            &raster.rgba[offset..offset + 4]
        };
        assert_eq!(pixel(20, 20), [255, 255, 255, 255]);
        assert_eq!(pixel(230, 20), [0, 255, 0, 255]);
        assert_eq!(pixel(230, 70), [0, 0, 255, 255]);
        assert_eq!(page_vm.vm_mut().eval("matchMedia('print').matches")?, "false");
        assert_eq!(page_vm.vm_mut().eval(geometry)?, "[40,40,40,40]");
        let after = page_vm.vm().layout_pass_observability_for_test();
        assert_eq!(after.1, before.1 + 1, "printing must perform exactly one layout");
        assert_eq!(after.3, before.3, "geometry keeps its published pass metrics");
        assert_eq!(page_vm.vm().layout_snapshot_cache_observability_for_test().2, published_before);

        for x in [20.0, 180.0] {
            for (event, buttons) in [("mousedown", 1), ("mouseup", 0)] {
                page_vm.vm_mut().dispatch_mouse_event_at_point(
                    x, 20.0, event, 0, Some(buttons), 0.0, 0.0,
                )?;
            }
        }
        assert_eq!(page_vm.vm_mut().eval("String(clicks)")?, "1");
        assert_eq!(page_vm.vm().layout_pass_observability_for_test().1, after.1);

        page_vm.capture_screenshot(RendererCaptureScreenshotRequest::viewport_png())?;
        assert_eq!(page_vm.vm_mut().eval(geometry)?, "[60,60,60,60]");
        assert_eq!(page_vm.vm().layout_snapshot_cache_observability_for_test().2, published_before + 1);
        Ok::<_, anyhow::Error>(())
    })
    .await
    .expect("print should preserve the interactive layout");
}
