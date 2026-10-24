//! Renders the start screen to PNG files, to judge the design by eye.
//!
//! ```text
//! cargo test -p cuboid-ui --test start_preview -- --ignored
//! ```
//!
//! Writes the start screen in a few sizes, themes and states to `target/start-preview/`: the
//! normal screen with sample data, a fresh install, the first-run panel and a 150 % UI scale. The
//! headless renderer prefers a software adapter (WARP on Windows).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_ui::{
    BriefCardView, ColorMode, FontSet, ImageData, LibraryView, Prefs, RecentEntry, SuggestionView,
    Ui, UiCx,
};
use egui_kittest::Harness;

/// What one picture shows.
struct Shot {
    name: &'static str,
    /// Appended to the file name.
    suffix: &'static str,
    mode: ColorMode,
    /// Size of the picture in pixels.
    size: [u32; 2],
    scale: f32,
    /// Sample recent files and library, or a fresh install.
    sample: bool,
    /// The camera and the accent are not chosen yet.
    first_run: bool,
}

struct Data {
    recent: Vec<RecentEntry>,
    library: LibraryView,
}

/// A 96 x 64 placeholder picture: a gradient with a shaded box.
fn thumbnail(seed: u32) -> ImageData {
    const W: usize = 96;
    const H: usize = 64;
    let hue: [u8; 3] = [
        [0xb9, 0x8b, 0x52],
        [0x7f, 0xc4, 0xc9],
        [0x8a, 0x8f, 0x94],
        [0xc9, 0x4f, 0x4f],
        [0x6f, 0xae, 0x4f],
        [0x8e, 0x6b, 0xbf],
    ][seed as usize % 6];
    let shade = |k: f32| hue.map(|c| (f32::from(c) * k) as u8);
    let mut rgba = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        for x in 0..W {
            let (bx, by) = (x as i32 - 48, y as i32 - 34);
            let half = 12 + (seed % 3) as i32 * 3;
            let px = if bx.abs() < half && by.abs() < half {
                // Top, front and side faces of the box.
                if by < -half / 2 {
                    shade(1.0)
                } else if bx < 0 {
                    shade(0.74)
                } else {
                    shade(0.55)
                }
            } else {
                let g = 38 + (y * 22 / H) as u8;
                [g, g + 2, g + 6]
            };
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    ImageData {
        size: [W, H],
        rgba: Arc::from(rgba),
        rev: u64::from(seed) + 1,
    }
}

fn sample_data() -> Data {
    let files = [
        ("lantern.cubik", "C:\\models\\blocks", true),
        ("oak_stool.cubik", "C:\\models\\furniture", true),
        ("iron_sword.bbmodel", "C:\\models\\items", true),
        ("campfire_ring.cubik", "C:\\models\\blocks", true),
        ("fox_plush.cub", "C:\\models\\mobs", false),
        ("market_stall.bbmodel", "C:\\models\\props", false),
    ];
    let recent: Vec<RecentEntry> = files
        .iter()
        .enumerate()
        .map(|(i, (name, dir, thumb))| RecentEntry {
            path: PathBuf::from(dir).join(name),
            name: (*name).to_owned(),
            detail: format!("{dir}  -  {} days ago", i + 1),
            thumbnail: thumb.then(|| thumbnail(i as u32)),
        })
        .collect();
    let library = LibraryView {
        rev: 1,
        busy: false,
        counts: [42, 118, 9, 23, 7],
        total: 199,
        matches: 199,
        brief: BriefCardView {
            project_name: "Mossy Hollow".to_owned(),
            mc_version: "1.21.6".to_owned(),
            style_tags: vec!["pixel".to_owned(), "low-poly".to_owned()],
            palette: vec![
                [0xb9, 0x8b, 0x52],
                [0x8a, 0x5a, 0x33],
                [0x5e, 0x3b, 0x22],
                [0xd9, 0xc0, 0x8a],
                [0x7f, 0xc4, 0xc9],
                [0x3f, 0x8f, 0x9b],
                [0x6f, 0xae, 0x4f],
                [0x4a, 0x7a, 0x38],
            ],
            density: Some(16.0),
            models: 42,
            textures: 118,
        },
        suggestions: Arc::from(vec![
            SuggestionView {
                title: "3 models use a different texel density".to_owned(),
                detail:
                    "lantern, oak_stool and market_stall sit between 8 and 32 texels per block."
                        .to_owned(),
            },
            SuggestionView {
                title: "No model for the texture moss_carpet".to_owned(),
                detail: "Create a flat item or a block that uses it.".to_owned(),
            },
        ]),
        pinned: Arc::from(vec![recent[0].path.clone(), recent[2].path.clone()]),
        ..LibraryView::default()
    };
    Data { recent, library }
}

struct App {
    ui: Ui,
    data: Data,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App { ui: cuboid, data } = app;
    let mut cx = UiCx::empty(&data.recent);
    cx.library = Some(&data.library);
    // The harness wraps the app in a padded frame; the window should fill the whole picture.
    let full = ui.ctx().content_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(full));
    child.set_clip_rect(full);
    cuboid.show(&mut child, &cx);
}

fn render(shot: &Shot, out_dir: &Path) {
    let mut prefs = Prefs {
        color_mode: shot.mode,
        nav_chosen: !shot.first_run,
        accent_chosen: !shot.first_run,
        ui_scale: shot.scale,
        ..Prefs::default()
    };
    prefs.sanitize();
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.show_start();
    let data = if shot.sample {
        sample_data()
    } else {
        Data {
            recent: Vec::new(),
            library: LibraryView {
                busy: false,
                ..LibraryView::default()
            },
        }
    };
    // The UI scale is the context's zoom factor: the picture keeps its pixel size.
    let points = egui::vec2(shot.size[0] as f32, shot.size[1] as f32);
    let mut harness = Harness::builder()
        .with_size(points)
        .wgpu()
        .build_ui_state(frame, App { ui, data });
    harness.run_steps(4);
    let picture = harness.render().expect("headless render");
    let size = [picture.width(), picture.height()];
    let path = out_dir.join(format!(
        "{}_{}x{}{}.png",
        shot.name, shot.size[0], shot.size[1], shot.suffix
    ));
    picture.save(&path).expect("write the PNG");
    println!("wrote {} ({} x {})", path.display(), size[0], size[1]);
}

#[test]
#[ignore = "writes PNG files to target/start-preview; run by hand"]
fn render_start_previews() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/start-preview");
    std::fs::create_dir_all(&out_dir).expect("create the output folder");
    let base = Shot {
        name: "start_mocha",
        suffix: "",
        mode: ColorMode::Dark,
        size: [1280, 800],
        scale: 1.0,
        sample: true,
        first_run: false,
    };
    render(&base, &out_dir);
    render(
        &Shot {
            name: "start_latte",
            mode: ColorMode::Light,
            ..base
        },
        &out_dir,
    );
    render(
        &Shot {
            size: [1920, 1080],
            ..base
        },
        &out_dir,
    );
    render(
        &Shot {
            name: "start_mocha_empty",
            sample: false,
            ..base
        },
        &out_dir,
    );
    render(
        &Shot {
            name: "start_firstrun_mocha",
            first_run: true,
            ..base
        },
        &out_dir,
    );
    render(
        &Shot {
            suffix: "_scale150",
            scale: 1.5,
            ..base
        },
        &out_dir,
    );
}
