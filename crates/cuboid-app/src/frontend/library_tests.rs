//! The library inside the frame: requests of the UI reach the service, the start screen gets the
//! view and a save asks for a picture.

use std::time::{Duration, Instant};

use cuboid_core::{Command, ElementKind};
use cuboid_settings::{Environment, Settings};
use cuboid_ui::{FontSet, LibraryRequest, Prefs, Template, UiRequest};

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
