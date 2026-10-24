//! Renders the main window with a sample scene to PNG files, to judge the theme by eye.
//!
//! ```text
//! cargo test -p cuboid-ui --test theme_preview -- --ignored
//! ```
//!
//! Writes `mocha.png`, `latte.png`, `graphite.png`, `graphite_light.png` and the high-contrast
//! `mocha_hc.png` and `latte_hc.png` (1280 x 800) to `target/theme-preview/`. The headless renderer prefers a software adapter (WARP on Windows).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_ui::{
    ColorMode, DocView, FaceView, FontSet, HistoryView, ImageData, Level, NodeId, NodeKind,
    NodeView, PaletteView, Prefs, PropsView, RecentEntry, TextureView, Ui, UiCx, ViewportFrame,
    ViewportPlacement, Zone,
};
use cuboid_ui_kit::{ThemeFamily, Tokens};
use egui::{Color32, ColorImage, TextureOptions, vec2};
use egui_kittest::Harness;
use glam::Vec3;

const SIZE: [u32; 2] = [1280, 800];

fn node(id: u64, parent: Option<u64>, name: &str, kind: NodeKind) -> NodeView {
    NodeView {
        id: NodeId(id),
        parent: parent.map(NodeId),
        name: Arc::from(name),
        kind,
        visible: id != 7,
        locked: id == 8,
    }
}

/// A 16 x 16 brick pattern.
fn texture_pixels() -> Arc<[u8]> {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16_u32 {
        for x in 0..16_u32 {
            let row = y / 4;
            let mortar = y % 4 == 3 || (x + if row % 2 == 0 { 0 } else { 4 }) % 8 == 7;
            let shade = 150 + ((x * 7 + y * 13) % 5) as u8 * 8;
            let px = if mortar {
                [92, 88, 80, 255]
            } else {
                [shade + 40, shade / 2 + 30, shade / 3 + 20, 255]
            };
            rgba.extend_from_slice(&px);
        }
    }
    Arc::from(rgba)
}

struct Fixture {
    nodes: Vec<NodeView>,
    selection: Vec<NodeId>,
    props: PropsView,
    textures: Vec<TextureView>,
    palette: PaletteView,
    history: HistoryView,
    recent: Vec<RecentEntry>,
    view: Option<ViewportFrame>,
}

impl Fixture {
    fn new() -> Fixture {
        let image = ImageData {
            size: [16, 16],
            rgba: texture_pixels(),
            rev: 1,
        };
        let colors: Vec<[f32; 4]> = [
            0xb98b52_u32,
            0x8a5a33,
            0x5e3b22,
            0xd9c08a,
            0x7fc4c9,
            0x3f8f9b,
            0x8a8f94,
            0x4c5157,
            0xe4e1d6,
            0x2c2f33,
            0xc94f4f,
            0xe39a3b,
            0x6fae4f,
            0x4a7a38,
            0x8e6bbf,
            0x5a4a8a,
        ]
        .iter()
        .map(|hex| {
            let [_, r, g, b] = hex.to_be_bytes();
            [r, g, b, 255].map(|v| f32::from(v) / 255.0)
        })
        .collect();
        Fixture {
            nodes: vec![
                node(1, None, "Lantern", NodeKind::Group),
                node(2, Some(1), "Frame", NodeKind::Element),
                node(3, Some(1), "Glass", NodeKind::Element),
                node(4, Some(1), "Cap", NodeKind::Element),
                node(5, None, "Chain", NodeKind::Group),
                node(6, Some(5), "Link A", NodeKind::Element),
                node(7, Some(5), "Link B", NodeKind::Element),
                node(8, None, "Hook", NodeKind::Mesh),
                node(9, None, "Voxel layer", NodeKind::Voxels),
            ],
            selection: vec![NodeId(3), NodeId(6)],
            props: PropsView {
                kind: NodeKind::Element,
                name: Arc::from("Glass"),
                selected: 2,
                position: Vec3::new(-1.0, 0.5, -1.0),
                size: Vec3::new(2.0, 3.0, 2.0),
                rotation: Vec3::new(0.0, 22.5, 0.0),
                pivot: Vec3::new(0.0, 0.0, 0.0),
                color: [0.5, 0.77, 0.79, 1.0],
                shade: true,
                uv_snap: true,
                faces: Some(std::array::from_fn(|i| FaceView {
                    enabled: i != 1,
                    texture: Arc::from("lantern"),
                    uv: [0.0, 0.0, 4.0 + 2.0 * i as f32, 8.0],
                    tint: -1,
                })),
            },
            textures: vec![
                TextureView {
                    label: Arc::from("lantern"),
                    size: [16, 16],
                    image: Some(image),
                },
                TextureView {
                    label: Arc::from("particle"),
                    size: [16, 16],
                    image: None,
                },
            ],
            palette: PaletteView {
                name: Arc::from("Stone and glass"),
                left: colors[4],
                right: colors[10],
                colors: Arc::from(colors),
            },
            history: HistoryView {
                labels: Arc::from(vec![
                    Arc::from("Create cube"),
                    Arc::from("Resize"),
                    Arc::from("Move"),
                    Arc::from("Rename"),
                ]),
                cursor: 3,
            },
            recent: Vec::new(),
            view: None,
        }
    }

    fn cx(&self) -> UiCx<'_> {
        let mut cx = UiCx::empty(&self.recent);
        cx.views[3] = self.view;
        cx.doc = Some(DocView {
            title: "lantern.cubik",
            path: Some(Path::new("C:/models/lantern.cubik")),
            dirty: true,
            nodes: &self.nodes,
            nodes_rev: 1,
            selection: &self.selection,
            primary: self.selection.first().copied(),
            props: Some(&self.props),
            textures: &self.textures,
            active_texture: Some(0),
            palette: &self.palette,
            history: &self.history,
            display: None,
            status: "2 elements selected",
        });
        cx
    }
}

struct App {
    ui: Ui,
    fx: Fixture,
    placement: Option<ViewportPlacement>,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        fx,
        placement,
    } = app;
    let cx = fx.cx();
    // The harness wraps the app in a padded frame; the window should fill the whole picture.
    let full = ui.ctx().content_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(full));
    child.set_clip_rect(full);
    let out = cuboid.show(&mut child, &cx);
    if let Some(first) = out.viewports.first() {
        *placement = Some(*first);
    }
}

/// A stand-in for the renderer: an isometric grid and a few shaded boxes, drawn on the CPU.
struct Canvas {
    image: ColorImage,
    origin: egui::Vec2,
    scale: f32,
}

impl Canvas {
    fn project(&self, p: [f32; 3]) -> egui::Pos2 {
        let (cos, sin) = (0.866_f32, 0.5_f32);
        egui::pos2(
            self.origin.x + (p[0] - p[2]) * cos * self.scale,
            self.origin.y + ((p[0] + p[2]) * sin - p[1]) * self.scale,
        )
    }

    fn put(&mut self, x: i32, y: i32, color: Color32) {
        let [w, h] = self.image.size;
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return;
        }
        self.image.pixels[y as usize * w + x as usize] = color;
    }

    fn line(&mut self, a: egui::Pos2, b: egui::Pos2, width: i32, color: Color32) {
        let steps = (b - a).length().ceil().max(1.0) as i32;
        for i in 0..=steps {
            let p = a + (b - a) * (i as f32 / steps as f32);
            for dy in 0..width {
                for dx in 0..width {
                    self.put(
                        p.x as i32 + dx - width / 2,
                        p.y as i32 + dy - width / 2,
                        color,
                    );
                }
            }
        }
    }

    fn quad(&mut self, q: [egui::Pos2; 4], fill: Color32, edge: Color32, edge_width: i32) {
        let min_x = q.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor() as i32;
        let max_x = q.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as i32;
        let min_y = q.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor() as i32;
        let max_y = q.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                let side = |a: egui::Pos2, b: egui::Pos2| {
                    (b - a).x * (p.y - a.y) - (b - a).y * (p.x - a.x)
                };
                let signs = [
                    side(q[0], q[1]),
                    side(q[1], q[2]),
                    side(q[2], q[3]),
                    side(q[3], q[0]),
                ];
                if signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0) {
                    self.put(x, y, fill);
                }
            }
        }
        for i in 0..4 {
            self.line(q[i], q[(i + 1) % 4], edge_width, edge);
        }
    }

    /// Draws the three faces of a box that face the camera.
    fn cuboid(
        &mut self,
        min: [f32; 3],
        size: [f32; 3],
        base: Color32,
        edge: Color32,
        edge_width: i32,
    ) {
        let [x, y, z] = min;
        let [w, h, d] = size;
        let p = |dx: f32, dy: f32, dz: f32| self.project([x + dx * w, y + dy * h, z + dz * d]);
        let shade = |k: f32| {
            Color32::from_rgb(
                (f32::from(base.r()) * k) as u8,
                (f32::from(base.g()) * k) as u8,
                (f32::from(base.b()) * k) as u8,
            )
        };
        let top = [
            p(0.0, 1.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(1.0, 1.0, 1.0),
            p(0.0, 1.0, 1.0),
        ];
        let front_z = [
            p(0.0, 1.0, 1.0),
            p(1.0, 1.0, 1.0),
            p(1.0, 0.0, 1.0),
            p(0.0, 0.0, 1.0),
        ];
        let front_x = [
            p(1.0, 1.0, 1.0),
            p(1.0, 1.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 0.0, 1.0),
        ];
        self.quad(top, shade(1.0), edge, edge_width);
        self.quad(front_z, shade(0.74), edge, edge_width);
        self.quad(front_x, shade(0.55), edge, edge_width);
    }
}

fn scene_image(size: [usize; 2], t: &Tokens) -> ColorImage {
    let [w, h] = size;
    let mut canvas = Canvas {
        image: ColorImage::filled(size, t.bg_viewport),
        origin: vec2(w as f32 * 0.5, h as f32 * 0.66),
        scale: h as f32 / 15.0,
    };
    let grid = t.bg_viewport.lerp_to_gamma(t.text, 0.16);
    for i in -8..=8 {
        let i = i as f32;
        let (a, b) = (
            canvas.project([i, 0.0, -8.0]),
            canvas.project([i, 0.0, 8.0]),
        );
        canvas.line(a, b, 1, grid);
        let (a, b) = (
            canvas.project([-8.0, 0.0, i]),
            canvas.project([8.0, 0.0, i]),
        );
        canvas.line(a, b, 1, grid);
    }
    let (a, b) = (
        canvas.project([-8.0, 0.0, 0.0]),
        canvas.project([8.0, 0.0, 0.0]),
    );
    canvas.line(a, b, 2, Color32::from_rgb(0xd0, 0x62, 0x58));
    let (a, b) = (
        canvas.project([0.0, 0.0, -8.0]),
        canvas.project([0.0, 0.0, 8.0]),
    );
    canvas.line(a, b, 2, Color32::from_rgb(0x58, 0x88, 0xd0));
    let edge = |c: Color32| c.lerp_to_gamma(Color32::BLACK, 0.55);
    let wood = Color32::from_rgb(0xb9, 0x8b, 0x52);
    let metal = Color32::from_rgb(0x8a, 0x8f, 0x94);
    let glass = Color32::from_rgb(0x7f, 0xc4, 0xc9);
    let stone = Color32::from_rgb(0xa8, 0xa4, 0x98);
    // Back to front.
    canvas.cuboid([2.5, 0.0, -3.5], [2.0, 1.0, 2.0], wood, edge(wood), 1);
    canvas.cuboid([3.0, 1.0, -3.0], [1.0, 1.0, 1.0], stone, edge(stone), 1);
    canvas.cuboid([-1.5, 0.0, -1.5], [3.0, 0.5, 3.0], metal, edge(metal), 1);
    canvas.cuboid([-1.0, 0.5, -1.0], [2.0, 3.0, 2.0], glass, t.accent, 2);
    canvas.cuboid([-1.5, 3.5, -1.5], [3.0, 0.5, 3.0], metal, edge(metal), 1);
    canvas.cuboid([-0.25, 4.0, -0.25], [0.5, 1.5, 0.5], wood, edge(wood), 1);
    canvas.image
}

fn render(name: &str, color_mode: ColorMode, graphite: bool, high_contrast: bool, out_dir: &Path) {
    let mut prefs = Prefs {
        color_mode,
        high_contrast,
        nav_chosen: true,
        ..Prefs::default()
    };
    if graphite {
        prefs.family = ThemeFamily::Graphite;
        prefs.accent = ThemeFamily::Graphite.default_accent();
    }
    let bottom = prefs.layout.zone_mut(Zone::Bottom);
    bottom.open = true;
    bottom.active = 1;
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
