//! End to end on the headless device: the real frame path (UI, requests, scene, renderer, view
//! textures, painter) with scripted requests, and the private samples of oracle O1.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cuboid_actions::ActionId;
use cuboid_settings::{Environment, Settings};
use cuboid_shell::Headless;
use cuboid_testkit::oracle::{self, Oracle};
use cuboid_ui::{Prefs, UiRequest};

use crate::app::App;
use crate::io_thread::IoThread;
use crate::platform::Dirs;
use crate::state::{Intent, State};

const SIZE: (u32, u32) = (1280, 800);

fn app_in(dir: &Path) -> App {
    let settings = Settings::first_run(&Environment {
        desktop_dir: String::new(),
        minecraft_dir: String::new(),
    });
    let state = State::new(
        Dirs::under(dir),
        settings,
        IoThread::spawn().unwrap(),
        1252,
        Instant::now(),
    );
    App::with_ui(
        state,
        Prefs::default(),
        crate::ui_setup::fonts(),
        dir.join("ui.json"),
    )
}

/// A software device, or `None` with a notice (an error under CI).
fn headless() -> Option<Headless> {
    match Headless::new(SIZE, 1.0) {
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

fn frames(headless: &mut Headless, app: &mut App, count: usize) -> cuboid_shell::RgbaImage {
    let mut last = cuboid_shell::RgbaImage::default();
    for _ in 0..count {
        last = headless.frame(app, &[]).expect("the frame is read back");
    }
    last
}

/// Frames until no file job runs, within a time limit.
fn settle(headless: &mut Headless, app: &mut App) {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        frames(headless, app, 1);
        if app.state.doc_job.is_none() && app.state.autosaves.is_empty() {
            return;
        }
        assert!(Instant::now() < end, "a file job did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn request(app: &mut App, request: UiRequest) {
    let App { state, front, .. } = app;
    front.apply(state, request);
}

#[test]
fn a_cube_added_through_the_ui_renders_saves_and_opens_again() {
    let Some(mut headless) = headless() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let before = frames(&mut headless, &mut app, 6);
    assert_eq!(app.state.editor.doc().elements.len(), 0);

    request(&mut app, UiRequest::Action(ActionId::CreateElementCube));
    let after = frames(&mut headless, &mut app, 6);
    assert_eq!(app.state.editor.doc().elements.len(), 1);
    assert!(app.state.editor.is_dirty());
    assert_ne!(before, after, "the window shows the new element");
    assert!(
        app.front.rendered_slots().contains(&3),
        "the perspective view has a picture"
    );
    let health = app.front.render_health().unwrap();
    assert!(
        !health.device_lost && health.last_error.is_none(),
        "{health:?}"
    );
    assert_eq!(health.elements, 1);
    assert!(app.front.errors.is_empty(), "{:?}", app.front.errors);

    let file = dir.path().join("cube.cubik");
    app.state.dispatch(Intent::SaveAs(Some(file.clone())));
    settle(&mut headless, &mut app);
    assert!(file.is_file());
    assert!(!app.state.editor.is_dirty());
    assert!(app.front.errors.is_empty(), "{:?}", app.front.errors);

    let other = tempfile::tempdir().unwrap();
    let mut second = app_in(other.path());
    request(&mut second, UiRequest::Open(file.clone()));
    settle(&mut headless, &mut second);
    frames(&mut headless, &mut second, 4);
    assert!(second.front.errors.is_empty(), "{:?}", second.front.errors);
    assert_eq!(second.state.editor.name(), "cube");
    let (a, b) = (app.state.editor.doc(), second.state.editor.doc());
    assert_eq!(a.elements.len(), b.elements.len());
    // The file keeps the folded UV corners, so compare those.
    let folded = |element: &cuboid_model::Element| {
        let mut element = element.clone();
        for face in &mut element.faces {
            face.uv = face.uv.folded();
        }
        element
    };
    for (x, y) in a.elements.items().iter().zip(b.elements.items()) {
        assert_eq!(folded(x), folded(y));
    }
    assert_eq!(a.textures.entries.len(), b.textures.entries.len());
    assert!(second.front.rendered_slots().contains(&3));
}

/// Opens every file of the private sample folder and renders it.
#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn every_private_sample_opens_and_renders() {
    let Some(folder) = oracle::require(Oracle::ParityRefs) else {
        return;
    };
    let Some(mut headless) = headless() else {
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("cubik") || ext.eq_ignore_ascii_case("cub")
            })
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the sample folder holds no model");
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    for file in files {
        request(&mut app, UiRequest::Open(file));
        settle(&mut headless, &mut app);
        // The extract and the uploads continue over several frames.
        frames(&mut headless, &mut app, 12);
        assert!(app.front.errors.is_empty(), "{:?}", app.front.errors);
        let health = app.front.render_health().unwrap();
        assert!(
            !health.device_lost && health.last_error.is_none(),
            "{health:?}"
        );
        assert!(app.front.rendered_slots().contains(&3));
    }
}
