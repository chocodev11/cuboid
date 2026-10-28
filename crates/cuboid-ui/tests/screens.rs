//! Renders every screen and state to PNG files for the review of the UI against ui_v2_spec 12:
//! the start screen (data, empty, first run) and the editor shell in each workspace with a
//! selection, at 1280 x 800 and 1920 x 1080, in Mocha and Latte, at 100 % and 150 %, and the
//! editor with the Cubik arrangement once. Each mode shows the panels and the properties tabs it
//! uses (`docs/design/mode_layouts.md`). The panels have their own pictures: each tab of the
//! properties (the element tab also in a right area of 280, 240 and 180 points, in Mocha at 100 %
//! and 1920 only), the UV editor zoomed in with the grid, the face letters and the numbers, the
//! outliner while a row is renamed and while rows are dragged, a left area of 200 points whose
//! tab row cuts a title, and the empty
//! states, in Mocha and Latte at 100 % and 1920 and in Mocha at 150 % and 1280. The dialogs have
//! theirs: each one open over the editor (the brief over the start screen), in Mocha and Latte at
//! 100 % and in Mocha at 150 %, all at 1280. `SCREENS_ONLY=dialog` renders just those. The title
//! bar of a window without the system frame has its own: the editor and the start screen in Mocha
//! and Latte, and a maximized inactive window and a 900 wide one in Mocha (`SCREENS_ONLY=titlebar`).
//!
//! ```text
//! cargo test -p cuboid-ui --test screens -j 4 -- --ignored
//! ```
//!
//! The files go to `target/ui-screens/`, named `{screen}_{state}_{theme}_{scale}_{width}.png`. The
//! headless renderer prefers a software adapter (WARP on Windows). The scene in the viewport is
//! drawn on the CPU. The input that opens a tab or starts a drag is synthetic input to the
//! harness; nothing touches the desktop.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{App, Fixture, frame, scene_image};
use cuboid_ui::{
    Arrangement, ClientsView, ColorMode, ComingFrom, ExportFormat, ExportInfo, ExportOutcome,
    ExportReport, FontSet, IssueLevel, IssueView, JobView, Layout, LibEntryView, LibKind, LibScope,
    Mode, NodeId, PanelId, Prefs, PresetKind, ProjectView, RecoveryEntry, RecoveryInfo, Shading,
    StyleSourceView, ThumbState, Ui, UvOutline, VersionChoice, ViewLayout, ViewportFrame, Zone,
};
use cuboid_ui_kit::Tokens;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Role, TextureOptions, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use glam::Vec3;

static NO_VERSIONS: [String; 0] = [];

/// A panel picture: a state of one panel of the editor.
#[derive(Copy, Clone)]
enum Shot {
    Element,
    /// The element in a right area of 280, 240 and 180 points.
    ElementNarrow,
    ElementNarrower,
    ElementNarrowest,
    Faces,
    Texture,
    Model,
    Display,
    DisplayCubik,
    Validator,
    OutlinerRename,
    OutlinerDrag,
    EmptyModel,
    EmptyLists,
    EmptyPalette,
    HeaderOverflow,
    Frames,
    /// The UV editor zoomed in, with two faces chosen and the faces of other elements outlined.
    UvEditor,
    /// Paint in the Cubik arrangement with a left area of 200 points: its tab row gives up the
    /// titles of the other tabs and cuts the shown one.
    TabsNarrow,
    /// The library panel of a project with style references, and the menu of an own entry.
    LibraryStyle,
}

impl Shot {
    const ALL: [Shot; 20] = [
        Shot::Element,
        Shot::ElementNarrow,
        Shot::ElementNarrower,
        Shot::ElementNarrowest,
        Shot::Faces,
        Shot::Texture,
        Shot::Frames,
        Shot::UvEditor,
        Shot::Model,
        Shot::Display,
        Shot::DisplayCubik,
        Shot::Validator,
        Shot::OutlinerRename,
        Shot::OutlinerDrag,
        Shot::EmptyModel,
        Shot::EmptyLists,
        Shot::EmptyPalette,
        Shot::HeaderOverflow,
        Shot::TabsNarrow,
        Shot::LibraryStyle,
    ];

    fn name(self) -> &'static str {
        match self {
            Shot::Element => "props_element",
            Shot::ElementNarrow => "props_element-narrow",
            Shot::ElementNarrower => "props_element-narrower",
            Shot::ElementNarrowest => "props_element-narrowest",
            Shot::Faces => "props_faces",
            Shot::Texture => "props_texture",
            Shot::Frames => "props_frames",
            Shot::UvEditor => "uv_editor",
            Shot::Model => "props_model",
            Shot::Display => "props_display",
            Shot::DisplayCubik => "props_display-cubik",
            Shot::Validator => "props_validator",
            Shot::OutlinerRename => "outliner_rename",
            Shot::OutlinerDrag => "outliner_drag",
            Shot::EmptyModel => "empty_model",
            Shot::EmptyLists => "empty_lists",
            Shot::EmptyPalette => "empty_palette",
            Shot::HeaderOverflow => "header_overflow",
            Shot::TabsNarrow => "tabs_paint-narrow",
            Shot::LibraryStyle => "library_style",
        }
    }

    /// The tip of the properties tab the picture opens, if it opens one.
    fn tab(self) -> Option<&'static str> {
        match self {
            Shot::Element
            | Shot::ElementNarrow
            | Shot::ElementNarrower
            | Shot::ElementNarrowest => Some("Element"),
            Shot::Faces => Some("Faces and UV"),
            Shot::Texture | Shot::Frames | Shot::EmptyPalette => Some("Texture"),
            Shot::Model => Some("Model properties"),
            Shot::Display | Shot::DisplayCubik => Some("Display properties"),
            _ => None,
        }
    }

    /// The width of the right area when the picture is about a narrow one; 180 is the narrowest an
    /// area may be.
    fn right_width(self) -> Option<f32> {
        match self {
            Shot::ElementNarrow => Some(280.0),
            Shot::ElementNarrower => Some(240.0),
            Shot::ElementNarrowest => Some(180.0),
            _ => None,
        }
    }

    fn blank(self) -> bool {
        matches!(
            self,
            Shot::EmptyModel | Shot::EmptyLists | Shot::EmptyPalette
        )
    }

    /// The preset of the document: the Minecraft fields show only under the Minecraft preset.
    fn project(self) -> ProjectView {
        match self {
            Shot::DisplayCubik => ProjectView::default(),
            _ => ProjectView {
                preset: PresetKind::Minecraft,
                version: "1.21.11".to_owned(),
                versions: &NO_VERSIONS,
                shading: Shading::MinecraftBlock,
                validation: "Java block 1.21.11".to_owned(),
                export_note: "Saves .cubik; export writes Minecraft JSON for 1.21.11.",
                ..ProjectView::default()
            },
        }
    }
}

/// A dialog open over the editor; the brief opens over the start screen.
#[derive(Copy, Clone)]
enum Dialog {
    Unsaved,
    Recovery,
    Project,
    PrefsInterface,
    PrefsNavigation,
    PrefsKeymap,
    PrefsEditing,
    PrefsGeneral,
    ExportSetup,
    ExportMinecraft,
    ExportFormats,
    ExportChecking,
    ExportReview,
    ExportDone,
    ExportFailed,
    Render,
    RenderColour,
    RenderToast,
    Brief,
    /// The brief scrolled to its style references.
    BriefStyle,
    /// The question before a style folder is removed, over the brief.
    BriefStyleRemove,
    /// The brief while Add folder runs its import: the job and its Cancel, the folders waiting.
    BriefStyleJob,
    /// The brief scrolled to its AI clients: the folder and the lines to paste.
    BriefClients,
    /// The same, scrolled to the end of the section.
    BriefClientsEnd,
}

impl Dialog {
    const ALL: [Dialog; 24] = [
        Dialog::Unsaved,
        Dialog::Recovery,
        Dialog::Project,
        Dialog::PrefsInterface,
        Dialog::PrefsNavigation,
        Dialog::PrefsKeymap,
        Dialog::PrefsEditing,
        Dialog::PrefsGeneral,
        Dialog::ExportSetup,
        Dialog::ExportMinecraft,
        Dialog::ExportFormats,
        Dialog::ExportChecking,
        Dialog::ExportReview,
        Dialog::ExportDone,
        Dialog::ExportFailed,
        Dialog::Render,
        Dialog::RenderColour,
        Dialog::RenderToast,
        Dialog::Brief,
        Dialog::BriefStyle,
        Dialog::BriefStyleRemove,
        Dialog::BriefStyleJob,
        Dialog::BriefClients,
        Dialog::BriefClientsEnd,
    ];

    fn name(self) -> &'static str {
        match self {
            Dialog::Unsaved => "unsaved",
            Dialog::Recovery => "recovery",
            Dialog::Project => "project",
            Dialog::PrefsInterface => "prefs-interface",
            Dialog::PrefsNavigation => "prefs-navigation",
            Dialog::PrefsKeymap => "prefs-keymap",
            Dialog::PrefsEditing => "prefs-editing",
            Dialog::PrefsGeneral => "prefs-general",
            Dialog::ExportSetup => "export-setup",
            Dialog::ExportMinecraft => "export-minecraft",
            Dialog::ExportFormats => "export-formats",
            Dialog::ExportChecking => "export-checking",
            Dialog::ExportReview => "export-review",
            Dialog::ExportDone => "export-done",
            Dialog::ExportFailed => "export-failed",
            Dialog::Render => "render",
            Dialog::RenderColour => "render-colour",
            Dialog::RenderToast => "render-toast",
            Dialog::Brief => "brief",
            Dialog::BriefStyle => "brief-style",
            Dialog::BriefStyleRemove => "brief-style-remove",
            Dialog::BriefStyleJob => "brief-style-job",
            Dialog::BriefClients => "brief-clients",
            Dialog::BriefClientsEnd => "brief-clients-end",
        }
    }

    /// The tab of the preferences the picture opens, if it opens one.
    fn prefs_tab(self) -> Option<&'static str> {
        match self {
            Dialog::PrefsInterface => Some("Interface"),
            Dialog::PrefsNavigation => Some("Navigation"),
            Dialog::PrefsKeymap => Some("Keymap"),
            Dialog::PrefsEditing => Some("Editing"),
            Dialog::PrefsGeneral => Some("General"),
            _ => None,
        }
    }
}

/// The top bar as the title bar of a window without the system frame.
#[derive(Copy, Clone)]
enum Bar {
    Editor,
    Start,
    /// Maximized and inactive: the Restore button and the weak text.
    Inactive,
    /// The narrowest window, 900 points.
    Narrow,
}

impl Bar {
    fn name(self) -> &'static str {
        match self {
            Bar::Editor => "editor",
            Bar::Start => "start",
            Bar::Inactive => "inactive",
            Bar::Narrow => "narrow",
        }
    }
}

/// What a picture shows.
#[derive(Copy, Clone)]
enum Screen {
    StartData,
    StartEmpty,
    StartFirstRun,
    Editor(Mode),
    Panel(Shot),
    Dialog(Dialog),
    TitleBar(Bar),
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
            Screen::Editor(Mode::Animate) => ("editor", "animate"),
            Screen::Panel(shot) => {
                let name = shot.name();
                name.split_once('_').unwrap_or(("panel", name))
            }
            Screen::Dialog(dialog) => ("dialog", dialog.name()),
            Screen::TitleBar(bar) => ("titlebar", bar.name()),
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
    if let Screen::Panel(shot) = screen {
        if let Some(width) = shot.right_width() {
            // The properties get most of the height, so that the whole tab is in the picture.
            let right = prefs.layout.zone_mut(Zone::Right);
            right.size = width;
            right.split = 0.85;
        }
        match shot {
            // The UV editor gets a left area of 400 and most of its height.
            Shot::UvEditor => {
                let left = prefs.layout.zone_mut(Zone::Left);
                left.size = 400.0;
                left.split = 0.85;
            }
            // The lists that have no tab in front by default.
            Shot::EmptyLists => {
                prefs.layout.bring_forward(PanelId::Library, Mode::Model);
                prefs.layout.bring_forward(PanelId::History, Mode::Model);
            }
            Shot::LibraryStyle => {
                prefs.layout.bring_forward(PanelId::Library, Mode::Model);
            }
            // Four views are too narrow for the tool options in a header.
            Shot::HeaderOverflow => prefs.view_layout = ViewLayout::Quad,
            // Four panels in the lower group of the left area, which Paint uses.
            Shot::TabsNarrow => {
                prefs.mode = Mode::Paint;
                prefs.layout = Layout::arranged(Arrangement::Cubik);
                prefs.layout.zone_mut(Zone::Left).size = 200.0;
            }
            _ => {}
        }
    }
    prefs.sanitize();
    let mut ui = Ui::new(prefs, FontSet::default());
    let mut fx = match screen {
        Screen::StartData => {
            ui.show_start();
            Fixture::new().start()
        }
        Screen::StartEmpty | Screen::StartFirstRun => {
            ui.show_start();
            Fixture::new().empty()
        }
        Screen::Editor(Mode::Animate) => {
            ui.set_mode(Mode::Animate);
            Fixture::new().animated()
        }
        Screen::TitleBar(Bar::Start) => {
            ui.show_start();
            Fixture::new().start().frameless()
        }
        Screen::TitleBar(bar) => {
            let mut fx = Fixture::new().frameless();
            if matches!(bar, Bar::Inactive) {
                fx.window.maximized = true;
                fx.window.focused = false;
            }
            fx
        }
        Screen::Editor(mode) => {
            ui.set_mode(mode);
            Fixture::new()
        }
        Screen::Dialog(dialog) => {
            let versions: Vec<String> = ["1.21.11", "1.21.6", "1.20.4"].map(str::to_owned).into();
            ui.set_project(ProjectView {
                versions: Box::leak(versions.into_boxed_slice()),
                ..Shot::Model.project()
            });
            ui.set_export_info(export_info());
            if matches!(
                dialog,
                Dialog::Brief
                    | Dialog::BriefStyle
                    | Dialog::BriefStyleRemove
                    | Dialog::BriefStyleJob
                    | Dialog::BriefClients
                    | Dialog::BriefClientsEnd
            ) {
                ui.show_start();
                with_style_references(Fixture::new().start())
            } else {
                Fixture::new()
            }
        }
        Screen::Panel(shot) => {
            ui.set_project(shot.project());
            // The tabs and panels each picture needs are the ones of its mode.
            match shot {
                Shot::HeaderOverflow => ui.set_mode(Mode::Voxel),
                Shot::Display | Shot::DisplayCubik => ui.set_mode(Mode::Display),
                Shot::EmptyPalette => ui.set_mode(Mode::Paint),
                _ => {}
            }
            if matches!(shot, Shot::Validator | Shot::EmptyLists) {
                ui.show_validator();
            }
            if shot.blank() {
                Fixture::new().blank()
            } else if matches!(shot, Shot::Frames) {
                Fixture::new().frames()
            } else if matches!(shot, Shot::LibraryStyle) {
                with_style_references(Fixture::new())
            } else {
                Fixture::new()
            }
        }
    };
    if let Screen::Panel(shot) = screen
        && shot.right_width().is_some()
    {
        // Values of up to five characters, which the narrow fields must show whole.
        fx.props.position = Vec3::new(-12.5, 16.25, 3.25);
        fx.props.size = Vec3::new(14.75, 8.5, 1.25);
        fx.props.rotation = Vec3::new(-45.0, 22.5, 90.0);
        fx.props.pivot = Vec3::new(8.0, 0.0, -2.5);
    }
    if matches!(screen, Screen::Panel(Shot::UvEditor)) {
        // Six faces of 4 by 4 texels, three by two on the texture, and the faces of other
        // elements in the free part of it. The zoomed view shows a little less than the texture.
        let grid = [
            [1.0, 1.0, 5.0, 5.0],
            [5.0, 1.0, 9.0, 5.0],
            [9.0, 1.0, 13.0, 5.0],
            [1.0, 5.0, 5.0, 9.0],
            [5.0, 5.0, 9.0, 9.0],
            [9.0, 5.0, 13.0, 9.0],
        ];
        for (face, uv) in fx.props.faces.iter_mut().flatten().zip(grid) {
            face.uv = uv;
            face.enabled = true;
        }
        fx.uv_outlines = [
            [13.5, 1.5, 15.0, 6.5],
            [1.5, 10.0, 6.5, 14.5],
            [8.0, 10.5, 14.0, 13.5],
        ]
        .map(|uv| UvOutline {
            texture: Arc::from("lantern"),
            uv,
        })
        .into();
    }
    App {
        ui,
        fx,
        placement: None,
    }
}

/// The library of the fixture as a project's: two style folders, entries of which some are style
/// references, and the lines that connect an AI client to its folder.
fn with_style_references(mut fx: Fixture) -> Fixture {
    let source = |path: &str, models, textures, licence: &str| StyleSourceView {
        path: path.to_owned(),
        models,
        textures,
        licence: licence.to_owned(),
    };
    let entry = |id: &str, kind, name: &str, detail: &str, style| LibEntryView {
        id: id.to_owned(),
        kind,
        name: name.to_owned(),
        detail: detail.to_owned(),
        tags: Vec::new(),
        style,
        thumb: ThumbState::Missing,
    };
    let view = fx.library.as_mut().expect("the fixture has a library");
    view.scope = LibScope::Project;
    view.project_available = true;
    view.brief.project = true;
    view.brief.style_sources = vec![
        source("refs/lamps", 12, 4, "CC0"),
        source("refs/vanilla_trims", 2, 0, "unknown"),
    ];
    view.brief.style_models = 14;
    view.brief.style_textures = 4;
    view.clients = Some(ClientsView {
        folder: r"C:\Users\builder\models\lamp".to_owned(),
        refused: None,
        desktop_config: r"%APPDATA%\Claude\claude_desktop_config.json".to_owned(),
        claude_code: r#"claude mcp add --transport stdio cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "C:\Users\builder\models\lamp""#.to_owned(),
        claude_desktop: r#""cuboid-lamp": {"command": "C:\\Program Files\\cuboid\\cuboid.exe", "args": ["mcp", "--root", "C:\\Users\\builder\\models\\lamp"]}"#.to_owned(),
        codex_command: r#"codex mcp add cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "C:\Users\builder\models\lamp""#.to_owned(),
        codex_toml: "[mcp_servers.cuboid]\ncommand = 'C:\\Program Files\\cuboid\\cuboid.exe'\nargs = ['mcp', '--root', 'C:\\Users\\builder\\models\\lamp']\ntool_timeout_sec = 120  # the default is 60\n".to_owned(),
        check: r#"& "C:\Program Files\cuboid\cuboid.exe" mcp --root "C:\Users\builder\models\lamp" --check"#.to_owned(),
    });
    view.entries = Arc::from(vec![
        entry(
            "m1",
            LibKind::Model,
            "iron_sword.bbmodel",
            "24 elements, 3 KB",
            false,
        ),
        entry(
            "m2",
            LibKind::Model,
            "street_lamp.json",
            "12 elements, 1.2 KB",
            true,
        ),
        entry(
            "t1",
            LibKind::Texture,
            "lamp_wood.png",
            "16 x 16, 410 B",
            true,
        ),
        entry(
            "t2",
            LibKind::Texture,
            "moss_carpet.png",
            "32 x 32, 2.1 KB",
            false,
        ),
    ]);
    view.matches = 4;
    fx
}

/// The game versions and the folder the export dialog starts with.
fn export_info() -> ExportInfo {
    let version = |id: &str, pack: Option<&str>| VersionChoice {
        id: id.to_owned(),
        pack_format: pack.map(str::to_owned),
    };
    ExportInfo {
        versions: vec![
            version("1.21.11", Some("75")),
            version("1.21.6", Some("63")),
            version("1.20.4", Some("22")),
            version("1.19", None),
        ],
        default_version: "1.21.11".to_owned(),
        folder: PathBuf::from("C:/Users/builder/models"),
    }
}

/// The findings of a check that stops the export.
fn review_report() -> ExportReport {
    let issue = |code: &str, message: &str, fixable: bool| IssueView {
        code: Arc::from(code),
        level: IssueLevel::Error,
        objects: vec![NodeId(2)],
        message: Arc::from(message),
        hint: Arc::from("Fix it."),
        fixable,
    };
    ExportReport {
        target: "Java block 1.21.11".to_owned(),
        errors: 3,
        warnings: 2,
        fixable: 2,
        issues: vec![
            issue("GEO_ROTATION", "Cap rotates on more than one axis.", true),
            issue(
                "GEO_OUTSIDE",
                "Frame reaches outside the 48 unit box.",
                false,
            ),
            issue("TEX_SIZE", "Texture brick is not a power of two.", true),
        ],
    }
}

/// Presses a button of the open dialog.
fn press(harness: &mut Harness<'static, App>, label: &str) {
    harness.run_steps(8);
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run_steps(4);
}

/// The synthetic input that opens a dialog and brings it into the state of its picture.
fn open_dialog(harness: &mut Harness<'static, App>, dialog: Dialog) {
    match dialog {
        Dialog::Unsaved => {
            assert!(!harness.state_mut().ui.close_requested());
        }
        Dialog::Recovery => {
            let entry = |name: &str, saved: &str, note: &str| RecoveryEntry {
                path: PathBuf::from(format!("recovery/{name}.cubik")),
                name: name.to_owned(),
                saved: saved.to_owned(),
                note: note.to_owned(),
            };
            harness.state_mut().ui.open_recovery(RecoveryInfo {
                message: "Cuboid did not close properly last time.".to_owned(),
                entries: vec![
                    entry("tower", "Today 10:42", "Saved before a crash"),
                    entry("lantern", "Today 09:15", ""),
                    entry(
                        "a_rather_long_model_name_that_must_not_push_the_buttons_away",
                        "Yesterday",
                        "",
                    ),
                ],
            });
        }
        Dialog::Project => {
            harness.get_by_label("File").click();
            harness.run_steps(3);
            harness.get_by_label("Project settings...").click();
        }
        Dialog::PrefsInterface
        | Dialog::PrefsNavigation
        | Dialog::PrefsKeymap
        | Dialog::PrefsEditing
        | Dialog::PrefsGeneral => {
            harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
            harness.run_steps(4);
            if let Some(tab) = dialog.prefs_tab() {
                harness.get_by_role_and_label(Role::Tab, tab).click();
            }
        }
        Dialog::ExportSetup => harness.state_mut().ui.open_export("tower", None, None),
        Dialog::ExportMinecraft => {
            let ui = &mut harness.state_mut().ui;
            ui.open_export("tower", None, Some(ExportFormat::MinecraftJson));
        }
        Dialog::ExportFormats => {
            harness.state_mut().ui.open_export("tower", None, None);
            harness.run_steps(8);
            harness
                .get_by_role_and_label(Role::ComboBox, "Cubik copy (.cubik)")
                .click();
        }
        Dialog::ExportChecking | Dialog::ExportReview => {
            harness.state_mut().ui.open_export("tower", None, None);
            press(harness, "Export");
            if matches!(dialog, Dialog::ExportReview) {
                harness.state_mut().ui.export_checked(review_report());
            }
        }
        Dialog::ExportDone | Dialog::ExportFailed => {
            let ui = &mut harness.state_mut().ui;
            ui.open_export("tower", None, Some(ExportFormat::Obj));
            press(harness, "Export");
            let outcome = if matches!(dialog, Dialog::ExportDone) {
                ExportOutcome::Done {
                    path: PathBuf::from("C:/Users/builder/models/tower.obj"),
                    notices: vec![
                        "Skipped 2 item overrides.".to_owned(),
                        "One texture was given a path.".to_owned(),
                    ],
                }
            } else {
                ExportOutcome::Failed("Cannot export tower: the disk is full.".to_owned())
            };
            harness.state_mut().ui.export_finished(outcome);
        }
        Dialog::Render | Dialog::RenderColour | Dialog::RenderToast => {
            let ui = &mut harness.state_mut().ui;
            ui.open_export("tower", None, Some(ExportFormat::Png));
            press(harness, "Continue");
            if matches!(dialog, Dialog::RenderColour) {
                let colour = harness.get_by_role_and_label(Role::RadioButton, "Colour");
                colour.click();
                harness.run_steps(4);
                let custom = harness.get_by_role_and_label(Role::RadioButton, "Custom");
                custom.click();
            }
            if matches!(dialog, Dialog::RenderToast) {
                let path = PathBuf::from("C:/Users/builder/models/tower.png");
                harness.state_mut().ui.render_finished(Ok(path));
            }
        }
        Dialog::Brief
        | Dialog::BriefStyle
        | Dialog::BriefStyleRemove
        | Dialog::BriefStyleJob
        | Dialog::BriefClients
        | Dialog::BriefClientsEnd => {
            // Below 1280 points the card sits under the recent files.
            let edit = harness.get_by_role_and_label(Role::Button, "Edit");
            edit.scroll_to_me();
            harness.run_steps(20);
            harness.get_by_role_and_label(Role::Button, "Edit").click();
            if matches!(dialog, Dialog::BriefClients | Dialog::BriefClientsEnd) {
                harness.run_steps(4);
                if matches!(dialog, Dialog::BriefClients) {
                    harness
                        .get_by_role_and_label(Role::Button, "AI clients")
                        .scroll_to_me();
                } else {
                    // The line of the last card.
                    harness.get_by_label_contains("--check").scroll_to_me();
                }
                harness.run_steps(20);
            } else if !matches!(dialog, Dialog::Brief) {
                harness.run_steps(4);
                // The last line of the section.
                harness
                    .get_by_label_contains("Pick a folder")
                    .scroll_to_me();
                harness.run_steps(20);
            }
            if matches!(dialog, Dialog::BriefStyleRemove) {
                // Remove of the first folder; the question stands over the dialog.
                harness
                    .get_all_by_role_and_label(Role::Button, "Remove")
                    .next()
                    .expect("the first folder")
                    .click();
                harness.run_steps(4);
            }
            if matches!(dialog, Dialog::BriefStyleJob) {
                // The dialog opens on an idle library; Add folder then starts the import.
                let view = harness
                    .state_mut()
                    .fx
                    .library
                    .as_mut()
                    .expect("the fixture has a library");
                view.busy = true;
                view.job = Some(JobView {
                    id: 2,
                    label: "Importing style references".to_owned(),
                    fraction: Some(0.4),
                    cancellable: true,
                });
                harness.run_steps(4);
            }
        }
    }
    harness.run_steps(8);
}

/// The synthetic input that brings a panel into the state of its picture.
fn interact(harness: &mut Harness<'static, App>, shot: Shot) {
    if let Some(tip) = shot.tab() {
        harness.get_by_role_and_label(Role::Tab, tip).click();
        harness.run_steps(4);
    }
    match shot {
        Shot::OutlinerRename => {
            harness.key_press(Key::F2);
            harness.run_steps(4);
        }
        Shot::OutlinerDrag => drag_row(harness, "Cap", "Frame"),
        Shot::HeaderOverflow => {
            harness.get_by_label("Tool options").click();
            harness.run_steps(4);
        }
        Shot::UvEditor => zoom_uv(harness),
        Shot::LibraryStyle => {
            // The menu of an entry that is the project's own work; the chips stay in sight.
            harness
                .get_by_role_and_label(Role::ListItem, "moss_carpet.png")
                .click_secondary();
            harness.run_steps(4);
        }
        _ => {}
    }
}

/// Zooms the UV editor in about the middle of the texture, turns on the outlines of the other
/// faces and chooses two faces.
fn zoom_uv(harness: &mut Harness<'static, App>) {
    let wheel_at = |harness: &mut Harness<'static, App>, at: Pos2| {
        harness.event(Event::PointerMoved(at));
        harness.run_steps(2);
        harness.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        harness.run_steps(4);
    };
    harness.get_by_label("Show all UVs on this texture").click();
    harness.run_steps(4);
    let view = harness.get_by_label("UV view").rect();
    wheel_at(harness, view.center());
    // The texture is 16 texels across and centred on the view; the readout gives the scale.
    let scale = harness
        .query_all_by(|node| node.role() == Role::Button)
        .find_map(|node| {
            let label = node.accesskit_node().label()?;
            label.strip_suffix('%')?.parse::<f32>().ok()
        })
        .expect("the zoom readout")
        / 100.0;
    let texel = |x: f32, y: f32| view.center() + (vec2(x, y) - vec2(8.0, 8.0)) * scale;
    for (at, mods) in [
        (texel(11.0, 3.0), Modifiers::NONE),
        (texel(3.0, 7.0), Modifiers::SHIFT),
    ] {
        harness.event(Event::ModifiersChanged(mods));
        harness.event(Event::PointerMoved(at));
        harness.run_steps(2);
        for pressed in [true, false] {
            harness.event(Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: mods,
            });
            harness.run_steps(3);
        }
    }
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.event(Event::PointerMoved(texel(7.0, 6.0)));
    harness.run_steps(4);
}

/// Presses on a row and moves onto another one without letting go, so the drop indicator shows.
fn drag_row(harness: &mut Harness<'static, App>, from: &str, onto: &str) {
    let start = harness
        .get_by_role_and_label(Role::TreeItem, from)
        .rect()
        .center();
    let target = harness.get_by_role_and_label(Role::TreeItem, onto).rect();
    // The upper quarter of a row drops before it.
    let end = target.center_top() + vec2(0.0, 3.0);
    harness.event(Event::PointerMoved(start));
    harness.run_steps(2);
    harness.event(Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    for step in 1..=4 {
        let at = start + (end - start) * (step as f32 / 4.0);
        harness.event(Event::PointerMoved(at));
        harness.run_steps(2);
    }
    harness.run_steps(2);
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
    // The scene texture lives until the picture is taken.
    let mut scene = None;
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
        scene = Some(handle);
        harness.run_steps(4);
    }
    match screen {
        Screen::Panel(shot) => interact(&mut harness, shot),
        Screen::Dialog(dialog) => open_dialog(&mut harness, dialog),
        _ => {}
    }
    let picture = harness.render().expect("headless render");
    drop(scene);
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
        Screen::Editor(Mode::Animate),
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
    // The panels: Mocha and Latte at 100 % and 1920, and Mocha at 150 % and 1280.
    let panel_variants = [
        Variant {
            light: false,
            scale: 1.0,
            size: [1920, 1080],
        },
        Variant {
            light: true,
            scale: 1.0,
            size: [1920, 1080],
        },
        Variant {
            light: false,
            scale: 1.5,
            size: [1280, 800],
        },
    ];
    // A header needs its overflow menu only in a window of 1024 points or so.
    let narrow = [Variant {
        light: false,
        scale: 1.0,
        size: [1024, 768],
    }];
    for shot in Shot::ALL {
        let variants: &[Variant] = if matches!(shot, Shot::HeaderOverflow) {
            &narrow
        } else if matches!(shot, Shot::LibraryStyle) {
            // Mocha and Latte at 1920: the list needs the height to show its four rows.
            &panel_variants[..2]
        } else if shot.right_width().is_some() || matches!(shot, Shot::TabsNarrow) {
            // One picture each: the reflow does not depend on the theme or the scale.
            &panel_variants[..1]
        } else {
            &panel_variants
        };
        for variant in variants.iter().copied() {
            let screen = Screen::Panel(shot);
            let name = file_name(screen, variant);
            if !name.contains(&only) {
                continue;
            }
            render(screen, variant, ComingFrom::Blockbench, &out_dir, &name);
            count += 1;
        }
    }
    // The dialogs: Mocha and Latte at 100 %, Mocha at 150 %, all at 1280.
    let dialog_variants = [
        Variant {
            light: false,
            scale: 1.0,
            size: [1280, 800],
        },
        Variant {
            light: true,
            scale: 1.0,
            size: [1280, 800],
        },
        Variant {
            light: false,
            scale: 1.5,
            size: [1280, 800],
        },
    ];
    for dialog in Dialog::ALL {
        for variant in dialog_variants {
            let screen = Screen::Dialog(dialog);
            let name = file_name(screen, variant);
            if !name.contains(&only) {
                continue;
            }
            render(screen, variant, ComingFrom::Blockbench, &out_dir, &name);
            count += 1;
        }
    }
    // The title bar: the editor and the start screen in Mocha and Latte, an inactive maximized
    // window and the narrowest window in Mocha.
    for (bar, size, themes) in [
        (Bar::Editor, [1280, 800], [false, true].as_slice()),
        (Bar::Start, [1280, 800], [false, true].as_slice()),
        (Bar::Inactive, [1280, 800], [false].as_slice()),
        (Bar::Narrow, [900, 700], [false].as_slice()),
    ] {
        for &light in themes {
            let variant = Variant {
                light,
                scale: 1.0,
                size,
            };
            let screen = Screen::TitleBar(bar);
            let name = file_name(screen, variant);
            if !name.contains(&only) {
                continue;
            }
            render(screen, variant, ComingFrom::Blockbench, &out_dir, &name);
            count += 1;
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
