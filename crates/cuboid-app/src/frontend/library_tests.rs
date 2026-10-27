//! The library inside the frame: requests of the UI reach the service, the start screen gets the
//! view and a save asks for a picture.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cuboid_core::{Command, ElementKind};
use cuboid_library::{ImportOptions, Library, PROJECT_DIR, Quiet, Scope};
use cuboid_settings::{Environment, Settings};
use cuboid_ui::{
    BriefInputs, FontSet, LibScope, LibraryRequest, Prefs, StyleSourceView, Template, UiRequest,
};

use super::*;
use crate::io_thread::IoThread;
use crate::library::JOB_ID;
use crate::platform::Dirs;
use crate::state::Intent;

fn setup() -> (tempfile::TempDir, State, Frontend) {
    let dir = tempfile::tempdir().expect("temp dir");
    let settings = Settings::first_run(&Environment {
        desktop_dir: String::new(),
        minecraft_dir: String::new(),
    });
    let state = State::new(
        Dirs::under(dir.path()),
        settings,
        IoThread::spawn().expect("thread"),
        1252,
        Instant::now(),
    );
    let prefs = Prefs {
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    };
    let front = Frontend::new(prefs, FontSet::default(), dir.path().join("ui.json"));
    (dir, state, front)
}

fn settle(state: &mut State, done: impl Fn(&State) -> bool) {
    let start = Instant::now();
    while !done(state) {
        state.step(Instant::now());
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_empty_template_makes_a_clean_model_without_objects() {
    let (_dir, mut state, mut front) = setup();
    state.run_command(Command::AddElement(ElementKind::Cube));
    assert!(state.editor.is_dirty());
    front.apply(&mut state, UiRequest::NewDocument(Template::Empty));
    // The open question of a dirty model is the UI's; the request came after it.
    assert!(state.editor.core.doc().elements.ids().is_empty());
    assert!(!state.editor.is_dirty());
}

#[test]
fn library_requests_reach_the_service_and_the_job_can_be_cancelled_by_id() {
    let (dir, mut state, mut front) = setup();
    settle(&mut state, |s| !s.library.view().busy);
    let note = dir.path().join("src").join("note.md");
    std::fs::create_dir_all(note.parent().expect("parent")).expect("dir");
    std::fs::write(&note, b"# note\n").expect("write");
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::Import(vec![note])),
    );
    assert_eq!(state.library.jobs().first().map(|j| j.id), Some(JOB_ID));
    // The job id of the UI reaches the library, not the file job.
    front.apply(&mut state, UiRequest::CancelJob(JOB_ID));
    settle(&mut state, |s| !s.library.view().busy);
    // Pinning a reference is announced as not available yet and changes nothing.
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::PinReference("x".to_owned())),
    );
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.text.contains("not available yet"))
    );
}

/// A project folder next to `tower.cubik` whose library holds one style folder, written the way
/// the library keeps it. Returns the project folder.
fn project_with_a_style_folder(dir: &Path) -> PathBuf {
    let work = dir.join("work");
    let lamps = work.join("refs").join("lamps");
    std::fs::create_dir_all(&lamps).expect("dir");
    let model = br#"{"elements": [{"from": [0, 0, 0], "to": [4, 12, 4]}]}"#;
    std::fs::write(lamps.join("post.json"), model).expect("write");
    let mut library = Library::new(work.join(PROJECT_DIR), Scope::Project);
    let options = ImportOptions {
        licence: "CC0".to_owned(),
        ..ImportOptions::default()
    };
    library
        .import_style(&lamps, &options, &Quiet)
        .expect("imports");
    work
}

#[test]
fn saving_the_brief_keeps_the_style_sources() {
    let (dir, mut state, mut front) = setup();
    let work = project_with_a_style_folder(dir.path());
    // The project library follows the folder of the saved document.
    state.run_command(Command::AddElement(ElementKind::Cube));
    state.dispatch(Intent::SaveAs(Some(work.join("tower.cubik"))));
    settle(&mut state, |s| {
        s.doc_job.is_none() && s.library.view().project_available
    });
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::SetScope(LibScope::Project)),
    );
    settle(&mut state, |s| {
        let view = s.library.view();
        view.scope == LibScope::Project && !view.busy
    });
    let lamps = StyleSourceView {
        path: "refs/lamps".to_owned(),
        models: 1,
        textures: 0,
        licence: "CC0".to_owned(),
    };
    assert_eq!(
        state.library.view().brief.style_sources,
        std::slice::from_ref(&lamps)
    );

    let inputs = BriefInputs {
        project_name: "Lamps".to_owned(),
        rules: vec!["Keep the posts thin".to_owned()],
        ..BriefInputs::default()
    };
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::SetBrief(Box::new(inputs))),
    );
    settle(&mut state, |s| {
        let view = s.library.view();
        !view.busy && view.brief.inputs.project_name == "Lamps"
    });
    assert_eq!(
        state.library.view().brief.style_sources,
        std::slice::from_ref(&lamps)
    );
    // The index on disk kept them too.
    let saved = Library::read(&work.join(PROJECT_DIR), Scope::Project).expect("reads");
    assert_eq!(saved.brief().project_name, "Lamps");
    assert_eq!(saved.brief().style_sources.len(), 1);
    assert_eq!(saved.brief().style_sources[0].licence, "CC0");
}

#[test]
fn style_requests_reach_the_service_through_the_frame_requests() {
    let (dir, mut state, mut front) = setup();
    let work = project_with_a_style_folder(dir.path());
    state.run_command(Command::AddElement(ElementKind::Cube));
    state.dispatch(Intent::SaveAs(Some(work.join("tower.cubik"))));
    settle(&mut state, |s| {
        s.doc_job.is_none() && s.library.view().project_available
    });
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::SetScope(LibScope::Project)),
    );
    settle(&mut state, |s| {
        let view = s.library.view();
        view.scope == LibScope::Project && !view.busy
    });
    let id = state.library.view().entries[0].id.clone();
    // Own work first, then a reference again.
    for style in [false, true] {
        let request = LibraryRequest::SetRole {
            id: id.clone(),
            style,
        };
        front.apply(&mut state, UiRequest::Library(request));
        settle(&mut state, |s| {
            let view = s.library.view();
            !view.busy && view.entries[0].style == style
        });
    }
    front.apply(
        &mut state,
        UiRequest::Library(LibraryRequest::RemoveStyleSource("refs/lamps".to_owned())),
    );
    settle(&mut state, |s| {
        let view = s.library.view();
        !view.busy && view.total == 0
    });
    assert!(state.library.view().brief.style_sources.is_empty());
    assert!(work.join("refs").join("lamps").join("post.json").is_file());
}

#[test]
fn a_save_asks_for_a_picture_of_the_file() {
    let (dir, mut state, _front) = setup();
    state.run_command(Command::AddElement(ElementKind::Cube));
    let path = dir.path().join("tower.cubik");
    state.dispatch(Intent::SaveAs(Some(path.clone())));
    settle(&mut state, |s| s.doc_job.is_none());
    let request = state.library.take_render().expect("a render request");
    assert_eq!(request.source, path);
    assert!(request.out.starts_with(dir.path().join("data")));
}

#[test]
fn an_export_of_a_model_file_asks_for_a_picture_of_the_file() {
    let (dir, mut state, _front) = setup();
    state.run_command(Command::AddElement(ElementKind::Cube));
    // The document is untitled, as an imported project is: writing it back is an export.
    let mesh = dir.path().join("tower.obj");
    state.dispatch(Intent::ExportCopy(Some(mesh)));
    settle(&mut state, |s| s.doc_job.is_none());
    assert!(!state.library.has_renders(), "a mesh has no picture");
    let path = dir.path().join("tower.bbmodel");
    state.dispatch(Intent::ExportCopy(Some(path.clone())));
    settle(&mut state, |s| s.doc_job.is_none());
    let request = state.library.take_render().expect("a render request");
    assert_eq!(request.source, path);
    assert!(request.out.starts_with(dir.path().join("data")));
    assert!(state.library.take_render().is_none());
}

#[test]
fn the_ui_frame_gets_the_library_and_the_pictures_of_recent_files() {
    let (_dir, mut state, mut front) = setup();
    front.ui.show_start();
    state.settings.recent_files.add("C:/models/tower.cubik");
    let ctx = egui::Context::default();
    for _ in 0..3 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..egui::RawInput::default()
        };
        ctx.begin_pass(raw);
        front.update(&mut state, &ctx, Instant::now());
        ctx.end_pass().drop_without_applying_deltas();
    }
    // The recent file was looked up for a picture; there is none, so a render waits only when
    // the file exists. A missing file asks for nothing.
    assert!(!state.library.has_renders());
    assert!(state.library.view().rev > 0);
}
