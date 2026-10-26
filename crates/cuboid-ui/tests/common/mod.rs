//! Sample data and a software-drawn scene for the picture and timing tests: a document with a
//! selection, recent files with thumbnails, a library, and a stand-in for the renderer.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use cuboid_ui::{
    AnimationRow, AnimationsView, BoneRow, BriefCardView, BriefInputs, DisplayPreset,
    DisplaySlotView, DisplayView, DocView, Face, FaceView, GroupView, HistoryView, ImageData,
    IssueLevel, IssueView, JobView, KeyKind, KeyMark, KeyNumber, LibraryView, LoopKind, ModelView,
    NodeId, NodeKind, NodeView, PaletteView, PoseView, PropsView, RecentEntry, SuggestionAction,
    SuggestionGoal, SuggestionView, Template, TextureView, TimelineView, Ui, UiCx, ValidatorView,
    ViewportFrame, ViewportPlacement, WindowView,
};
use cuboid_ui_kit::Tokens;
use egui::{Color32, ColorImage, vec2};
use glam::Vec3;

fn node(id: u64, parent: Option<u64>, name: &str, kind: NodeKind) -> NodeView {
    NodeView {
        id: NodeId(id),
        parent: parent.map(NodeId),
        name: Arc::from(name),
        kind,
        visible: id != 7,
        locked: id == 8,
        hidden_above: false,
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

/// A 96 x 64 placeholder picture of a recent file: a gradient with a shaded box.
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

/// The owned data behind a [`UiCx`]: an open document with a selection, recent files, a library
/// and the rendered picture of the main view.
pub struct Fixture {
    pub has_doc: bool,
    pub nodes: Vec<NodeView>,
    pub selection: Vec<NodeId>,
    pub props: PropsView,
    pub textures: Vec<TextureView>,
    pub palette: PaletteView,
    pub history: HistoryView,
    pub recent: Vec<RecentEntry>,
    pub library: Option<LibraryView>,
    pub jobs: Vec<JobView>,
    pub view: Option<ViewportFrame>,
    pub status: &'static str,
    pub animations: Option<AnimationsView>,
    pub timeline: Option<TimelineView>,
    pub display: DisplayView,
    pub model: ModelView,
    pub validator: Option<ValidatorView>,
    pub has_props: bool,
    pub window: WindowView,
}

/// The eight display contexts with the settings of a lantern held in a hand.
fn display_view() -> DisplayView {
    let keys = [
        "thirdperson_righthand",
        "thirdperson_lefthand",
        "firstperson_righthand",
        "firstperson_lefthand",
        "head",
        "gui",
        "ground",
        "fixed",
    ];
    let slots = keys.iter().enumerate().map(|(i, key)| {
        let enabled = i != 1 && i != 3;
        DisplaySlotView {
            name: Arc::from(*key),
            enabled,
            preset: Arc::from(if i == 5 { "block/block" } else { "" }),
            rotation: if i == 5 {
                Vec3::new(30.0, 225.0, 0.0)
            } else {
                Vec3::new(0.0, 180.0, 0.0)
            },
            translation: Vec3::new(0.0, 3.0, 1.0),
            scale: if i == 5 {
                Vec3::splat(0.625)
            } else {
                Vec3::splat(0.55)
            },
        }
    });
    DisplayView {
        slots: Arc::from(slots.collect::<Vec<_>>()),
        active: 5,
    }
}

/// A result with errors, warnings and info that names the nodes of the fixture.
fn validator_view() -> ValidatorView {
    let issue =
        |code: &str, level, objects: &[u64], message: &str, hint: &str, fixable| IssueView {
            code: Arc::from(code),
            level,
            objects: objects.iter().copied().map(NodeId).collect(),
            message: Arc::from(message),
            hint: Arc::from(hint),
            fixable,
        };
    ValidatorView {
        target: Arc::from("Java block 1.21.11"),
        issues: Arc::from(vec![
            issue(
                "GEO_ROTATION",
                IssueLevel::Error,
                &[3],
                "Glass is rotated 22.5 degrees on Y and X.",
                "The game allows one rotated axis.",
                true,
            ),
            issue(
                "GEO_ROTATION",
                IssueLevel::Error,
                &[6],
                "Link A is rotated on two axes.",
                "The game allows one rotated axis.",
                true,
            ),
            issue(
                "TEX_MISSING",
                IssueLevel::Error,
                &[2, 4],
                "Two faces name a texture the list does not have.",
                "Add the texture or point the faces at another one.",
                false,
            ),
            issue(
                "GEO_OUT_OF_BOUNDS",
                IssueLevel::Warning,
                &[8],
                "Hook reaches 4 units outside the block.",
                "The game clips elements beyond -16 and 32.",
                false,
            ),
            issue(
                "TEX_UNUSED",
                IssueLevel::Info,
                &[],
                "The texture particle is not used by any face.",
                "It is kept in the file.",
                false,
            ),
        ]),
        rev: 3,
        running: false,
        stale: false,
    }
}

/// A keyframe of the animate fixture.
pub fn key_mark(time: f32, kind: KeyKind, value: f32) -> KeyMark {
    KeyMark {
        time,
        kind,
        value: [
            KeyNumber::Num(value),
            KeyNumber::Num(0.0),
            KeyNumber::Num(0.0),
        ],
        jump: false,
    }
}

/// The walk animation and the bones of the lantern with their keyframes.
pub fn animate_views() -> (AnimationsView, TimelineView) {
    let keys = |marks: Vec<KeyMark>| -> Arc<[KeyMark]> { Arc::from(marks) };
    let mut jump = key_mark(1.0, KeyKind::Step, 2.0);
    jump.jump = true;
    let mut expression = key_mark(0.75, KeyKind::Linear, 0.0);
    expression.value[1] = KeyNumber::Expr(Arc::from("math.sin(q.anim_time * 90)"));
    let bone =
        |id: u64, name: &str, depth: usize, selected: bool, channels: [Arc<[KeyMark]>; 3]| {
            BoneRow {
                id: NodeId(id),
                name: Arc::from(name),
                depth,
                selected,
                channels,
            }
        };
    let none = || keys(Vec::new());
    let lantern = bone(
        1,
        "Lantern",
        0,
        false,
        [
            keys(vec![
                key_mark(0.0, KeyKind::Linear, 0.0),
                key_mark(0.5, KeyKind::CatmullRom, 15.0),
                key_mark(1.0, KeyKind::Bezier, -15.0),
                key_mark(1.5, KeyKind::Linear, 0.0),
            ]),
            keys(vec![
                key_mark(0.0, KeyKind::Linear, 0.0),
                key_mark(1.5, KeyKind::Linear, 1.0),
            ]),
            none(),
        ],
    );
    let chain = bone(
        5,
        "Chain",
        0,
        true,
        [
            keys(vec![key_mark(0.25, KeyKind::Step, 5.0), jump, expression]),
            none(),
            keys(vec![
                key_mark(0.0, KeyKind::Linear, 1.0),
                key_mark(1.5, KeyKind::Linear, 1.2),
            ]),
        ],
    );
    let animations = AnimationsView {
        rows: Arc::from(vec![
            AnimationRow {
                id: 11,
                name: Arc::from("walk"),
                length: 1.5,
                loop_kind: LoopKind::Loop,
                fps: 24,
            },
            AnimationRow {
                id: 12,
                name: Arc::from("idle"),
                length: 4.0,
                loop_kind: LoopKind::Once,
                fps: 30,
            },
        ]),
        selected: Some(0),
        playing: false,
        previewing: true,
        time: 0.5,
        speed: 1.0,
        looping: true,
    };
    let timeline = TimelineView {
        rev: 1,
        bones: Arc::from(vec![lantern, chain]),
        has_groups: true,
        pose: Some(PoseView {
            bone: NodeId(5),
            name: Arc::from("Chain"),
            values: [
                Vec3::new(5.0, 0.0, 0.0),
                Vec3::ZERO,
                Vec3::new(1.1, 1.1, 1.1),
            ],
            keyed: [false, false, false],
        }),
    };
    (animations, timeline)
}

fn model_view() -> ModelView {
    ModelView {
        model_box: [Vec3::new(-16.0, -16.0, -32.0), Vec3::new(32.0, 32.0, 16.0)],
        origin: Vec3::ZERO,
        blueprint: Vec3::new(-2.0, 0.0, 2.0),
        voxel_resolution: Some(16),
    }
}

impl Fixture {
    /// The lantern document with two selected elements; the start screen data are there too.
    pub fn new() -> Fixture {
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
            has_doc: true,
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
                    rotation: if i == 2 { 90 } else { 0 },
                    cull: (i == 2).then_some(Face::North),
                })),
                group: None,
            },
            textures: vec![
                TextureView {
                    label: Arc::from("lantern"),
                    size: [16, 16],
                    image: Some(image),
                    source: Some(Arc::from("C:/models/lantern/lantern.png")),
                },
                TextureView {
                    label: Arc::from("particle"),
                    size: [16, 16],
                    image: None,
                    source: None,
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
            recent: recent_files(),
            library: Some(library_view()),
            jobs: Vec::new(),
            view: None,
            status: "",
            animations: None,
            timeline: None,
            display: display_view(),
            model: model_view(),
            validator: Some(validator_view()),
            has_props: true,
            window: WindowView::default(),
        }
    }

    /// A window without the system title bar: the top bar is the title bar.
    pub fn frameless(mut self) -> Fixture {
        self.window.decorated = false;
        self
    }

    /// The lantern with a walk animation of 1.5 seconds: the groups are bones with keyframes of
    /// every kind, the player stands at 0.5 seconds and the chain is the selected bone.
    pub fn animated(mut self) -> Fixture {
        let (animations, timeline) = animate_views();
        self.animations = Some(animations);
        self.timeline = Some(timeline);
        self.selection = vec![NodeId(5)];
        self.props.kind = NodeKind::Group;
        self.props.name = Arc::from("Chain");
        self.props.group = Some(GroupView {
            visible: true,
            locked: false,
            export: true,
            children: 2,
        });
        self
    }

    /// A fresh install: no recent files, an empty library.
    pub fn empty(mut self) -> Fixture {
        self.has_doc = false;
        self.recent = Vec::new();
        self.library = Some(LibraryView {
            busy: false,
            ..LibraryView::default()
        });
        self
    }

    /// A new model with nothing in it: no objects, textures, steps or colours, and a library and
    /// a check that found nothing.
    pub fn blank(mut self) -> Fixture {
        self.nodes.clear();
        self.selection.clear();
        self.has_props = false;
        self.textures.clear();
        self.history = HistoryView::default();
        self.palette.colors = Arc::from(Vec::new());
        self.library = Some(LibraryView {
            busy: false,
            ..LibraryView::default()
        });
        self.validator = Some(ValidatorView {
            target: Arc::from("Java block 1.21.11"),
            issues: Arc::from(Vec::new()),
            rev: 1,
            running: false,
            stale: false,
        });
        self
    }

    /// The start screen with sample data.
    pub fn start(mut self) -> Fixture {
        self.has_doc = false;
        self
    }

    /// The context of one frame.
    pub fn cx(&self) -> UiCx<'_> {
        let mut cx = UiCx::empty(&self.recent);
        cx.window = self.window;
        cx.library = self.library.as_ref();
        cx.jobs = &self.jobs;
        cx.views[3] = self.view;
        cx.validator = self.validator.as_ref();
        if self.has_doc {
            cx.doc = Some(DocView {
                title: "lantern.cubik",
                path: Some(Path::new("C:/models/lantern.cubik")),
                dirty: true,
                nodes: &self.nodes,
                nodes_rev: 1,
                selection: &self.selection,
                primary: self.selection.first().copied(),
                props: self.has_props.then_some(&self.props),
                textures: &self.textures,
                active_texture: Some(0),
                palette: &self.palette,
                history: &self.history,
                display: Some(&self.display),
                model: Some(&self.model),
                animations: self.animations.as_ref(),
                timeline: self.timeline.as_ref(),
                status: self.status,
            });
        }
        cx
    }
}

fn recent_files() -> Vec<RecentEntry> {
    let files = [
        ("lantern.cubik", "C:\\models\\blocks", true, 26),
        ("oak_stool.cubik", "C:\\models\\furniture", true, 50),
        ("iron_sword.bbmodel", "C:\\models\\items", true, 75),
        ("campfire_ring.cubik", "C:\\models\\blocks", true, 100),
        ("fox_plush.cub", "C:\\models\\mobs", false, 400),
        ("market_stall.bbmodel", "C:\\models\\props", false, 900),
        ("moss_carpet.json", "C:\\models\\items", false, 3_000),
    ];
    let now = SystemTime::now();
    files
        .iter()
        .enumerate()
        .map(|(i, (name, dir, thumb, hours))| RecentEntry {
            path: PathBuf::from(dir).join(name),
            name: (*name).to_owned(),
            detail: (*dir).to_owned(),
            thumbnail: thumb.then(|| thumbnail(i as u32)),
            modified: Some(now - Duration::from_secs(hours * 3_600)),
        })
        .collect()
}

fn library_view() -> LibraryView {
    let recent = recent_files();
    LibraryView {
        rev: 1,
        busy: false,
        counts: [42, 118, 9, 23, 7],
        total: 199,
        matches: 199,
        brief: BriefCardView {
            inputs: BriefInputs {
                project_name: "Mossy Hollow".to_owned(),
                mc_version: "1.21.6".to_owned(),
                style_tags: vec!["pixel".to_owned(), "low-poly".to_owned()],
                goal: "A cosy forest village of small, hand-painted props.".to_owned(),
                platform: "items_adder".to_owned(),
                namespace: "mossy".to_owned(),
                texel_density: Some(16.0),
                texture_size: Some(32),
                max_elements: Some(64),
                name_case: "snake".to_owned(),
                name_prefix: "moss_".to_owned(),
                display_preset: Some(DisplayPreset::ItemHandheld),
                rules: vec!["Keep every prop under two blocks.".to_owned()],
                ..BriefInputs::default()
            },
            observed_palette: vec![
                [0xb9, 0x8b, 0x52],
                [0x8a, 0x5a, 0x33],
                [0x5e, 0x3b, 0x22],
                [0xd9, 0xc0, 0x8a],
                [0x7f, 0xc4, 0xc9],
                [0x3f, 0x8f, 0x9b],
                [0x6f, 0xae, 0x4f],
                [0x4a, 0x7a, 0x38],
            ],
            observed_density: Some(16.0),
            models: 42,
            textures: 118,
        },
        suggestions: Arc::from(vec![
            SuggestionView {
                title: "3 models use a different texel density".to_owned(),
                detail:
                    "lantern, oak_stool and market_stall sit between 8 and 32 texels per block."
                        .to_owned(),
                action: Some(SuggestionAction {
                    label: "Open".to_owned(),
                    goal: SuggestionGoal::Open(recent[0].path.clone()),
                }),
            },
            SuggestionView {
                title: "No model for the texture moss_carpet".to_owned(),
                detail: "Create a flat item or a block that uses it.".to_owned(),
                action: Some(SuggestionAction {
                    label: "Create".to_owned(),
                    goal: SuggestionGoal::New(Template::Item),
                }),
            },
        ]),
        pinned: Arc::from(vec![recent[0].path.clone(), recent[2].path.clone()]),
        ..LibraryView::default()
    }
}

/// The window under test and what it last reported.
pub struct App {
    pub ui: Ui,
    pub fx: Fixture,
    pub placement: Option<ViewportPlacement>,
}

/// One frame of the window over the whole screen. The test harness pads its root, so the window
/// gets a child that fills the screen.
pub fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        fx,
        placement,
    } = app;
    let cx = fx.cx();
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

/// Samples per pixel edge of the scene: it is drawn this much larger and filtered down, so the
/// edges look like those of a multisampled render.
const SUPERSAMPLE: usize = 3;

/// The picture of the main view: an isometric grid with a lantern, in the colours of the theme,
/// drawn supersampled.
///
/// Pictures are kept by size and colours: the pictures of a run repeat them.
pub fn scene_image(size: [usize; 2], t: &Tokens) -> ColorImage {
    type Key = ([usize; 2], [[u8; 4]; 3]);
    static DRAWN: Mutex<Vec<(Key, ColorImage)>> = Mutex::new(Vec::new());
    let key: Key = (
        size,
        [t.bg_viewport, t.text, t.accent].map(|c| c.to_array()),
    );
    let mut drawn = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, image)) = drawn.iter().find(|(k, _)| *k == key) {
        return image.clone();
    }
    let k = SUPERSAMPLE;
    let wide = draw_scene([size[0] * k, size[1] * k], k as i32, t);
    let image = box_filter(&wide, k, size);
    drawn.push((key, image.clone()));
    image
}

/// The scene at `size` pixels with lines `k` times as thick as in the final picture.
fn draw_scene(size: [usize; 2], k: i32, t: &Tokens) -> ColorImage {
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
        canvas.line(a, b, k, grid);
        let (a, b) = (
            canvas.project([-8.0, 0.0, i]),
            canvas.project([8.0, 0.0, i]),
        );
        canvas.line(a, b, k, grid);
    }
    let (a, b) = (
        canvas.project([-8.0, 0.0, 0.0]),
        canvas.project([8.0, 0.0, 0.0]),
    );
    canvas.line(a, b, 2 * k, Color32::from_rgb(0xd0, 0x62, 0x58));
    let (a, b) = (
        canvas.project([0.0, 0.0, -8.0]),
        canvas.project([0.0, 0.0, 8.0]),
    );
    canvas.line(a, b, 2 * k, Color32::from_rgb(0x58, 0x88, 0xd0));
    let edge = |c: Color32| c.lerp_to_gamma(Color32::BLACK, 0.55);
    let wood = Color32::from_rgb(0xb9, 0x8b, 0x52);
    let metal = Color32::from_rgb(0x8a, 0x8f, 0x94);
    let glass = Color32::from_rgb(0x7f, 0xc4, 0xc9);
    let stone = Color32::from_rgb(0xa8, 0xa4, 0x98);
    // Back to front.
    canvas.cuboid([2.5, 0.0, -3.5], [2.0, 1.0, 2.0], wood, edge(wood), k);
    canvas.cuboid([3.0, 1.0, -3.0], [1.0, 1.0, 1.0], stone, edge(stone), k);
    canvas.cuboid([-1.5, 0.0, -1.5], [3.0, 0.5, 3.0], metal, edge(metal), k);
    canvas.cuboid([-1.0, 0.5, -1.0], [2.0, 3.0, 2.0], glass, t.accent, 2 * k);
    canvas.cuboid([-1.5, 3.5, -1.5], [3.0, 0.5, 3.0], metal, edge(metal), k);
    canvas.cuboid([-0.25, 4.0, -0.25], [0.5, 1.5, 0.5], wood, edge(wood), k);
    canvas.image
}

/// Averages each `k` by `k` block of an opaque picture into one pixel of a picture of `size`.
fn box_filter(image: &ColorImage, k: usize, size: [usize; 2]) -> ColorImage {
    let [w, h] = size;
    let [wide, _] = image.size;
    let mut pixels = Vec::with_capacity(w * h);
    let samples = (k * k) as u32;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0_u32; 3];
            for dy in 0..k {
                for dx in 0..k {
                    let c = image.pixels[(y * k + dy) * wide + x * k + dx];
                    sum[0] += u32::from(c.r());
                    sum[1] += u32::from(c.g());
                    sum[2] += u32::from(c.b());
                }
            }
            let [r, g, b] = sum.map(|v| ((v + samples / 2) / samples) as u8);
            pixels.push(Color32::from_rgb(r, g, b));
        }
    }
    ColorImage::new(size, pixels)
}
