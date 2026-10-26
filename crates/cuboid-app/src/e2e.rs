//! End to end on the headless device: the real frame path (UI, requests, scene, renderer, view
//! textures, painter) with scripted requests, and the private samples of oracle O1.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cuboid_actions::{ActionId, EngineKey};
use cuboid_settings::{Environment, Settings};
use cuboid_shell::{Headless, ScriptedInput, WindowCommand};
use cuboid_testkit::oracle::{self, Oracle};
use cuboid_ui::{NavPreset, Prefs, UiRequest};

use crate::app::App;
use crate::io_thread::IoThread;
use crate::platform::Dirs;
use crate::state::{Intent, State};

const SIZE: (u32, u32) = (1280, 800);

fn app_in(dir: &Path) -> App {
    app_with_nav(dir, NavPreset::default())
}

fn app_with_nav(dir: &Path, nav: NavPreset) -> App {
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
        Prefs {
            nav,
            nav_chosen: true,
            ..Prefs::default()
        },
        crate::ui_setup::fonts(),
        dir.join("ui.json"),
    )
}

/// A software device, or `None` with a notice (an error under CI).
fn headless() -> Option<Headless> {
    headless_scaled(1.0)
}

fn headless_scaled(scale: f32) -> Option<Headless> {
    match Headless::new(SIZE, scale) {
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
fn the_title_bar_choice_reaches_the_shell_as_a_window_command() {
    let Some(mut headless) = headless() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    frames(&mut headless, &mut app, 3);
    assert!(headless.take_window_commands().is_empty());

    let prefs = |system_title_bar| {
        Box::new(Prefs {
            system_title_bar,
            ..Prefs::default()
        })
    };
    request(&mut app, UiRequest::PrefsChanged(prefs(true)));
    frames(&mut headless, &mut app, 2);
    let commands = headless.take_window_commands();
    assert_eq!(commands, [WindowCommand::SetDecorations(true)]);

    request(&mut app, UiRequest::PrefsChanged(prefs(false)));
    frames(&mut headless, &mut app, 2);
    let commands = headless.take_window_commands();
    assert_eq!(commands, [WindowCommand::SetDecorations(false)]);
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

/// A drag in the window moves the camera and the picture of the view follows, in each preset.
#[test]
fn dragging_in_the_window_orbits_the_view() {
    for (nav, button, scale) in [
        (NavPreset::Blockbench, EngineKey::Mouse0, 1.0),
        (NavPreset::Blender, EngineKey::Mouse2, 1.0),
        (NavPreset::Blockbench, EngineKey::Mouse0, 1.5),
        (NavPreset::Blender, EngineKey::Mouse2, 1.5),
    ] {
        let Some(mut headless) = headless_scaled(scale) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_nav(dir.path(), nav);
        request(&mut app, UiRequest::Action(ActionId::CreateElementCube));
        let before = frames(&mut headless, &mut app, 8);
        let pos_before = app.front.views.views[3].camera.pos;
        let view = app.front.placements[0];
        let (x, y) = (view.rect[0] + 40.0, view.rect[1] + 40.0);
        let key = |down| ScriptedInput::Key(cuboid_shell::EngineKeyEvent { key: button, down });
        let script = [
            vec![ScriptedInput::PointerMoved([x, y])],
            vec![key(true)],
            vec![ScriptedInput::PointerMoved([x + 30.0, y + 10.0])],
            vec![ScriptedInput::PointerMoved([x + 70.0, y + 20.0])],
            vec![key(false)],
        ];
        for step in &script {
            headless.frame(&mut app, step).unwrap();
        }
        let after = frames(&mut headless, &mut app, 6);
        assert_ne!(
            app.front.views.views[3].camera.pos, pos_before,
            "{nav:?} at {scale}: the camera did not move"
        );
        assert_ne!(
            before, after,
            "{nav:?} at {scale}: the picture did not follow"
        );
    }
}

/// A project with one red cube, for the pictures of models.
fn red_cube_project() -> String {
    use cuboid_model::image::{Image, RgbaBuf, encode_png};
    let mut buf = RgbaBuf::new(16, 16);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&[220, 20, 20, 255]);
    }
    let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
    let url = cuboid_fmt_bbmodel::data_url::png_url(&png);
    let face = r#"{"uv": [0, 0, 16, 16], "texture": 0}"#;
    format!(
        r#"{{"meta": {{"format_version": "4.10", "model_format": "free"}}, "name": "cube",
        "resolution": {{"width": 16, "height": 16}},
        "elements": [{{"name": "a", "from": [0, 0, 0], "to": [16, 16, 16], "origin": [8, 8, 8],
            "faces": {{"north": {face}, "east": {face}, "south": {face}, "west": {face},
                       "up": {face}, "down": {face}}}}}],
        "textures": [{{"name": "t.png", "id": "0", "source": "{url}"}}]}}"#
    )
}

#[test]
fn a_saved_model_gets_a_160_px_picture_from_the_frame_path() {
    let Some(mut headless) = headless() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let model = dir.path().join("cube.bbmodel");
    std::fs::write(&model, red_cube_project()).unwrap();
    app.state.library.thumbnail_saved(&model);
    let end = Instant::now() + Duration::from_secs(60);
    let picture = loop {
        frames(&mut headless, &mut app, 1);
        let shown = app
            .state
            .library
            .recent_thumbs(std::slice::from_ref(&model))
            .remove(0);
        if let Some(picture) = shown {
            break picture;
        }
        assert!(Instant::now() < end, "no picture came");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(picture.size, [160, 160]);
    let at = |x: usize, y: usize| picture.rgba[(y * 160 + x) * 4..][..4].to_vec();
    assert_eq!(at(0, 0)[3], 0, "the corner is clear");
    let centre = at(80, 80);
    assert!(
        centre[3] == 255 && centre[0] > 100 && centre[1] < 100,
        "the red cube is in the middle: {centre:?}"
    );
    // The picture is on disk for the next start.
    let cache = dir.path().join("data").join("cache").join("thumbs");
    assert!(cuboid_library::thumb::recent_thumb_path(&cache, &model).is_file());
    assert!(!app.front.thumbs_busy(&app.state));
}

#[test]
fn a_model_that_cannot_be_opened_keeps_its_placeholder_and_the_queue_goes_on() {
    let Some(mut headless) = headless() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let broken = dir.path().join("broken.bbmodel");
    std::fs::write(&broken, b"{ nope").unwrap();
    let good = dir.path().join("cube.bbmodel");
    std::fs::write(&good, red_cube_project()).unwrap();
    app.state.library.thumbnail_saved(&broken);
    app.state.library.thumbnail_saved(&good);
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        frames(&mut headless, &mut app, 1);
        let shown = app
            .state
            .library
            .recent_thumbs(&[broken.clone(), good.clone()]);
        if shown[1].is_some() {
            assert!(shown[0].is_none(), "the broken one stays a placeholder");
            break;
        }
        assert!(Instant::now() < end, "no picture came");
        std::thread::sleep(Duration::from_millis(5));
    }
}
