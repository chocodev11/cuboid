//! Flow, recovery and panic-boundary tests on temporary folders. A frame is driven by calling
//! the guarded step; the dialogs of the system are not involved, paths come as intents.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cuboid_core::{Command, ElementKind, MeshShape};
use cuboid_settings::{Environment, Settings};
use cuboid_shell::{ShellApp, ShellEvent};
use tempfile::TempDir;

use crate::app::App;
use crate::backup;
use crate::flows::ensure_cubik_extension;
use crate::io_thread::IoThread;
use crate::platform::Dirs;
use crate::state::{Dialog, Intent, Level, State, UnsavedAnswer};

mod panel;

fn settings(minutes: i32) -> Settings {
    let mut settings = Settings::first_run(&Environment {
        desktop_dir: String::new(),
        minecraft_dir: String::new(),
    });
    settings.backup_every_minutes = minutes;
    settings
}

fn app_in(dir: &Path, minutes: i32, now: Instant) -> App {
    let state = State::new(
        Dirs::under(dir),
        settings(minutes),
        IoThread::spawn().unwrap(),
        1252,
        now,
    );
    App::new(state)
}

struct Fixture {
    dir: Arc<TempDir>,
    app: App,
    /// The clock of the frames.
    now: Instant,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture::with_interval(10)
    }

    fn with_interval(minutes: i32) -> Fixture {
        let dir = Arc::new(tempfile::tempdir().unwrap());
        let now = Instant::now();
        let app = app_in(dir.path(), minutes, now);
        Fixture { dir, app, now }
    }

    /// The program starts again on the same folders after the old run died: nothing was cleaned up.
    fn restart(&mut self) -> Fixture {
        self.state().io.shutdown(Duration::from_secs(10));
        let now = Instant::now();
        let mut app = app_in(self.dir.path(), 10, now);
        app.state.startup(None);
        Fixture {
            dir: Arc::clone(&self.dir),
            app,
            now,
        }
    }

    /// A second instance of the program on the same folders while this one keeps running.
    fn sibling(&self) -> Fixture {
        let now = Instant::now();
        let mut app = app_in(self.dir.path(), 10, now);
        app.state.startup(None);
        Fixture {
            dir: Arc::clone(&self.dir),
            app,
            now,
        }
    }

    /// The program dies: its lock goes away with the process, the autosaves stay.
    fn crash(&mut self) {
        self.state().io.shutdown(Duration::from_secs(10));
        self.state().session_lock = None;
    }

    /// What a start-up scan from this instance offers.
    fn scan(&mut self) -> Vec<backup::Candidate> {
        let session = self.state().session.clone();
        backup::scan(&self.recovery(), &session)
    }

    /// The autosave files in the folder that belong to this session.
    fn own_autosaves(&mut self) -> usize {
        let prefix = format!("{}_", self.state().session);
        fs::read_dir(self.recovery())
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(".json")
            })
            .count()
    }

    /// Edits and autosaves `times` times.
    fn autosaves(&mut self, times: usize) {
        for _ in 0..times {
            self.edit();
            self.state().request_autosave(backup::Reason::Timer);
            self.settle();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn recovery(&self) -> PathBuf {
        Dirs::under(self.dir.path()).recovery()
    }

    fn state(&mut self) -> &mut State {
        &mut self.app.state
    }

    /// One frame at the fixture's clock.
    fn frame(&mut self) {
        let now = self.now;
        assert!(
            self.app.guarded(now, |state| state.step(now)),
            "the frame panicked"
        );
    }

    /// Frames until every job is done.
    fn settle(&mut self) {
        let start = Instant::now();
        loop {
            self.frame();
            if !self.app.state.is_busy() {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "jobs did not finish"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Advances the clock a little so that the good state is refreshed.
    fn tick(&mut self) {
        self.now += Duration::from_secs(1);
        self.frame();
    }

    fn send(&mut self, intent: Intent) {
        self.state().inbox.push(intent);
        self.settle();
    }

    /// Adds one cube through the command path.
    fn edit(&mut self) {
        let add = Command::AddElement(ElementKind::Cube);
        self.state().inbox.push(Intent::Command(Box::new(add)));
        self.tick();
    }

    fn cubes(&self) -> usize {
        self.app.state.editor.doc().elements.len()
    }

    fn toasts(&self, level: Level) -> Vec<String> {
        let texts = self.app.state.toasts.iter();
        texts
            .filter(|toast| toast.level == level)
            .map(|toast| toast.text.clone())
            .collect()
    }

    fn saved_file(&mut self, name: &str) -> PathBuf {
        let path = self.path(name);
        self.edit();
        self.send(Intent::SaveAs(Some(path.clone())));
        path
    }
}

#[test]
fn extension_is_added_when_missing() {
    assert_eq!(ensure_cubik_extension("a".into()), PathBuf::from("a.cubik"));
    assert_eq!(
        ensure_cubik_extension("a.CUBIK".into()),
        PathBuf::from("a.CUBIK")
    );
    assert_eq!(
        ensure_cubik_extension("a.v2".into()),
        PathBuf::from("a.v2.cubik")
    );
}

#[test]
fn a_saved_model_opens_again_with_its_content() {
    let mut fx = Fixture::new();
    let path = fx.saved_file("model.cubik");
    assert!(path.is_file());
    assert_eq!(fx.state().editor.path().as_deref(), Some(path.as_path()));
    assert!(!fx.state().editor.is_dirty());
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .any(|text| text.starts_with("Saved model"))
    );
    assert_eq!(fx.state().settings.recent_files.list.len(), 1);
    // The atomic write leaves nothing but the file.
    assert!(!fs::read_dir(fx.dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tmp")
    }));

    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path.clone())));
    assert_eq!(other.cubes(), 1);
    assert_eq!(other.state().editor.path().as_deref(), Some(path.as_path()));
    assert!(!other.state().editor.is_dirty());
    assert!(
        other
            .toasts(Level::Info)
            .iter()
            .any(|text| text.starts_with("Opened model"))
    );
}

#[test]
fn save_overwrites_the_same_file_and_keeps_the_model_clean() {
    let mut fx = Fixture::new();
    let path = fx.saved_file("model.cubik");
    fx.edit();
    assert!(fx.state().editor.is_dirty());
    fx.send(Intent::Save);
    assert!(!fx.state().editor.is_dirty());
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    assert_eq!(other.cubes(), 2);
}

#[test]
fn a_primitive_mesh_gets_its_geometry_when_the_file_is_opened() {
    let mut fx = Fixture::new();
    let add = Command::AddMesh(MeshShape::Cylinder);
    fx.state().inbox.push(Intent::Command(Box::new(add)));
    fx.tick();
    let path = fx.path("mesh.cubik");
    fx.send(Intent::SaveAs(Some(path.clone())));
    // The file holds no geometry for a primitive.
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    let doc = other.state().editor.doc();
    assert_eq!(doc.meshes.len(), 1);
    assert_eq!(doc.meshes.items()[0].geometry.vertices.len(), 132);
}

#[test]
fn a_display_slot_is_kept_by_the_file_and_follows_undo() {
    use cuboid_core::DisplayContext;
    use cuboid_model::{DisplaySlot, DisplaySlotId};
    use glam::Vec3;

    let mut fx = Fixture::new();
    let set = Command::SetDisplaySlot {
        slot: DisplayContext::Gui,
        enabled: Some(true),
        rotation: Some([30.0, 225.0, 0.0]),
        translation: Some([0.0, 1.5, 0.0]),
        scale: Some([0.625; 3]),
        preset: Some("block/block".to_owned()),
    };
    fx.state().inbox.push(Intent::Command(Box::new(set)));
    fx.tick();
    let gui = |fx: &mut Fixture| fx.state().editor.doc().display[DisplaySlotId::Gui].clone();
    assert!(gui(&mut fx).enabled);
    let path = fx.path("display.cubik");
    fx.send(Intent::SaveAs(Some(path.clone())));

    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    let loaded = gui(&mut other);
    assert!(loaded.enabled);
    assert_eq!(loaded.rotation, Vec3::new(30.0, 225.0, 0.0));
    assert_eq!(loaded.translation, Vec3::new(0.0, 1.5, 0.0));
    assert_eq!(loaded.scale, Vec3::splat(0.625));
    assert_eq!(loaded.preset, "block/block");

    // The same edit is one undo step: undo hands the empty slot back, redo the set one.
    fx.state()
        .inbox
        .push(Intent::Command(Box::new(Command::Undo)));
    fx.tick();
    assert_eq!(gui(&mut fx), DisplaySlot::default());
    fx.state()
        .inbox
        .push(Intent::Command(Box::new(Command::Redo)));
    fx.tick();
    assert_eq!(gui(&mut fx).rotation, Vec3::new(30.0, 225.0, 0.0));
}

#[test]
fn opening_a_missing_file_is_a_message_and_leaves_the_document() {
    let mut fx = Fixture::new();
    let gone = fx.path("gone.cubik");
    fx.state()
        .settings
        .recent_files
        .add(&gone.display().to_string());
    fx.send(Intent::Open(Some(gone)));
    assert!(
        fx.toasts(Level::Error)
            .iter()
            .any(|text| text.contains("does not exist"))
    );
    assert!(fx.state().settings.recent_files.list.is_empty());
    assert!(!fx.state().editor.is_dirty());
}

#[test]
fn opening_damaged_and_unknown_files_is_a_message() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::SaveAs(Some(fx.path("keep.cubik"))));
    for (name, bytes) in [
        ("junk.cubik", &b"this is not a zip file"[..]),
        ("empty.cubik", &b""[..]),
        ("bad.cub", &b"{ not json"[..]),
        ("notes.txt", &b"hello"[..]),
    ] {
        fs::write(fx.path(name), bytes).unwrap();
        fx.send(Intent::Open(Some(fx.path(name))));
        assert!(
            fx.toasts(Level::Error)
                .iter()
                .any(|text| text.contains(name)),
            "no error toast for {name}: {:?}",
            fx.toasts(Level::Error)
        );
    }
    assert_eq!(fx.cubes(), 1);
    assert!(!fx.state().editor.is_dirty());
}

#[test]
fn a_cub_file_opens_as_an_untitled_model() {
    let mut fx = Fixture::new();
    let plain = r#"{"textures_path":{},"elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#;
    // The obfuscation is its own inverse.
    let cub = cuboid_fmt_cubik::cub::deobfuscate(plain.as_bytes());
    fs::write(fx.path("block.cub"), cub).unwrap();
    fx.send(Intent::Open(Some(fx.path("block.cub"))));
    assert_eq!(
        fx.state().editor.doc().elements.len(),
        1,
        "{:?}",
        fx.toasts(Level::Error)
    );
    assert!(fx.state().editor.path().is_none());
}

#[test]
fn a_dirty_document_asks_before_new_and_cancel_keeps_it() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::New);
    assert!(matches!(fx.state().dialog, Some(Dialog::Unsaved { .. })));
    fx.send(Intent::Unsaved(UnsavedAnswer::Cancel));
    assert!(fx.state().dialog.is_none());
    assert_eq!(fx.cubes(), 1);
    assert!(fx.state().editor.is_dirty());
}

#[test]
fn discarding_starts_a_new_model_and_keeps_an_autosave_of_the_old_one() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::New);
    fx.send(Intent::Unsaved(UnsavedAnswer::Discard));
    assert_eq!(fx.cubes(), 0);
    assert!(!fx.state().editor.is_dirty());
    let found = fx.scan();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, "risky");
}

#[test]
fn saving_from_the_question_saves_then_continues_with_the_action() {
    let mut fx = Fixture::new();
    let path = fx.saved_file("first.cubik");
    fx.edit();
    fx.send(Intent::New);
    fx.send(Intent::Unsaved(UnsavedAnswer::Save));
    assert_eq!(fx.cubes(), 0, "the new model replaced the old one");
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    assert_eq!(other.cubes(), 2);
}

#[test]
fn quit_asks_when_dirty_and_then_cleans_up_and_writes_the_settings() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.state()
        .autosave_and_remove(backup::Reason::Risky, Vec::new());
    fx.settle();
    assert_eq!(fx.scan().len(), 1);

    fx.send(Intent::Quit);
    assert!(!fx.state().quit);
    assert!(matches!(fx.state().dialog, Some(Dialog::Unsaved { .. })));
    fx.send(Intent::Unsaved(UnsavedAnswer::Discard));
    assert!(fx.state().quit);
    fx.state().io.shutdown(Duration::from_secs(10));
    assert!(fx.scan().is_empty(), "this session's autosaves are removed");
    assert!(Dirs::under(fx.dir.path()).settings_file().is_file());
}

#[test]
fn quit_with_a_clean_document_leaves_at_once() {
    let mut fx = Fixture::new();
    fx.send(Intent::Quit);
    assert!(fx.state().quit);
}

#[test]
fn quit_waits_for_a_running_save() {
    let mut fx = Fixture::new();
    fx.edit();
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let blocker = fx.state().io.submit(move |_| {
        let _ = gate_rx.recv_timeout(Duration::from_secs(10));
        Ok(())
    });
    let path = fx.path("busy.cubik");
    fx.state().inbox.push(Intent::SaveAs(Some(path)));
    fx.frame();
    assert!(fx.state().doc_job.is_some());
    fx.state().inbox.push(Intent::Quit);
    fx.frame();
    assert!(!fx.state().quit, "the save is still running");
    gate_tx.send(()).unwrap();
    drop(blocker);
    fx.settle();
    assert!(fx.state().quit, "the clean document quits after the save");
}

#[test]
fn a_queued_open_can_be_cancelled() {
    let mut fx = Fixture::new();
    let path = fx.saved_file("a.cubik");
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let _blocker = fx.state().io.submit(move |_| {
        let _ = gate_rx.recv_timeout(Duration::from_secs(10));
        Ok(())
    });
    fx.edit();
    fx.state().editor.mark_clean();
    fx.state().inbox.push(Intent::Open(Some(path)));
    fx.frame();
    assert!(fx.state().doc_job.is_some());
    fx.state().inbox.push(Intent::CancelJob);
    fx.frame();
    gate_tx.send(()).unwrap();
    fx.settle();
    assert_eq!(fx.cubes(), 2);
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .any(|text| text.starts_with("Cancelled"))
    );
}

#[test]
fn a_second_file_operation_is_refused_while_one_runs() {
    let mut fx = Fixture::new();
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let _blocker = fx.state().io.submit(move |_| {
        let _ = gate_rx.recv_timeout(Duration::from_secs(10));
        Ok(())
    });
    let (one, two) = (fx.path("one.cubik"), fx.path("two.cubik"));
    fx.state().inbox.push(Intent::SaveAs(Some(one)));
    fx.frame();
    fx.state().inbox.push(Intent::SaveAs(Some(two)));
    fx.frame();
    assert!(
        fx.toasts(Level::Warn)
            .iter()
            .any(|text| text.contains("Please wait"))
    );
    gate_tx.send(()).unwrap();
    fx.settle();
    assert!(fx.path("one.cubik").is_file());
    assert!(!fx.path("two.cubik").exists());
}

#[test]
fn a_failed_save_keeps_the_document_unsaved() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::SaveAs(Some(
        fx.path("missing_folder").join("model.cubik"),
    )));
    assert!(fx.state().editor.is_dirty());
    assert!(fx.state().editor.path().is_none());
    assert!(
        fx.toasts(Level::Error)
            .iter()
            .any(|text| text.starts_with("Cannot save"))
    );
}

#[test]
fn an_exported_copy_leaves_path_and_flag_alone() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::ExportCopy(Some(fx.path("copy"))));
    assert!(fx.path("copy.cubik").is_file());
    assert!(fx.state().editor.is_dirty());
    assert!(fx.state().editor.path().is_none());
    assert!(fx.state().settings.recent_files.list.is_empty());
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(fx.path("copy.cubik"))));
    assert_eq!(other.cubes(), 1);
}

#[test]
fn animation_data_of_an_opened_project_is_reported_where_it_is_not_written() {
    let mut fx = Fixture::new();
    let project = r#"{"meta": {"format_version": "4.10", "model_format": "free"}, "name": "m",
        "resolution": {"width": 16, "height": 16},
        "elements": [{"name": "a", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}}],
        "animation_controllers": [{"name": "walk"}, {"name": "idle"}]}"#;
    std::fs::write(fx.path("m.bbmodel"), project).unwrap();
    fx.send(Intent::Open(Some(fx.path("m.bbmodel"))));
    assert_eq!(fx.cubes(), 1);
    let dropped = |fx: &Fixture| {
        fx.toasts(Level::Warn)
            .iter()
            .filter(|text| text.contains("are not written"))
            .count()
    };
    fx.send(Intent::ExportCopy(Some(fx.path("copy.bbmodel"))));
    assert_eq!(dropped(&fx), 0, "a .bbmodel keeps them");
    fx.send(Intent::ExportCopy(Some(fx.path("copy.cubik"))));
    assert_eq!(dropped(&fx), 1);
    fx.state().toasts.clear();
    fx.send(Intent::SaveAs(Some(fx.path("m.cubik"))));
    assert_eq!(dropped(&fx), 1, "a save says it too");
    // Once per document: the next save of the same document stays quiet.
    fx.state().toasts.clear();
    fx.edit();
    fx.send(Intent::SaveAs(Some(fx.path("m2.cubik"))));
    assert!(fx.path("m2.cubik").is_file());
    assert_eq!(dropped(&fx), 0, "once per document");
}

#[test]
fn the_animations_of_the_model_are_reported_where_they_are_not_written() {
    let mut fx = Fixture::new();
    let project = r#"{"meta": {"format_version": "4.10", "model_format": "free"}, "name": "m",
        "resolution": {"width": 16, "height": 16},
        "elements": [{"name": "a", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}, "uuid": "e1"}],
        "outliner": [{"name": "bone", "origin": [0, 0, 0], "uuid": "g1", "children": ["e1"]}],
        "animations": [{"name": "walk", "length": 1, "animators": {"g1": {"name": "bone",
            "type": "bone", "keyframes": [{"channel": "rotation", "time": 0,
            "data_points": [{"x": "0", "y": "0", "z": "0"}]}]}}}, {"name": "idle"}]}"#;
    std::fs::write(fx.path("m.bbmodel"), project).unwrap();
    fx.send(Intent::Open(Some(fx.path("m.bbmodel"))));
    assert_eq!(fx.cubes(), 1);
    assert_eq!(fx.state().editor.core.doc().animations.len(), 2);
    let dropped = |fx: &Fixture| {
        fx.toasts(Level::Warn)
            .iter()
            .filter(|text| text.contains("2 animations of the project are not written"))
            .count()
    };
    fx.send(Intent::ExportCopy(Some(fx.path("copy.bbmodel"))));
    assert_eq!(dropped(&fx), 0, "a .bbmodel keeps them");
    fx.send(Intent::ExportCopy(Some(fx.path("copy.cubik"))));
    assert_eq!(dropped(&fx), 0, "a .cubik keeps them too");
    fx.send(Intent::ExportCopy(Some(fx.path("copy.obj"))));
    assert_eq!(dropped(&fx), 1, "a mesh does not");
    // A saved .cubik holds them: reopened, the model has its animations.
    fx.send(Intent::SaveAs(Some(fx.path("m.cubik"))));
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(fx.path("m.cubik"))));
    assert_eq!(other.state().editor.core.doc().animations.len(), 2);
}

#[test]
fn settings_with_the_recent_file_reach_the_disk() {
    let mut fx = Fixture::new();
    let path = fx.saved_file("recent.cubik");
    let settings_file = Dirs::under(fx.dir.path()).settings_file();
    let env = Environment {
        desktop_dir: String::new(),
        minecraft_dir: String::new(),
    };
    let loaded = Settings::load(&settings_file, &env).unwrap();
    assert_eq!(loaded.settings.recent_files.list.len(), 1);
    assert_eq!(
        loaded.settings.recent_files.list[0].path,
        path.display().to_string()
    );
}

#[test]
fn the_timer_writes_one_autosave_per_change() {
    let mut fx = Fixture::with_interval(1);
    fx.edit();
    fx.now += Duration::from_secs(61);
    fx.settle();
    let found = fx.scan();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Untitled");
    assert_eq!(found[0].reason, "timer");
    // Nothing changed: the next interval writes nothing.
    fx.now += Duration::from_secs(61);
    fx.settle();
    assert_eq!(fx.state().autosave_seq, 1);
    // A clean document is never autosaved.
    let mut clean = Fixture::with_interval(1);
    clean.now += Duration::from_secs(61);
    clean.settle();
    assert_eq!(clean.state().autosave_seq, 0);
}

#[test]
fn start_up_offers_the_autosave_of_a_crashed_session_and_recovers_it() {
    let mut crashed = Fixture::new();
    crashed.edit();
    crashed.state().request_autosave(backup::Reason::Timer);
    crashed.settle();
    let mut fx = crashed.restart();
    fx.settle();
    let Some(Dialog::Recovery { candidates }) = &fx.state().dialog else {
        panic!("no recovery dialog: {:?}", fx.state().dialog);
    };
    assert_eq!(candidates.len(), 1);
    fx.send(Intent::RecoveryAccept(0));
    assert_eq!(fx.cubes(), 1);
    assert!(fx.state().editor.is_dirty(), "recovered work is unsaved");
    // The old autosave is replaced by one of this session.
    let found = fx.scan();
    assert_eq!(found.len(), 1);
    assert!(
        found[0]
            .cubik
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&fx.state().session)
    );
}

#[test]
fn a_discarded_recovery_file_is_gone_and_a_clean_start_has_no_dialog() {
    let mut crashed = Fixture::new();
    crashed.edit();
    crashed.state().request_autosave(backup::Reason::Timer);
    crashed.settle();
    let mut fx = crashed.restart();
    fx.settle();
    fx.send(Intent::RecoveryDiscard(0));
    assert!(fx.state().dialog.is_none());
    fx.state().io.shutdown(Duration::from_secs(10));
    assert!(fx.scan().is_empty());

    let mut fresh = fx.restart();
    fresh.settle();
    assert!(fresh.state().dialog.is_none());
}

#[test]
fn a_panic_in_a_frame_restores_the_last_good_document_and_the_loop_goes_on() {
    let mut fx = Fixture::new();
    fx.edit();
    let now = fx.now + Duration::from_secs(1);
    let finished = fx.app.guarded(now, |state| {
        let add = Command::AddElement(ElementKind::Cube);
        assert!(state.editor.core.execute(add).is_done());
        panic!("command exploded");
    });
    assert!(!finished);
    assert_eq!(fx.cubes(), 1);
    assert!(!fx.state().safe_mode);
    assert!(
        fx.toasts(Level::Error)
            .iter()
            .any(|text| text.contains("restored"))
    );
    // The next frame runs and the autosave of the restored document lands in the folder.
    fx.settle();
    let found = fx.scan();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, "panic");
    fx.edit();
    assert_eq!(fx.cubes(), 2);
}

#[test]
fn repeated_panics_open_the_recovery_dialog_instead_of_looping() {
    let mut fx = Fixture::new();
    fx.edit();
    for _ in 0..3 {
        let now = fx.now;
        assert!(!fx.app.guarded(now, |_| panic!("again")));
    }
    assert!(fx.state().safe_mode);
    assert_eq!(fx.state().autosave_seq, 1, "no autosave per panic");
    fx.settle();
    let Some(Dialog::Recovery { candidates }) = &fx.state().dialog else {
        panic!("no recovery dialog: {:?}", fx.state().dialog);
    };
    assert_eq!(candidates.len(), 1);
    fx.send(Intent::RecoveryLater);
    assert!(!fx.state().safe_mode);
    assert!(fx.state().reset_guard);
}

#[test]
fn window_events_become_intents() {
    let mut fx = Fixture::new();
    // egui receives dropped files itself and the UI asks its own question; the app adds nothing.
    fx.app
        .window_event(ShellEvent::FilesDropped(vec![PathBuf::from("model.CUBIK")]));
    assert!(fx.state().inbox.is_empty());
    fx.app.window_event(ShellEvent::CloseRequested);
    assert_eq!(fx.state().inbox, [Intent::Quit]);
}

#[test]
fn commands_run_through_the_core_and_refusals_become_toasts() {
    let mut fx = Fixture::new();
    fx.send(Intent::Command(Box::new(Command::Undo)));
    assert_eq!(fx.toasts(Level::Info).len(), 1, "nothing to undo is said");
    fx.edit();
    assert_eq!(fx.cubes(), 1);
    fx.send(Intent::Command(Box::new(Command::Undo)));
    assert_eq!(fx.cubes(), 0);
    fx.send(Intent::Command(Box::new(Command::Redo)));
    assert_eq!(fx.cubes(), 1);
    assert!(fx.state().editor.is_dirty());
}

#[test]
fn a_command_that_cannot_run_yet_is_a_message_not_a_panic() {
    let mut fx = Fixture::new();
    fx.send(Intent::Command(Box::new(Command::Delete {
        targets: Vec::new(),
    })));
    assert!(!fx.toasts(Level::Info).is_empty());
    assert_eq!(fx.cubes(), 0);
}

fn recovery_candidates(fx: &mut Fixture) -> usize {
    match &fx.state().dialog {
        Some(Dialog::Recovery { candidates }) => candidates.len(),
        _ => 0,
    }
}

#[test]
fn decide_later_keeps_the_crashed_runs_autosaves_through_a_long_session() {
    let mut crashed = Fixture::new();
    crashed.autosaves(1);
    let mut fx = crashed.restart();
    fx.settle();
    assert_eq!(recovery_candidates(&mut fx), 1);
    fx.send(Intent::RecoveryLater);
    // Far more autosaves than the folder keeps, as a long session with risky ones would write.
    fx.autosaves(backup::KEEP * 2);
    assert_eq!(fx.own_autosaves(), backup::KEEP);
    let mut third = fx.sibling();
    third.settle();
    let Some(Dialog::Recovery { candidates }) = &third.state().dialog else {
        panic!(
            "the crashed run autosave is gone: {:?}",
            third.state().dialog
        );
    };
    assert_eq!(candidates.len(), 1, "the running session is not offered");
    let prefix = crashed.state().session.clone();
    let name = candidates[0].cubik.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with(&prefix), "{name}");
}

#[test]
fn two_instances_keep_each_others_autosaves_and_a_crash_makes_them_offered() {
    let mut a = Fixture::new();
    a.state().startup(None);
    a.settle();
    a.autosaves(1);
    let mut b = a.sibling();
    b.settle();
    assert!(
        b.state().dialog.is_none(),
        "A is running, its work is not offered"
    );
    b.autosaves(backup::KEEP + 3);
    assert_eq!(a.own_autosaves(), 1, "B never trims the files of A");
    assert_eq!(b.own_autosaves(), backup::KEEP);

    a.crash();
    let mut c = b.sibling();
    c.settle();
    let Some(Dialog::Recovery { candidates }) = &c.state().dialog else {
        panic!("no recovery dialog: {:?}", c.state().dialog);
    };
    assert_eq!(candidates.len(), 1);
    let name = candidates[0].cubik.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with(&a.state().session), "{name}");
}

#[test]
fn a_save_keeps_the_autosave_of_a_document_dropped_earlier() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.send(Intent::New);
    fx.send(Intent::Unsaved(UnsavedAnswer::Discard));
    assert_eq!(
        fx.own_autosaves(),
        1,
        "the dropped model keeps a safety net"
    );
    fx.saved_file("y.cubik");
    assert!(!fx.state().editor.is_dirty());
    assert_eq!(
        fx.own_autosaves(),
        1,
        "saving Y says nothing about the dropped model"
    );
    // A clean exit is the user's last word: the safety net goes.
    fx.send(Intent::Quit);
    assert!(fx.state().quit);
    fx.state().io.shutdown(Duration::from_secs(10));
    assert_eq!(fx.own_autosaves(), 0);
}

#[test]
fn a_clean_save_removes_the_autosaves_of_its_own_document() {
    let mut fx = Fixture::new();
    fx.autosaves(2);
    assert_eq!(fx.own_autosaves(), 2);
    fx.send(Intent::SaveAs(Some(fx.path("m.cubik"))));
    assert!(!fx.state().editor.is_dirty());
    fx.state().io.shutdown(Duration::from_secs(10));
    assert_eq!(fx.own_autosaves(), 0);
}

#[test]
fn quit_leaves_the_autosaves_of_other_sessions_alone() {
    let mut crashed = Fixture::new();
    crashed.autosaves(1);
    let mut fx = crashed.restart();
    fx.settle();
    fx.send(Intent::RecoveryLater);
    fx.autosaves(1);
    fx.send(Intent::Quit);
    fx.send(Intent::Unsaved(UnsavedAnswer::Discard));
    assert!(fx.state().quit);
    fx.state().io.shutdown(Duration::from_secs(10));
    assert_eq!(fx.own_autosaves(), 0);
    assert_eq!(crashed.own_autosaves(), 1);
}

#[test]
fn accepting_a_recovery_asks_about_unsaved_changes_first() {
    let mut crashed = Fixture::new();
    crashed.autosaves(1);
    let mut fx = crashed.restart();
    fx.settle();
    fx.edit();
    fx.edit();
    fx.send(Intent::RecoveryAccept(0));
    assert!(matches!(fx.state().dialog, Some(Dialog::Unsaved { .. })));
    assert_eq!(fx.cubes(), 2, "nothing was replaced yet");
    fx.send(Intent::Unsaved(UnsavedAnswer::Cancel));
    assert_eq!(recovery_candidates(&mut fx), 1, "the offer comes back");
    fx.send(Intent::RecoveryAccept(0));
    fx.send(Intent::Unsaved(UnsavedAnswer::Discard));
    assert_eq!(fx.cubes(), 1);
    assert!(fx.state().editor.is_dirty(), "recovered work is unsaved");
    // The two cubes the user dropped keep a safety net until the exit.
    assert!(fx.scan().iter().any(|found| found.reason == "risky"));
}

#[test]
fn a_changed_file_does_not_delete_the_autosave_and_is_reported() {
    let mut crashed = Fixture::new();
    let path = crashed.saved_file("m.cubik");
    crashed.edit();
    crashed.state().request_autosave(backup::Reason::Timer);
    crashed.settle();
    // Another instance, a sync client or a checkout writes the file after the autosave.
    fs::write(&path, b"written by someone else").unwrap();
    let mut fx = crashed.restart();
    fx.settle();
    let Some(Dialog::Recovery { candidates }) = &fx.state().dialog else {
        panic!("no recovery dialog: {:?}", fx.state().dialog);
    };
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].source_changed);
}

#[test]
fn save_as_without_the_extension_asks_before_replacing_the_file() {
    let mut fx = Fixture::new();
    fx.edit();
    let existing = fx.path("chair.cubik");
    fs::write(&existing, b"old model").unwrap();
    fx.send(Intent::SaveAs(Some(fx.path("chair"))));
    assert!(matches!(
        fx.state().dialog,
        Some(Dialog::Overwrite { ref path, .. }) if *path == existing
    ));
    assert_eq!(fs::read(&existing).unwrap(), b"old model");
    fx.send(Intent::Overwrite(false));
    assert!(fx.state().dialog.is_none());
    assert_eq!(fs::read(&existing).unwrap(), b"old model");
    assert!(fx.state().editor.path().is_none());

    fx.send(Intent::SaveAs(Some(fx.path("chair"))));
    fx.send(Intent::Overwrite(true));
    assert_ne!(fs::read(&existing).unwrap(), b"old model");
    assert_eq!(
        fx.state().editor.path().as_deref(),
        Some(existing.as_path())
    );

    // The same question for an exported copy, and none for a free name.
    fx.send(Intent::ExportCopy(Some(fx.path("chair"))));
    assert!(matches!(fx.state().dialog, Some(Dialog::Overwrite { .. })));
    fx.send(Intent::Overwrite(false));
    fx.send(Intent::ExportCopy(Some(fx.path("table"))));
    assert!(fx.state().dialog.is_none());
    assert!(fx.path("table.cubik").is_file());
}

#[test]
fn a_frame_that_panics_after_the_command_does_not_make_that_state_good() {
    let mut fx = Fixture::new();
    fx.edit();
    for _ in 0..2 {
        // More than the refresh interval later, the command runs and the drawing panics.
        let now = fx.now + Duration::from_secs(1);
        let add = Command::AddElement(ElementKind::Cube);
        fx.state().inbox.push(Intent::Command(Box::new(add)));
        let finished = fx.app.guarded(now, |state| {
            state.step(now);
            panic!("drawing exploded");
        });
        assert!(!finished);
        assert_eq!(fx.cubes(), 1, "the state before the command is back");
    }
    assert!(!fx.state().safe_mode);
    fx.settle();
    // One autosave for the one good state, not one per panic.
    assert_eq!(fx.state().autosave_seq, 1);
    let found = fx.scan();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, "panic");
}

#[test]
fn a_recovery_that_panics_still_leaves_an_autosave_and_ends_in_the_dialog() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.state().editor.fail_restore = true;
    let now = fx.now;
    assert!(!fx.app.guarded(now, |_| panic!("frame exploded")));
    assert!(fx.state().safe_mode);
    assert!(
        fx.toasts(Level::Error)
            .iter()
            .any(|text| text.contains("could not be restored"))
    );
    fx.settle();
    assert_eq!(recovery_candidates(&mut fx), 1);
    assert_eq!(fx.scan()[0].reason, "panic");
}

#[test]
fn a_capture_that_panics_in_the_recovery_does_not_escape() {
    let mut fx = Fixture::new();
    fx.autosaves(1);
    fx.state().editor.fail_capture = true;
    let now = fx.now;
    assert!(!fx.app.guarded(now, |_| panic!("frame exploded")));
    assert_eq!(fx.state().autosave_seq, 1, "no autosave could be taken");
    fx.settle();
    // The autosave from before stays.
    assert_eq!(fx.scan().len(), 1);
}

#[test]
fn a_json_export_asks_before_replacing_the_pictures_next_to_it() {
    let mut fx = Fixture::new();
    fx.edit();
    let json = fx.path("block.json");
    fx.send(Intent::ExportCopy(Some(json.clone())));
    assert!(fx.state().dialog.is_none());
    assert!(json.is_file());
    let pictures: Vec<PathBuf> = fs::read_dir(fx.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect();
    assert!(!pictures.is_empty(), "the export wrote no picture");

    // Someone's own picture with the same name, and the JSON gone: the picture is asked about.
    fs::remove_file(&json).unwrap();
    for picture in &pictures {
        fs::write(picture, b"mine").unwrap();
    }
    fx.send(Intent::ExportCopy(Some(json.clone())));
    let Some(Dialog::Overwrite { also, .. }) = &fx.state().dialog else {
        panic!("no question: {:?}", fx.state().dialog);
    };
    let mut listed = also.clone();
    listed.sort();
    let mut expected = pictures.clone();
    expected.sort();
    assert_eq!(listed, expected);
    assert!(!json.exists());
    assert!(pictures.iter().all(|p| fs::read(p).unwrap() == b"mine"));

    fx.send(Intent::Overwrite(false));
    assert!(fx.state().dialog.is_none());
    assert!(!json.exists());
    assert!(pictures.iter().all(|p| fs::read(p).unwrap() == b"mine"));

    fx.send(Intent::ExportCopy(Some(json.clone())));
    fx.send(Intent::Overwrite(true));
    assert!(json.is_file());
    assert!(pictures.iter().all(|p| fs::read(p).unwrap() != b"mine"));
}

#[test]
fn a_pack_folder_export_asks_before_replacing_files_in_it() {
    use crate::flows::ExportPlan;

    let mut fx = Fixture::new();
    fx.edit();
    let folder = fx.path("pack");
    let plan = ExportPlan {
        folder: true,
        ..ExportPlan::latest()
    };
    fx.state()
        .start_export_with(folder.clone(), Some(plan), false);
    fx.settle();
    assert!(fx.state().dialog.is_none());
    let meta = folder.join("pack.mcmeta");
    assert!(meta.is_file());

    fs::write(&meta, b"my pack").unwrap();
    fx.state()
        .start_export_with(folder.clone(), Some(plan), false);
    fx.settle();
    let Some(Dialog::Overwrite { also, .. }) = &fx.state().dialog else {
        panic!("no question: {:?}", fx.state().dialog);
    };
    assert!(also.contains(&meta));
    fx.send(Intent::Overwrite(false));
    assert_eq!(fs::read(&meta).unwrap(), b"my pack");
    assert_eq!(
        fx.state().export_result.take(),
        Some(cuboid_ui::ExportOutcome::Declined)
    );

    fx.state().start_export_with(folder, Some(plan), false);
    fx.settle();
    fx.send(Intent::Overwrite(true));
    assert_ne!(fs::read(&meta).unwrap(), b"my pack");
}
