//! Renders the main window with a sample scene to PNG files, to judge the theme families by eye.
//!
//! ```text
//! cargo test -p cuboid-ui --test theme_preview -j 4 -- --ignored
//! ```
//!
//! Writes `mocha.png`, `latte.png`, `graphite.png`, `graphite_light.png` and the high-contrast
//! `mocha_hc.png` and `latte_hc.png` (1280 x 800) to `target/theme-preview/`. The headless
//! renderer prefers a software adapter (WARP on Windows).

mod common;

use std::path::{Path, PathBuf};

use common::{App, Fixture, frame, scene_image};
use cuboid_ui::{ColorMode, ComingFrom, FontSet, Level, Prefs, Ui, ViewportFrame};
use cuboid_ui_kit::{ThemeFamily, Tokens};
use egui::{TextureOptions, vec2};
use egui_kittest::Harness;

const SIZE: [u32; 2] = [1280, 800];

fn render(name: &str, color_mode: ColorMode, graphite: bool, high_contrast: bool, out_dir: &Path) {
    let mut prefs = Prefs {
        color_mode,
        high_contrast,
        ..Prefs::default()
    };
    prefs.apply_coming_from(ComingFrom::Blockbench);
    prefs.accent_chosen = true;
    if graphite {
        prefs.family = ThemeFamily::Graphite;
        prefs.accent = ThemeFamily::Graphite.default_accent();
    }
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.toast(Level::Success, "Saved lantern.cubik");
    let app = App {
        ui,
        fx: Fixture::new(),
        placement: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(SIZE[0] as f32, SIZE[1] as f32))
        .wgpu()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    let tokens = Tokens::of(&harness.ctx);
    let placement = harness.state().placement.expect("the main view is drawn");
    let [w, h] = placement.size_px;
    let image = scene_image([w as usize, h as usize], &tokens);
    let handle = harness
        .ctx
        .load_texture("theme_preview_scene", image, TextureOptions::NEAREST);
    harness.state_mut().fx.view = Some(ViewportFrame {
        texture: Some(handle.id()),
        size_px: placement.size_px,
    });
    harness.run_steps(3);
    let picture = harness.render().expect("headless render");
    assert_eq!([picture.width(), picture.height()], SIZE);
    let path: PathBuf = out_dir.join(format!("{name}.png"));
    picture.save(&path).expect("write the PNG");
    println!("wrote {}", path.display());
}

#[test]
#[ignore = "writes PNG files to target/theme-preview; run by hand"]
fn render_theme_previews() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/theme-preview");
    std::fs::create_dir_all(&out_dir).expect("create the output folder");
    render("mocha", ColorMode::Dark, false, false, &out_dir);
    render("latte", ColorMode::Light, false, false, &out_dir);
    render("graphite", ColorMode::Dark, true, false, &out_dir);
    render("graphite_light", ColorMode::Light, true, false, &out_dir);
    render("mocha_hc", ColorMode::Dark, false, true, &out_dir);
    render("latte_hc", ColorMode::Light, false, true, &out_dir);
}
