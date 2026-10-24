//! The component gallery.
//!
//! ```text
//! cargo test -p cuboid-ui-kit --test gallery -- --ignored
//! ```
//!
//! Writes a PNG per sheet, theme (Mocha, Latte) and UI scale (100 %, 150 %) to
//! `target/ui-gallery/`, such as `mocha_100_components.png`. The headless renderer prefers a
//! software adapter (WARP on Windows).

mod common;

use std::path::Path;

use common::{Gallery, Sheet, setup, show, theme};
use egui::vec2;
use egui_kittest::Harness;

fn harness(g: Gallery, scale: f32, wgpu: bool) -> Harness<'static, Gallery> {
    let mut builder = Harness::builder()
        .with_size(vec2(1180.0 * scale, 800.0 * scale))
        .with_context_setup(setup);
    if wgpu {
        builder = builder.wgpu();
    }
    builder.build_ui_state(|ui, g: &mut Gallery| show(ui, g), g)
}

#[test]
fn every_sheet_builds_without_missing_glyphs() {
    for sheet in Sheet::ALL {
        for light in [false, true] {
            let mut h = harness(Gallery::new(theme(light, 1.0), sheet), 1.0, false);
            h.run_steps(3);
        }
    }
}

#[test]
#[ignore = "writes PNG files to target/ui-gallery; run by hand"]
fn render_the_gallery() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-gallery");
    std::fs::create_dir_all(&out_dir).expect("create the output folder");
    for (light, theme_name) in [(false, "mocha"), (true, "latte")] {
        for (scale, scale_name) in [(1.0, "100"), (1.5, "150")] {
            for sheet in Sheet::ALL {
                let g = Gallery::new(theme(light, scale), sheet);
                let mut h = harness(g, scale, true);
                h.run_steps(3);
                h.fit_contents();
                h.run_steps(3);
                let picture = h.render().expect("headless render");
                let path = out_dir.join(format!("{theme_name}_{scale_name}_{}.png", sheet.name()));
                picture.save(&path).expect("write the PNG");
                println!(
                    "wrote {} ({}x{})",
                    path.display(),
                    picture.width(),
                    picture.height()
                );
            }
        }
    }
}
