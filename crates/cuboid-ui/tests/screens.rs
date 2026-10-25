//! Renders every screen and state to PNG files for the review of the UI against ui_v2_spec 12:
//! the start screen (data, empty, first run) and the editor shell in each workspace with a
//! selection, at 1280 x 800 and 1920 x 1080, in Mocha and Latte, at 100 % and 150 %, and the
//! editor with the Cubik arrangement once.
//!
//! ```text
//! cargo test -p cuboid-ui --test screens -j 4 -- --ignored
//! ```
//!
//! The files go to `target/ui-screens/`, named `{screen}_{state}_{theme}_{scale}_{width}.png`. The
//! headless renderer prefers a software adapter (WARP on Windows). The scene in the viewport is
//! drawn on the CPU.

mod common;

use std::path::Path;

use common::{App, Fixture, frame, scene_image};
use cuboid_ui::{ColorMode, ComingFrom, FontSet, Mode, Prefs, Ui, ViewportFrame};
use cuboid_ui_kit::Tokens;
use egui::{TextureOptions, vec2};
use egui_kittest::Harness;

/// What a picture shows.
#[derive(Copy, Clone)]
enum Screen {
    StartData,
    StartEmpty,
    StartFirstRun,
    Editor(Mode),
}

impl Screen {
    /// The screen and state parts of the file name.
    fn name(self) -> (&'static str, &'static str) {
        match self {
            Screen::StartData => ("start", "data"),
            Screen::StartEmpty => ("start", "empty"),
            Screen::StartFirstRun => ("start", "firstrun"),
            Screen::Editor(Mode::Model) => ("editor", "model"),
            Screen::Editor(Mode::Voxel) => ("editor", "voxel"),
            Screen::Editor(Mode::Paint) => ("editor", "paint"),
            Screen::Editor(Mode::Display) => ("editor", "display"),
        }
    }
}

/// Where in the matrix a picture is.
#[derive(Copy, Clone)]
struct Variant {
    light: bool,
    scale: f32,
    size: [u32; 2],
}

fn app_for(screen: Screen, variant: Variant, from: ComingFrom) -> App {
    let first_run = matches!(screen, Screen::StartFirstRun);
    let mut prefs = Prefs {
        color_mode: if variant.light {
            ColorMode::Light
        } else {
            ColorMode::Dark
        },
        ui_scale: variant.scale,
        ..Prefs::default()
    };
    if !first_run {
        prefs.apply_coming_from(from);
        prefs.accent_chosen = true;
    }
    prefs.sanitize();
    let mut ui = Ui::new(prefs, FontSet::default());
    let fx = match screen {
        Screen::StartData => {
            ui.show_start();
            Fixture::new().start()
        }
        Screen::StartEmpty | Screen::StartFirstRun => {
            ui.show_start();
            Fixture::new().empty()
        }
        Screen::Editor(mode) => {
            ui.set_mode(mode);
            Fixture::new()
        }
    };
    App {
        ui,
        fx,
        placement: None,
    }
}

fn render(screen: Screen, variant: Variant, from: ComingFrom, out_dir: &Path, name: &str) {
    let app = app_for(screen, variant, from);
    let editor = app.fx.has_doc;
    // The UI scale is the context's zoom factor: the picture keeps its pixel size.
    let size = vec2(variant.size[0] as f32, variant.size[1] as f32);
    let mut harness = Harness::builder()
        .with_size(size)
        .wgpu()
        .build_ui_state(frame, app);
    harness.run_steps(4);
    if editor {
        let tokens = Tokens::of(&harness.ctx);
        let placement = harness.state().placement.expect("the main view is drawn");
        let [w, h] = placement.size_px;
        let image = scene_image([w as usize, h as usize], &tokens);
        let handle = harness
            .ctx
            .load_texture("screens_scene", image, TextureOptions::NEAREST);
        harness.state_mut().fx.view = Some(ViewportFrame {
            texture: Some(handle.id()),
            size_px: placement.size_px,
        });
        harness.run_steps(4);
    }
    let picture = harness.render().expect("headless render");
    assert_eq!(
        [picture.width(), picture.height()],
        variant.size,
        "{name}: the picture keeps the pixel size"
    );
    let path = out_dir.join(format!("{name}.png"));
    picture.save(&path).expect("write the PNG");
}

fn file_name(screen: Screen, variant: Variant) -> String {
    let (kind, state) = screen.name();
    let theme = if variant.light { "latte" } else { "mocha" };
    let scale = (variant.scale * 100.0).round() as u32;
    format!("{kind}_{state}_{theme}_{scale}_{}", variant.size[0])
}

#[test]
#[ignore = "writes PNG files to target/ui-screens; run by hand"]
fn render_the_screen_set() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-screens");
    std::fs::create_dir_all(&out_dir).expect("create the output folder");
    let screens = [
        Screen::StartData,
        Screen::StartEmpty,
        Screen::StartFirstRun,
        Screen::Editor(Mode::Model),
        Screen::Editor(Mode::Voxel),
        Screen::Editor(Mode::Paint),
        Screen::Editor(Mode::Display),
    ];
    // `SCREENS_ONLY=editor_paint` renders the pictures whose name contains the text.
    let only = std::env::var("SCREENS_ONLY").unwrap_or_default();
    let mut count = 0;
    for screen in screens {
        for size in [[1280, 800], [1920, 1080]] {
            for light in [false, true] {
                for scale in [1.0, 1.5] {
                    let variant = Variant { light, scale, size };
                    let name = file_name(screen, variant);
                    if !name.contains(&only) {
                        continue;
                    }
                    render(screen, variant, ComingFrom::Blockbench, &out_dir, &name);
                    count += 1;
                }
            }
        }
    }
    // The Cubik arrangement: properties on the left, outliner on the right.
    let variant = Variant {
        light: false,
        scale: 1.0,
        size: [1920, 1080],
    };
    let name = "editor_cubik_mocha_100_1920";
    if name.contains(&only) {
        render(
            Screen::Editor(Mode::Model),
            variant,
            ComingFrom::Cubik,
            &out_dir,
            name,
        );
        count += 1;
    }
    println!("wrote {count} pictures to {}", out_dir.display());
}
