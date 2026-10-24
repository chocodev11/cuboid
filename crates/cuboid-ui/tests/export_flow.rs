//! The export and render dialogs through the public API: the requests they send and how they
//! react to the answers of the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_ui::{
    ColorMode, ExportFormat, ExportInfo, ExportOutcome, ExportReport, ExportRequest, ExportStage,
    FontSet, IssueLevel, IssueView, NodeId, Prefs, RenderBackground, RenderCamera, RenderRequest,
    RenderShading, Ui, UiCx, UiRequest, VersionChoice,
};
use egui::{Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct App {
    ui: Ui,
    requests: Vec<UiRequest>,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let cx = UiCx::empty(&[]);
    let out = app.ui.show(ui, &cx);
    app.requests.extend(out.requests);
}

fn harness() -> Harness<'static, App> {
    let prefs = Prefs {
        color_mode: ColorMode::Dark,
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    };
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_export_info(ExportInfo {
        versions: vec![
            VersionChoice {
                id: "26.4".into(),
                pack_format: None,
            },
            VersionChoice {
                id: "1.21.11".into(),
                pack_format: Some("75".into()),
            },
        ],
        default_version: String::new(),
        folder: PathBuf::from("out"),
    });
    let app = App {
        ui,
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(900.0, 700.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

fn take(harness: &mut Harness<'static, App>) -> Vec<UiRequest> {
    std::mem::take(&mut harness.state_mut().requests)
}

fn click(harness: &mut Harness<'static, App>, label: &str) {
    // A dialog that changed its content settles over a few frames.
    harness.run_steps(8);
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run_steps(3);
}

fn error_issue(code: &str, fixable: bool) -> IssueView {
    IssueView {
        code: Arc::from(code),
        level: IssueLevel::Error,
        objects: vec![NodeId(2)],
        message: Arc::from(format!("{code} is broken")),
        hint: Arc::from("fix it"),
        fixable,
    }
}

fn report(errors: usize, fixable: usize) -> ExportReport {
    ExportReport {
        target: "Java block 26.4".into(),
        errors,
        warnings: 1,
        fixable,
        issues: (0..errors)
            .map(|i| error_issue(&format!("GEO_{i}"), i < fixable))
            .collect(),
    }
}

/// Opens the dialog and presses Export; the check is answered with `answer`.
fn reach_review(harness: &mut Harness<'static, App>, answer: ExportReport) {
    harness.state_mut().ui.open_export("chair", None, None);
    harness.run_steps(3);
    click(harness, "Export");
    let requests = take(harness);
    assert!(
        matches!(
            requests.as_slice(),
            [UiRequest::Export(ExportRequest::Check(settings))]
                if settings.format == ExportFormat::Cubik
        ),
        "{requests:?}"
    );
    assert_eq!(
        harness.state().ui.export_stage(),
        Some(&ExportStage::Checking)
    );
    harness.state_mut().ui.export_checked(answer);
    harness.run_steps(3);
}

#[test]
fn errors_show_a_summary_and_export_anyway_forces_the_write() {
    let mut harness = harness();
    reach_review(&mut harness, report(2, 1));
    assert!(matches!(
        harness.state().ui.export_stage(),
        Some(ExportStage::Review(r)) if r.errors == 2
    ));
    harness.get_by_label_contains("2 errors and 1 warning for Java block 26.4");
    harness.get_by_label_contains("GEO_0 is broken");
    harness.get_by_role_and_label(Role::Button, "Fix 1");
    harness.get_by_role_and_label(Role::Button, "Cancel");

    click(&mut harness, "Export anyway");
    let requests = take(&mut harness);
    let [UiRequest::Export(ExportRequest::Write { path, force, .. })] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert!(*force);
    assert_eq!(path, &Path::new("out").join("chair.cubik"));
    assert_eq!(
        harness.state().ui.export_stage(),
        Some(&ExportStage::Writing)
    );
}

#[test]
fn cancel_in_the_summary_writes_nothing_and_closes() {
    let mut harness = harness();
    reach_review(&mut harness, report(3, 0));
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Fix 0")
            .is_none(),
        "nothing to fix: no Fix button"
    );
    click(&mut harness, "Cancel");
    assert!(take(&mut harness).is_empty());
    assert_eq!(harness.state().ui.export_stage(), None);
}

#[test]
fn fix_asks_the_app_to_fix_and_check_again() {
    let mut harness = harness();
    reach_review(&mut harness, report(1, 1));
    click(&mut harness, "Fix 1");
    let requests = take(&mut harness);
    assert!(
        matches!(
            requests.as_slice(),
            [UiRequest::Export(ExportRequest::FixErrors(_))]
        ),
        "{requests:?}"
    );
    assert_eq!(
        harness.state().ui.export_stage(),
        Some(&ExportStage::Checking)
    );
    // The second check is clean: the file is written without a question.
    harness.state_mut().ui.export_checked(report(0, 0));
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(
        matches!(
            requests.as_slice(),
            [UiRequest::Export(ExportRequest::Write { force: false, .. })]
        ),
        "{requests:?}"
    );
}

#[test]
fn the_notices_of_the_exporter_are_listed_after_the_write() {
    let mut harness = harness();
    reach_review(&mut harness, report(0, 0));
    take(&mut harness);
    assert_eq!(
        harness.state().ui.export_stage(),
        Some(&ExportStage::Writing)
    );
    harness.state_mut().ui.export_finished(ExportOutcome::Done {
        path: PathBuf::from("out/chair.cubik"),
        notices: vec![
            "skipped 2 item overrides".into(),
            "1 texture was given a path".into(),
        ],
    });
    harness.run_steps(3);
    harness.get_by_label_contains("2 notices from the exporter");
    harness.get_by_label_contains("skipped 2 item overrides");
    harness.get_by_label_contains("1 texture was given a path");
    click(&mut harness, "Open folder");
    assert_eq!(
        take(&mut harness),
        [UiRequest::OpenFolder(PathBuf::from("out/chair.cubik"))]
    );
    click(&mut harness, "Close");
    assert_eq!(harness.state().ui.export_stage(), None);
}

#[test]
fn a_failed_export_shows_its_text() {
    let mut harness = harness();
    reach_review(&mut harness, report(0, 0));
    harness
        .state_mut()
        .ui
        .export_finished(ExportOutcome::Failed(
            "Cannot export chair: disk full".into(),
        ));
    harness.run_steps(3);
    harness.get_by_label_contains("disk full");
}

#[test]
fn a_result_without_a_dialog_becomes_a_toast_with_open_folder() {
    let mut harness = harness();
    harness.state_mut().ui.export_finished(ExportOutcome::Done {
        path: PathBuf::from("out/pack.zip"),
        notices: vec!["a".into()],
    });
    harness.run_steps(3);
    harness.get_by_label_contains("Exported out");
    click(&mut harness, "Open folder");
    assert_eq!(
        take(&mut harness),
        [UiRequest::OpenFolder(PathBuf::from("out/pack.zip"))]
    );
}

#[test]
fn png_hands_over_to_the_render_dialog_which_asks_for_a_picture() {
    let mut harness = harness();
    harness.state_mut().ui.open_export("chair", None, None);
    harness.run_steps(3);
    // Pick the PNG entry of the format list.
    click(&mut harness, "PNG picture (.png)");
    click(&mut harness, "Continue");
    assert!(take(&mut harness).is_empty());
    assert_eq!(harness.state().ui.export_stage(), None);
    assert!(harness.state().ui.render_open());

    click(&mut harness, "Render");
    let requests = take(&mut harness);
    let [UiRequest::Render(RenderRequest::Start { settings, path })] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(settings.size, [1024, 1024]);
    assert_eq!(settings.background, RenderBackground::Transparent);
    assert_eq!(settings.camera, RenderCamera::Current);
    assert_eq!(settings.shading, RenderShading::Current);
    assert!(settings.msaa);
    assert_eq!(path, &Path::new("out").join("chair.png"));

    // A failure returns to the choices with the reason; a success closes with a toast.
    harness
        .state_mut()
        .ui
        .render_finished(Err("out of GPU memory".into()));
    harness.run_steps(3);
    harness.get_by_label_contains("out of GPU memory");
    assert!(harness.state().ui.render_open());
    harness
        .state_mut()
        .ui
        .render_finished(Ok(PathBuf::from("out/chair.png")));
    harness.run_steps(3);
    assert!(!harness.state().ui.render_open());
    harness.get_by_label_contains("Rendered out");
    click(&mut harness, "Open folder");
    assert_eq!(
        take(&mut harness),
        [UiRequest::OpenFolder(PathBuf::from("out/chair.png"))]
    );
}

#[test]
fn minecraft_formats_offer_the_versions_and_mark_the_unknown_ones() {
    let mut harness = harness();
    harness.state_mut().ui.open_export("chair", None, None);
    harness.run_steps(8);
    assert!(
        harness.query_by_label_contains("pack format").is_none(),
        "the Cubik copy has no game version"
    );
    click(&mut harness, "Minecraft model (.json)");
    harness.run_steps(8);
    harness.get_by_label_contains("26.4  (no known pack format)");
    click(&mut harness, "1.21.11  (pack format 75)");
    click(&mut harness, "Export");
    let requests = take(&mut harness);
    let [UiRequest::Export(ExportRequest::Check(settings))] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(settings.format, ExportFormat::MinecraftJson);
    assert_eq!(settings.version, "1.21.11");
}
