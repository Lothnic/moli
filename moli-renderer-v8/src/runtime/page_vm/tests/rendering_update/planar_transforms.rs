use super::*;

#[tokio::test(flavor = "current_thread")]
async fn planar_3d_functions_match_chromium_popover_geometry_paint_and_hit_testing() {
    run_page_vm_async_test(async move {
        let loader =
            crate::network::ResourceRequestClient::new(&FetchConfig::default()).expect("loader");
        let mut page_vm = test_page_vm_with_loader_and_document_url(
            &loader,
            Vec::new(),
            Url::parse("https://example.com/planar-transforms.html")?,
        );
        page_vm.vm_mut().eval(
            r#"
document.head.innerHTML = `<style>
html,body{margin:0;background:white}
.anchor{position:absolute;width:50px;height:50px;background:blue}
#bottom{left:200px;top:20px}#right{left:20px;top:220px}
.popover{position:absolute;box-sizing:border-box;border:1px solid black;background:red}
#bottom-popover{top:100%;left:50%;width:361px;height:80px;transform:translate3d(-50%,0,0)}
#right-popover{top:50%;left:100%;width:120px;height:81px;transform:translate3d(0,-50%,0)}
#content{position:absolute;left:10px;top:10px;width:20px;height:20px;background:lime}
#composed{position:absolute;left:260px;top:220px;width:80px;height:40px;background:purple;transform-origin:0 0;transform:translate3d(10px,-20px,0) rotateZ(90deg) scale3d(2,1,1)}
#matrix{position:absolute;left:400px;top:220px;width:80px;height:40px;background:orange;transform:matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,-35,15,0,1)}
#zero-z{position:absolute;left:400px;top:320px;width:40px;height:20px;background:teal;transform:translateZ(0) translate(15px,-5px)}
</style>`;
document.body.innerHTML = `<div id=bottom class=anchor><div id=bottom-popover class=popover><div id=content></div></div></div>
<div id=right class=anchor><div id=right-popover class=popover></div></div>
<div id=composed></div><div id=matrix></div><div id=zero-z></div>`;
'installed'
"#,
        )?;
        page_vm.vm_mut().sync_live_document_style_sources();
        let viewport = moli_layout::PaintViewport::new(600, 400, 1.0);
        let snapshot = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("planar transform fixture must retain a layout root");
        assert!(
            snapshot
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "transform-3d-geometry-fallback"),
            "3D function syntax with a 2D matrix must not trigger a fallback"
        );

        let result = page_vm.vm_mut().eval(
            r#"JSON.stringify((()=>{
const ids=['bottom','bottom-popover','content','right','right-popover','composed','matrix','zero-z'];
const rect=id=>{const r=document.getElementById(id).getBoundingClientRect();return [r.x,r.y,r.width,r.height]};
return {geometry:Object.fromEntries(ids.map(id=>[id,rect(id)])),
hits:[[50,75],[60,90],[80,210],[240,220],[370,240],[420,320],[410,80]].map(([x,y])=>document.elementFromPoint(x,y)?.id||null)};
})())"#,
        )?;
        let result: serde_json::Value = serde_json::from_str(&result)?;
        // Recorded in Chromium. Odd border-box sizes also pin the percentage
        // reference box and preserve the half-pixel centering translation.
        for (id, expected) in [
            ("bottom", [200.0, 20.0, 50.0, 50.0]),
            ("bottom-popover", [44.5, 70.0, 361.0, 80.0]),
            ("content", [55.5, 81.0, 20.0, 20.0]),
            ("right", [20.0, 220.0, 50.0, 50.0]),
            ("right-popover", [70.0, 204.5, 120.0, 81.0]),
            ("composed", [230.0, 200.0, 40.0, 160.0]),
            ("matrix", [365.0, 235.0, 80.0, 40.0]),
            ("zero-z", [415.0, 315.0, 40.0, 20.0]),
        ] {
            for (index, expected) in expected.into_iter().enumerate() {
                let actual = result["geometry"][id][index]
                    .as_f64()
                    .expect("numeric geometry");
                assert!(
                    (actual - expected).abs() <= 0.001,
                    "{id}[{index}]: expected {expected}, got {actual}"
                );
            }
        }
        assert_eq!(
            result["hits"],
            serde_json::json!([
                "bottom-popover", "content", "right-popover", "composed", "matrix", "zero-z", null
            ]),
        );

        let image = moli_paint::raster_snapshot(&snapshot)?;
        let pixel = |x: u32, y: u32| {
            let index = ((y * image.width + x) * 4) as usize;
            <[u8; 4]>::try_from(&image.rgba[index..index + 4]).expect("RGBA pixel")
        };
        for (point, expected) in [
            ((50, 75), [255, 0, 0, 255]),
            ((60, 90), [0, 255, 0, 255]),
            ((80, 210), [255, 0, 0, 255]),
            ((240, 220), [128, 0, 128, 255]),
            ((370, 240), [255, 165, 0, 255]),
            ((420, 320), [0, 128, 128, 255]),
            ((410, 80), [255, 255, 255, 255]),
        ] {
            assert_eq!(pixel(point.0, point.1), expected, "pixel at {point:?}");
        }

        // A nonzero z translation still cannot be represented in 2D. Retain
        // the existing diagnostic and conservative geometry for that case.
        page_vm.vm_mut().eval(
            r#"
const depth = document.createElement('div');
depth.id = 'depth';
depth.style.cssText = 'position:absolute;left:500px;top:20px;width:40px;height:20px;transform:translate3d(-10px,0,5px)';
document.body.appendChild(depth);
'installed'
"#,
        )?;
        let snapshot = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("depth fixture must retain a layout root");
        assert_eq!(
            snapshot
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "transform-3d-geometry-fallback")
                .count(),
            1,
        );
        assert_eq!(
            page_vm.vm_mut().eval(
                "(()=>{const r=document.getElementById('depth').getBoundingClientRect();return [r.x,r.y,r.width,r.height].join('|')})()",
            )?,
            "500|20|40|20",
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .expect("planar transform fixture should run");
}
