//! The time the editor shell takes per frame on the CPU: building the UI and tessellating it.
//!
//! ```text
//! cargo test -p cuboid-ui --release --test timing -j 4 -- --ignored --nocapture
//! ```
//!
//! The target of ui_v2_spec 12 is 2 ms per frame at 1920 x 1080. The test prints the mean and
//! the slowest frame and fails above 4 ms, twice the target, so a debug build does not pass it by
//! accident of a noisy run.

mod common;

use std::time::{Duration, Instant};

use common::Fixture;
use cuboid_ui::{ColorMode, ComingFrom, FontSet, Mode, Prefs, Ui, ViewportFrame};
use cuboid_ui_kit::Tokens;
use egui::{RawInput, Rect, TextureOptions, pos2};

const WARM_UP: usize = 30;
const FRAMES: usize = 300;

/// Mean and slowest frame time of the editor in a mode.
fn measure(mode: Mode, light: bool) -> (Duration, Duration, usize) {
    let mut prefs = Prefs {
        color_mode: if light {
            ColorMode::Light
        } else {
            ColorMode::Dark
        },
        ..Prefs::default()
    };
    prefs.apply_coming_from(ComingFrom::Blockbench);
    prefs.accent_chosen = true;
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_mode(mode);
    let mut fx = Fixture::new();
    let ctx = egui::Context::default();
    let screen = Rect::from_min_max(pos2(0.0, 0.0), pos2(1920.0, 1080.0));
    let mut times = Vec::with_capacity(FRAMES);
    let mut vertices = 0;
    for frame in 0..WARM_UP + FRAMES {
        let input = RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            ..RawInput::default()
        };
        let started = Instant::now();
        let output = ctx.run_ui(input, |root| {
            if frame == 0 {
                let tokens = Tokens::of(root.ctx());
                let image = common::scene_image([64, 64], &tokens);
                let handle =
                    root.ctx()
                        .load_texture("timing_scene", image, TextureOptions::NEAREST);
                fx.view = Some(ViewportFrame {
                    texture: Some(handle.id()),
                    size_px: [64, 64],
                });
                std::mem::forget(handle);
            }
            let cx = fx.cx();
            ui.show(root, &cx);
        });
        let ppp = output.pixels_per_point;
        let meshes = ctx.tessellate(output.shapes.clone(), ppp);
        let elapsed = started.elapsed();
        vertices = meshes
            .iter()
            .map(|clipped| match &clipped.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
        output.drop_without_applying_deltas();
        if frame >= WARM_UP {
            times.push(elapsed);
        }
    }
    let total: Duration = times.iter().sum();
    let mean = total / u32::try_from(times.len()).unwrap_or(1);
    (mean, times.into_iter().max().unwrap_or_default(), vertices)
}

#[test]
#[ignore = "a timing; run by hand in a release build"]
fn the_editor_shell_builds_and_tessellates_within_the_frame_budget() {
    let mut worst = Duration::ZERO;
    for mode in Mode::ALL {
        for light in [false, true] {
            let (mean, slowest, vertices) = measure(mode, light);
            println!(
                "{} {}: mean {:.3} ms, slowest {:.3} ms, {vertices} vertices",
                mode.name(),
                if light { "Latte" } else { "Mocha" },
                mean.as_secs_f64() * 1000.0,
                slowest.as_secs_f64() * 1000.0
            );
            worst = worst.max(mean);
        }
    }
    assert!(
        worst < Duration::from_millis(4),
        "the mean frame took {worst:?}"
    );
}
