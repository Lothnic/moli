use super::*;

fn dark_pixels(image: &moli_paint::RasterImage, x: u32, y: u32, width: u32, height: u32) -> usize {
    (y..y + height)
        .flat_map(|y| (x..x + width).map(move |x| ((y * image.width + x) * 4) as usize))
        .filter(|&index| {
            image.rgba[index..index + 3]
                .iter()
                .all(|&channel| channel < 128)
        })
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn screenshot_select_listboxes_paint_every_option_and_clip_their_contents() {
    run_page_vm_async_test(async move {
        let loader =
            crate::network::ResourceRequestClient::new(&FetchConfig::default()).expect("loader");
        let mut page_vm = test_page_vm_with_loader_and_document_url(
            &loader,
            Vec::new(),
            Url::parse("https://example.com/select-listbox.html")?,
        );
        page_vm.vm_mut().eval(
            r#"
document.head.innerHTML = `<style>
html,body{margin:0;padding:0;background:white}
select{position:absolute;top:0;width:140px;height:90px;box-sizing:border-box;
       margin:0;padding:0;border:1px solid black;background:white;color:black;font:16px/20px sans-serif}
#multi{left:0}#selected{left:160px}#sized{left:320px}#single{left:480px;height:22px}
#clipped{left:640px;height:36px}
</style>`;
const options = `<option value=a>Alpha</option><option value=b label=Beta>not the label</option><option value=c>\n Gamma \t </option>`;
document.body.innerHTML = `<select id=multi multiple>${options}</select>
<select id=selected multiple>${options}</select><select id=sized size=3>${options}</select>
<select id=single>${options}</select>
<select id=clipped multiple><option>AlphaBetaGammaAlphaBetaGamma</option><option>Beta</option><option>Gamma</option></select>`;
document.getElementById('selected').options[1].selected = true;
document.getElementById('single').value = 'b';
'installed'
"#,
        )?;
        page_vm.vm_mut().sync_live_document_style_sources();
        assert_eq!(
            page_vm.vm_mut().eval(
                "[...document.getElementById('multi').options].map(o=>o.label).join('|') + ';' + document.getElementById('multi').selectedOptions.length",
            )?,
            "Alpha|Beta|Gamma;0",
            "the unselected listbox must still have all three option labels",
        );
        let viewport = moli_layout::PaintViewport::new(800, 120, 1.0);
        let snapshot = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("select fixture must retain a layout root");
        let image = moli_paint::raster_snapshot(&snapshot)?;
        for (name, x) in [("multiple", 0), ("selected multiple", 160), ("size=3", 320)] {
            for (row, label) in ["Alpha", "Beta", "Gamma"].into_iter().enumerate() {
                assert!(
                    dark_pixels(&image, x + 2, 1 + row as u32 * 25, 130, 24) > 10,
                    "{name} must paint {label} on row {row}, independently of selectedness",
                );
            }
            assert_eq!(
                dark_pixels(&image, x + 52, 26, 80, 24),
                0,
                "{name} must use the option's label attribute rather than its longer text content",
            );
        }
        assert!(dark_pixels(&image, 482, 2, 130, 18) > 10);
        assert_eq!(dark_pixels(&image, 482, 24, 130, 60), 0, "a dropdown paints only its selected label");
        assert!(dark_pixels(&image, 642, 2, 130, 24) > 10);
        assert_eq!(dark_pixels(&image, 642, 37, 136, 70), 0, "listbox rows must be clipped vertically");
        assert_eq!(dark_pixels(&image, 781, 2, 19, 32), 0, "long option labels must be clipped horizontally");

        assert_eq!(
            page_vm.vm_mut().eval(
                r#"(() => {
const select = document.getElementById('multi');
select.options[0].label = 'Delta';
select.options[1].remove();
select.options[0].selected = true;
select.options[1].selected = true;
return select.value + ';' + [...select.selectedOptions].map(o=>o.label).join('|');
})()"#,
            )?,
            "a;Delta|Gamma",
        );
        let updated = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("mutated select fixture must retain a layout root");
        let updated = moli_paint::raster_snapshot(&updated)?;
        assert!(dark_pixels(&updated, 2, 1, 130, 24) > 10);
        assert!(dark_pixels(&updated, 2, 26, 130, 24) > 10);
        assert_eq!(dark_pixels(&updated, 2, 51, 130, 24), 0, "removing an option must remove its painted row");
        let first_row_end = (25 * image.width * 4) as usize;
        assert_ne!(
            image.rgba[..first_row_end], updated.rgba[..first_row_end],
            "changing an option label must change the painted text",
        );

        page_vm.vm_mut().eval(
            "document.getElementById('multi').add(new Option('', 'blank'), 1); 'inserted'",
        )?;
        let with_blank = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("select with an empty option must retain a layout root");
        let with_blank = moli_paint::raster_snapshot(&with_blank)?;
        assert!(dark_pixels(&with_blank, 2, 1, 130, 24) > 10);
        assert_eq!(dark_pixels(&with_blank, 2, 26, 130, 24), 0, "an empty option must keep its row");
        assert!(dark_pixels(&with_blank, 2, 51, 130, 24) > 10);

        assert_eq!(page_vm.vm_mut().eval(
            r#"(() => {
const select = document.getElementById('multi');
select.options[0].label = '';
select.size = 1;
select.value = 'c';
select.style.height = '22px';
return select.value + ';' + select.selectedOptions[0].label;
})()"#,
        )?, "c;Gamma");
        let dropdown = page_vm
            .vm_mut()
            .screenshot_layout_snapshot(viewport)?
            .expect("single-row multiple select must retain a layout root");
        let dropdown = moli_paint::raster_snapshot(&dropdown)?;
        assert!(dark_pixels(&dropdown, 2, 2, 130, 18) > 10, "size=1 must paint the selected label even when multiple is set");
        assert_eq!(dark_pixels(&dropdown, 2, 24, 130, 60), 0);
        Ok::<_, anyhow::Error>(())
    })
    .await
    .expect("select listbox rendering fixture should run");
}
