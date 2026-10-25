//! The Validator panel, the export dialog and the PNG render dialog through the real frame path:
//! synthetic egui input, the UI frame, the requests of the UI applied by the app and the workers
//! they start. The PNG test runs on the headless (software) device.

use std::path::Path;
use std::time::{Duration, Instant};

use cuboid_assets::mc_versions::McVersion;
use cuboid_core::{Command, ElementKind, Target};
use cuboid_model::image::{ImageFileKind, decode_file};
use cuboid_settings::{Environment, Settings};
use cuboid_shell::Headless;
use cuboid_ui::{
    Edit, ExportFormat, ExportRequest, ExportSettings, ExportStage, FontSet, IssueLevel, Prefs,
    RenderBackground, RenderCamera, RenderRequest, RenderSettings, RenderShading, SelectOp,
    UiRequest,
};
use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, vec2};

use super::*;
use crate::app::App;
use crate::io_thread::IoThread;
use crate::platform::Dirs;
use crate::preset::{Preset, ProjectSettings};

/// A game version with the classic rotation rule.
const CLASSIC: McVersion = McVersion::new(1, 21, 4);

struct Rig {
    dir: tempfile::TempDir,
    state: State,
    front: Frontend,
    ctx: egui::Context,
    clock: Instant,
    time: f64,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::first_run(&Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let clock = Instant::now();
        let mut state = State::new(
            Dirs::under(dir.path()),
            settings,
            IoThread::spawn().unwrap(),
            1252,
            clock,
        );
        state.editor.project = ProjectSettings::new(Preset::Minecraft { version: CLASSIC });
        let prefs = Prefs {
            nav_chosen: true,
            accent_chosen: true,
            ..Prefs::default()
        };
        let front = Frontend::new(prefs, FontSet::default(), dir.path().join("ui.json"));
        let mut rig = Rig {
            dir,
            state,
            front,
            ctx: egui::Context::default(),
            clock,
            time: 0.0,
        };
        rig.frames(2, Vec::new());
        rig
    }

    /// `count` frames, 100 ms apart on the app clock; the events go to the first.
    fn frames(&mut self, count: usize, events: Vec<Event>) {
        let mut events = Some(events);
        for _ in 0..count {
            self.clock += Duration::from_millis(100);
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
                time: Some(self.time),
                events: events.take().unwrap_or_default(),
                ..RawInput::default()
            };
            self.time += 0.1;
            self.ctx.begin_pass(raw);
            self.front.update(&mut self.state, &self.ctx, self.clock);
            self.ctx.end_pass().drop_without_applying_deltas();
        }
    }

    /// Frames until `done` holds, within a time limit.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Rig) -> bool) {
        let end = Instant::now() + Duration::from_secs(30);
        while !done(self) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            self.frames(1, Vec::new());
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    fn enter(&mut self) {
        let press = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        self.frames(1, vec![press(true), press(false)]);
    }

    /// A cube far outside the Java range and one with a rotation the classic rule refuses.
    fn add_problems(&mut self) {
        let core = &mut self.state.editor.core;
        core.env_mut().restrict_rotations = false;
        std::sync::Arc::make_mut(&mut core.doc_for_extract().props)
            .general
            .restrict_rotations = false;
        for _ in 0..2 {
            assert!(
                core.execute(Command::AddElement(ElementKind::Cube))
                    .is_done()
            );
        }
        let out = core.execute(Command::SetPosition {
            targets: vec![Target::Element(0)],
            value: [Some(40.0), None, None],
        });
        assert!(out.is_done(), "{out:?}");
        let out = core.execute(Command::SetRotation {
            targets: vec![Target::Element(1)],
            value: [None, Some(30.0), None],
        });
        assert!(out.is_done(), "{out:?}");
    }

    fn undo_len(&self) -> usize {
        self.state.editor.core.history().undo_len()
    }

    fn apply(&mut self, request: UiRequest) {
        let Rig { state, front, .. } = self;
        front.apply(state, request);
    }

    fn settings(&self, format: ExportFormat) -> ExportSettings {
        ExportSettings {
            format,
            version: CLASSIC.to_string(),
        }
    }
}

#[test]
fn the_validator_panel_counts_selects_and_fixes_in_one_undo_step() {
    let mut rig = Rig::new();
    rig.add_problems();
    // A hidden panel runs no check.
    rig.frames(8, Vec::new());
    assert!(rig.front.validation.view().issues.is_empty());

    rig.front.ui.show_validator();
    rig.until("the first check", |rig| {
        !rig.front.validation.view().issues.is_empty() && !rig.front.validation.view().stale
    });
    let view = rig.front.validation.view().clone();
    assert_eq!(view.count(IssueLevel::Error), 2);
    assert_eq!(view.count(IssueLevel::Warning), 2);
    assert_eq!(view.badge(), Some((2, cuboid_ui_kit::Tone::Danger)));

    // A click on an issue selects its objects.
    let bounds = view
        .issues
        .iter()
        .find(|i| &*i.code == "GEO_BOUNDS")
        .unwrap()
        .clone();
    rig.apply(UiRequest::Edit(Edit::Select {
        ids: bounds.objects.clone(),
        op: SelectOp::Replace,
    }));
    assert_eq!(rig.state.editor.core.selection().list.len(), 1);

    // "Fix all" of the errors is one undo step.
    let before = rig.undo_len();
    let index = view
        .issues
        .iter()
        .position(|i| &*i.code == "GEO_ROTATION")
        .unwrap();
    rig.apply(UiRequest::Validator(cuboid_ui::ValidatorRequest::Fix {
        rev: view.rev,
        index,
    }));
    assert_eq!(rig.undo_len(), before + 1);
    rig.apply(UiRequest::Validator(cuboid_ui::ValidatorRequest::FixAll {
        rev: view.rev,
        code: "GEO_BOUNDS".into(),
    }));
    assert_eq!(rig.undo_len(), before + 2, "each request is its own step");
    rig.until("the second check", |rig| {
        let v = rig.front.validation.view();
        !v.stale && !v.running && v.rev != view.rev
    });
    assert_eq!(rig.front.validation.view().count(IssueLevel::Error), 0);
    assert!(
        rig.state
            .toasts
            .iter()
            .all(|t| t.level != crate::state::Level::Error)
    );
}

#[test]
fn the_export_dialog_finds_errors_and_export_anyway_writes_with_notices() {
    let mut rig = Rig::new();
    rig.add_problems();
    let folder = rig.dir.path().join("out");
    rig.front
        .ui
        .open_export("chair", Some(&folder), Some(ExportFormat::MinecraftJson));
    rig.frames(3, Vec::new());
    assert_eq!(rig.front.ui.export_stage(), Some(&ExportStage::Setup));

    // Enter presses "Export": the model is checked on a worker.
    rig.enter();
    rig.until("the check", |rig| {
        matches!(rig.front.ui.export_stage(), Some(ExportStage::Review(_)))
    });
    let Some(ExportStage::Review(report)) = rig.front.ui.export_stage().cloned() else {
        unreachable!();
    };
    assert_eq!(report.errors, 2);
    assert_eq!(report.fixable, 2);
    assert!(
        report.target.contains("Java block 1.21.4"),
        "{}",
        report.target
    );
    assert!(
        !folder.join("chair.json").exists(),
        "nothing is written yet"
    );

    // The exporter itself refuses a model that does not fit.
    let path = folder.join("chair.json");
    rig.apply(UiRequest::Export(ExportRequest::Write {
        settings: rig.settings(ExportFormat::MinecraftJson),
        path: path.clone(),
        force: false,
    }));
    rig.until("the refusal", |rig| {
        matches!(rig.front.ui.export_stage(), Some(ExportStage::Failed(_)))
    });
    assert!(!path.exists());

    // "Export anyway" forces the write; the notices of the exporter reach the dialog.
    std::fs::create_dir_all(&folder).unwrap();
    rig.apply(UiRequest::Export(ExportRequest::Write {
        settings: rig.settings(ExportFormat::MinecraftJson),
        path: path.clone(),
        force: true,
    }));
    rig.until("the export", |rig| {
        matches!(rig.front.ui.export_stage(), Some(ExportStage::Done { .. }))
    });
    let Some(ExportStage::Done { notices, .. }) = rig.front.ui.export_stage().cloned() else {
        unreachable!();
    };
    assert!(!notices.is_empty(), "textures without a path give a notice");
    assert!(path.is_file());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("elements"), "{text}");
}

#[test]
fn fix_in_the_export_dialog_is_one_undo_step_and_the_next_check_is_clean() {
    let mut rig = Rig::new();
    rig.add_problems();
    rig.front.ui.open_export(
        "chair",
        Some(rig.dir.path()),
        Some(ExportFormat::ResourcePack),
    );
    rig.frames(3, Vec::new());
    rig.enter();
    rig.until("the check", |rig| {
        matches!(rig.front.ui.export_stage(), Some(ExportStage::Review(_)))
    });
    let before = rig.undo_len();
    let settings = rig.settings(ExportFormat::ResourcePack);
    rig.apply(UiRequest::Export(ExportRequest::FixErrors(
        settings.clone(),
    )));
    assert_eq!(rig.undo_len(), before + 1);
    rig.until("the second check", |rig| !rig.front.export.is_checking());
    let target = rig
        .state
        .editor
        .project
        .validation_target(rig.state.editor.doc());
    let issues = cuboid_validate::validate(
        rig.state.editor.doc(),
        &cuboid_validate::Profile::new(target),
    );
    assert!(
        issues
            .iter()
            .all(|i| i.severity != cuboid_validate::Severity::Error),
        "{issues:?}"
    );

    // Cancelling a check that runs drops its answer.
    rig.apply(UiRequest::Export(ExportRequest::Check(settings)));
    assert!(rig.front.export.is_checking());
    rig.apply(UiRequest::Export(ExportRequest::CancelCheck));
    assert!(!rig.front.export.is_checking());
}

#[test]
fn a_pack_folder_is_written_as_files() {
    let mut rig = Rig::new();
    rig.state
        .run_command(Command::AddElement(ElementKind::Cube));
    let folder = rig.dir.path().join("pack");
    rig.apply(UiRequest::Export(ExportRequest::Write {
        settings: rig.settings(ExportFormat::ResourcePackFolder),
        path: folder.clone(),
        force: false,
    }));
    rig.until("the export", |rig| rig.state.doc_job.is_none());
    assert!(folder.join("pack.mcmeta").is_file());
    assert!(rig.front.errors.is_empty(), "{:?}", rig.front.errors);
}

fn headless() -> Option<Headless> {
    match Headless::new((640, 480), 1.0) {
        Ok(headless) => Some(headless),
        Err(error) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "no wgpu adapter under CI: {error}"
            );
            eprintln!("SKIPPED (no wgpu adapter): {error}");
            None
        }
    }
}

#[test]
fn the_png_dialog_renders_through_the_headless_renderer_into_a_file() {
    let Some(mut headless) = headless() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::first_run(&Environment {
        desktop_dir: String::new(),
        minecraft_dir: String::new(),
    });
    let state = State::new(
        Dirs::under(dir.path()),
        settings,
        IoThread::spawn().unwrap(),
        1252,
        Instant::now(),
    );
    let mut app = App::with_ui(
        state,
        Prefs {
            nav_chosen: true,
            accent_chosen: true,
            ..Prefs::default()
        },
        crate::ui_setup::fonts(),
        dir.path().join("ui.json"),
    );
    let App { state, .. } = &mut app;
    assert!(
        state
            .editor
            .core
            .execute(Command::AddElement(ElementKind::Cube))
            .is_done()
    );
    for _ in 0..4 {
        headless.frame(&mut app, &[]).unwrap();
    }

    let out = dir.path().join("pictures");
    let cases = [
        (
            "current",
            RenderCamera::Current,
            RenderShading::Current,
            RenderBackground::Transparent,
        ),
        (
            "front",
            RenderCamera::Front,
            RenderShading::Current,
            RenderBackground::Transparent,
        ),
        (
            "side",
            RenderCamera::Side,
            RenderShading::Current,
            RenderBackground::Transparent,
        ),
        (
            "top",
            RenderCamera::Top,
            RenderShading::Current,
            RenderBackground::Transparent,
        ),
        (
            "iso",
            RenderCamera::Iso,
            RenderShading::ItemGui,
            RenderBackground::Transparent,
        ),
        (
            "blue",
            RenderCamera::Iso,
            RenderShading::Current,
            RenderBackground::Color([0, 0, 255]),
        ),
    ];
    for (name, camera, shading, background) in cases {
        let path = out.join(format!("{name}.png"));
        let request = UiRequest::Render(RenderRequest::Start {
            settings: RenderSettings {
                size: [48, 32],
                background,
                camera,
                shading,
                msaa: name == "iso",
            },
            path: path.clone(),
        });
        while !app.front.png_idle() {
            headless.frame(&mut app, &[]).unwrap();
        }
        let App { state, front, .. } = &mut app;
        front.apply(state, request);
        let end = Instant::now() + Duration::from_secs(60);
        while !path.is_file() {
            assert!(Instant::now() < end, "the picture {name} was not written");
            headless.frame(&mut app, &[]).unwrap();
            let failure = app
                .state
                .toasts
                .iter()
                .find(|t| t.level == crate::state::Level::Error);
            assert!(
                failure.is_none() && app.front.errors.is_empty(),
                "{name}: {:?} {:?}",
                failure,
                app.front.errors
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let bytes = std::fs::read(&path).unwrap();
        let image = decode_file(&bytes, ImageFileKind::Other).unwrap();
        assert_eq!((image.w, image.h), (48, 32), "{name}");
        let pixels = image.rgba();
        let alpha = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 3];
        let drawn = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
        // Rows run bottom to top; the corner is empty, the middle is the model.
        match background {
            RenderBackground::Transparent => {
                assert_eq!(alpha(0, 0), 0, "{name}: transparent corner");
                // The named cameras frame the model; the window camera sees a small cube far away.
                let least = if camera == RenderCamera::Current {
                    0
                } else {
                    20
                };
                assert!(drawn > least, "{name}: the model is drawn ({drawn} pixels)");
            }
            RenderBackground::Color(_) => {
                let corner = &pixels[0..4];
                assert_eq!(corner, [0, 0, 255, 255], "{name}: blue corner");
            }
        }
    }
    let end = Instant::now() + Duration::from_secs(30);
    while !app.front.png_idle() {
        assert!(Instant::now() < end, "a render is still running");
        headless.frame(&mut app, &[]).unwrap();
    }
    let App { front, state, .. } = &mut app;
    assert!(front.errors.is_empty(), "{:?}", front.errors);
    assert!(
        state
            .toasts
            .iter()
            .all(|t| t.level != crate::state::Level::Error)
    );
    assert!(
        front.png_idle(),
        "no render is left running after the pictures"
    );
    // "Open folder" asks the platform only through the recorded path in tests.
    let App { state, front, .. } = &mut app;
    front.apply(state, UiRequest::OpenFolder(out.join("iso.png")));
    assert_eq!(front.revealed, [Path::new(&out).join("iso.png")]);
}
