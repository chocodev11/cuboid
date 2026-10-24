//! The state of the application and the logic of one frame: jobs polled, intents handled, timers,
//! toasts and the recovery after a panic. The file flows are in [`crate::flows`].

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use cuboid_model::image::Rev;
use cuboid_settings::Settings;

use cuboid_core::{CmdOutcome, Command};

use crate::backup::{self, Candidate, Origin, Reason, SessionLock};
use crate::crash::{self, Panicked};
use crate::editor::{Editor, SaveInput};
use crate::flows::{DocJob, PickPurpose};
use crate::io_thread::IoThread;
use crate::jobs::{JobError, JobHandle};
use crate::library::{LibraryService, Prepared};
use crate::platform::Dirs;

/// Toasts shown at once; the oldest goes first.
const MAX_TOASTS: usize = 5;
const TOAST_TIME: Duration = Duration::from_secs(5);
const ERROR_TOAST_TIME: Duration = Duration::from_secs(12);
/// How often a frame runs while a job is in flight, so that its end shows quickly.
pub const BUSY_TICK: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub level: Level,
    pub text: String,
    pub expires: Instant,
}

/// A modal question. Everything else is a toast.
#[derive(Debug, Clone)]
pub enum Dialog {
    /// The document has unsaved changes and an action would drop them.
    Unsaved { name: String },
    /// Autosaves found after a crash, newest first.
    Recovery { candidates: Vec<Candidate> },
    /// Files would be replaced: the name the file dialog returned without the extension is taken
    /// (`also` is empty), or an export finds files next to its target (`also` lists the ones that
    /// exist, the target among them if it does). `plan` is the export dialog's, if it asked.
    Overwrite {
        path: PathBuf,
        also: Vec<PathBuf>,
        purpose: PickPurpose,
        plan: Option<crate::flows::ExportPlan>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsavedAnswer {
    Save,
    Discard,
    Cancel,
}

/// What the UI asks for. The UI never changes the state itself (architecture 14.6).
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// An edit of the model, undo and redo included (the UI sends commands itself; tests use
    /// this to drive the core).
    #[cfg(test)]
    Command(Box<Command>),
    /// File > New with the unsaved-changes question of the app.
    #[cfg(test)]
    New,
    /// `None` asks for a path with the system dialog.
    Open(Option<PathBuf>),
    Save,
    SaveAs(Option<PathBuf>),
    /// Writes a copy any Cubik reads; the document, its path and its unsaved flag stay.
    ExportCopy(Option<PathBuf>),
    Quit,
    Unsaved(UnsavedAnswer),
    /// The answer to [`Dialog::Overwrite`]: true replaces the file.
    Overwrite(bool),
    RecoveryAccept(usize),
    RecoveryDiscard(usize),
    /// Closes the recovery dialog and keeps the files for the next start.
    RecoveryLater,
    /// Stops the running open, save or export if it has not passed the point of no return.
    CancelJob,
}

/// An action that waits for the answer to the unsaved-changes question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    New,
    /// The system dialog for a file to open.
    PickOpen,
    /// A new model made from a template of the start screen.
    Template(cuboid_ui::Template),
    Open(PathBuf),
    /// Opens an autosave in place of the document.
    Recover(Box<Candidate>),
    Quit,
}

/// The window the shell last reported, kept for the settings.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowGeometry {
    pub size: Option<(u32, u32)>,
    pub position: Option<(i32, i32)>,
}

pub struct Picker {
    pub rx: Receiver<Option<PathBuf>>,
    pub purpose: PickPurpose,
}

pub struct State {
    pub dirs: Dirs,
    pub settings: Settings,
    pub editor: Editor,
    pub io: IoThread,
    /// The asset library, on a file thread of its own.
    pub library: LibraryService,
    pub session: String,
    /// Held from start-up until the program ends; tells other instances the session is alive.
    pub session_lock: Option<SessionLock>,
    pub now: Instant,
    pub inbox: Vec<Intent>,
    pub toasts: Vec<Toast>,
    pub dialog: Option<Dialog>,
    pub doc_job: Option<DocJob>,
    pub autosaves: Vec<JobHandle<PathBuf>>,
    pub settings_job: Option<JobHandle<()>>,
    pub scan_job: Option<JobHandle<Vec<Candidate>>>,
    pub picker: Option<Picker>,
    pub pending: Option<Pending>,
    /// Runs after the save that the unsaved-changes question started.
    pub after_save: Option<Pending>,
    /// `pending` waits for the running job to end.
    pub when_idle: bool,
    /// Autosaves to offer again once the question or the open that displaced them is over.
    pub recovery_rest: Vec<Candidate>,
    /// Documents the user dropped with "Don't save"; a clean exit removes their autosaves.
    pub discarded: Vec<u64>,
    pub next_autosave: Instant,
    /// The revision of the document whose content the newest autosave holds.
    pub last_autosaved: Option<Rev>,
    /// The last good state an autosave of a panic already holds.
    last_autosaved_good: Option<Rev>,
    pub autosave_seq: u32,
    pub autosave_failing: bool,
    pub settings_dirty: bool,
    /// Repeated panics: only the recovery dialog runs until the user decides.
    pub safe_mode: bool,
    /// Scan for autosaves as soon as the autosave of the panic is on disk.
    pub scan_when_idle: bool,
    pub reset_guard: bool,
    /// The shell leaves its event loop after this frame.
    pub quit: bool,
    pub window: WindowGeometry,
    /// How the last export of the export dialog ended; the frontend hands it to the dialog.
    pub export_result: Option<cuboid_ui::ExportOutcome>,
    /// The folder the user picked for the export or render dialog.
    pub folder_answer: Option<PathBuf>,
}

impl State {
    pub fn new(
        dirs: Dirs,
        settings: Settings,
        io: IoThread,
        code_page: u16,
        now: Instant,
    ) -> State {
        let mut state = State {
            library: LibraryService::new(&dirs),
            dirs,
            settings,
            editor: Editor::new(code_page),
            io,
            session: backup::session_id(),
            session_lock: None,
            now,
            inbox: Vec::new(),
            toasts: Vec::new(),
            dialog: None,
            doc_job: None,
            autosaves: Vec::new(),
            settings_job: None,
            scan_job: None,
            picker: None,
            pending: None,
            after_save: None,
            when_idle: false,
            recovery_rest: Vec::new(),
            discarded: Vec::new(),
            next_autosave: now,
            last_autosaved: None,
            last_autosaved_good: None,
            autosave_seq: 0,
            autosave_failing: false,
            settings_dirty: false,
            safe_mode: false,
            scan_when_idle: false,
            reset_guard: false,
            quit: false,
            window: WindowGeometry::default(),
            export_result: None,
            folder_answer: None,
        };
        state.next_autosave = now + state.autosave_interval();
        state
    }

    /// Start-up work that does not block the window: look for autosaves, open the file named on
    /// the command line.
    pub fn startup(&mut self, file: Option<PathBuf>) {
        match SessionLock::acquire(&self.dirs.recovery(), &self.session) {
            Ok(lock) => self.session_lock = Some(lock),
            Err(error) => log::warn!("cannot lock the session: {error}"),
        }
        self.start_scan();
        if let Some(path) = file {
            self.inbox.push(Intent::Open(Some(path)));
        }
    }

    pub fn start_scan(&mut self) {
        let dir = self.dirs.recovery();
        let session = self.session.clone();
        self.scan_job = Some(self.io.submit(move |_| Ok(backup::scan(&dir, &session))));
    }

    fn autosave_interval(&self) -> Duration {
        let minutes = self.settings.backup_every_minutes.clamp(1, 120);
        Duration::from_secs(u64::from(minutes.unsigned_abs()) * 60)
    }

    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        let time = if level == Level::Error {
            ERROR_TOAST_TIME
        } else {
            TOAST_TIME
        };
        let expires = self.now + time;
        if let Some(same) = self.toasts.iter_mut().find(|toast| toast.text == text) {
            same.expires = expires;
            return;
        }
        if self.toasts.len() == MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast {
            level,
            text,
            expires,
        });
    }

    /// The logic of one frame, run inside the panic boundary.
    pub fn step(&mut self, now: Instant) {
        self.now = now;
        self.toasts.retain(|toast| toast.expires > now);
        self.poll_jobs();
        for intent in std::mem::take(&mut self.inbox) {
            self.dispatch(intent);
        }
        // The queue is in order, so the scan sees the autosave of the panic.
        if self.scan_when_idle && self.scan_job.is_none() {
            self.scan_when_idle = false;
            self.start_scan();
        }
        if !self.safe_mode {
            self.autosave_on_timer();
        }
        self.flush_settings();
    }

    fn poll_jobs(&mut self) {
        self.poll_doc_job();
        self.poll_autosaves();
        self.poll_settings_job();
        self.poll_scan_job();
        self.poll_picker();
        self.poll_library();
    }

    /// The library follows the folder of the open document (its project library) and reports
    /// to the user.
    fn poll_library(&mut self) {
        let path = self.editor.path();
        let folder = path.as_deref().and_then(std::path::Path::parent);
        for (level, text) in self.library.step(folder) {
            self.toast(level, text);
        }
        for prepared in self.library.take_prepared() {
            match prepared {
                Ok(prepared) => self.add_dropped_texture(prepared),
                Err(error) => self.toast(Level::Error, format!("Cannot add the texture: {error}")),
            }
        }
    }

    /// Adds a library texture as one undoable step.
    fn add_dropped_texture(&mut self, prepared: Prepared) {
        let outcome = self.editor.core.execute(Command::AddTexture {
            label: prepared.stem,
            width: prepared.width,
            height: prepared.height,
            rgba: prepared.rgba,
        });
        match outcome {
            CmdOutcome::Done(_) => {}
            CmdOutcome::Failed(why) => return self.toast(Level::Error, why),
            CmdOutcome::Refused(why) => {
                return self.toast(Level::Warn, format!("Cannot add the texture: {why}"));
            }
            CmdOutcome::Unchanged | CmdOutcome::NotYet(_) => return,
        }
        let label = self
            .editor
            .core
            .doc()
            .textures
            .entries
            .last()
            .map_or_else(String::new, |entry| entry.label.to_string());
        self.toast(Level::Info, format!("Added the texture {label}."));
    }

    fn poll_autosaves(&mut self) {
        let mut finished = Vec::new();
        self.autosaves.retain_mut(|handle| match handle.poll() {
            None => true,
            Some(result) => {
                finished.push(result);
                false
            }
        });
        for result in finished {
            match result {
                Ok(path) => {
                    log::debug!("autosave {}", path.display());
                    self.autosave_failing = false;
                }
                Err(error) => {
                    log::warn!("autosave failed: {error}");
                    if !self.autosave_failing {
                        self.autosave_failing = true;
                        self.toast(
                            Level::Warn,
                            format!("Autosave failed: {error}. Save your work to a file."),
                        );
                    }
                }
            }
        }
    }

    fn poll_settings_job(&mut self) {
        let Some(result) = self.settings_job.as_mut().and_then(JobHandle::poll) else {
            return;
        };
        self.settings_job = None;
        if let Err(error) = result {
            log::warn!("cannot write the settings: {error}");
            self.toast(Level::Warn, format!("Cannot save the settings: {error}"));
        }
    }

    fn poll_scan_job(&mut self) {
        let Some(result) = self.scan_job.as_mut().and_then(JobHandle::poll) else {
            return;
        };
        self.scan_job = None;
        match result {
            Ok(candidates) => self.offer_recovery(candidates),
            Err(error) => {
                log::warn!("cannot look for autosaves: {error}");
                self.leave_safe_mode();
            }
        }
    }

    /// Opens the recovery dialog for what the scan found.
    fn offer_recovery(&mut self, candidates: Vec<Candidate>) {
        if candidates.is_empty() {
            if self.safe_mode {
                self.toast(Level::Warn, "No autosave was found.");
                self.leave_safe_mode();
            }
            return;
        }
        if self.dialog.is_some() {
            log::info!("{} autosaves stay for the next start", candidates.len());
            return;
        }
        log::info!("{} autosaves found", candidates.len());
        self.dialog = Some(Dialog::Recovery { candidates });
    }

    pub fn leave_safe_mode(&mut self) {
        if self.safe_mode {
            self.safe_mode = false;
            self.reset_guard = true;
        }
    }

    /// Writes the settings when something changed and no write is running.
    fn flush_settings(&mut self) {
        if self.settings_dirty && self.settings_job.is_none() {
            self.settings_dirty = false;
            self.save_settings();
        }
    }

    /// Queues a write of the settings behind everything already queued.
    pub fn save_settings(&mut self) {
        let settings = self.settings.clone();
        let path = self.dirs.settings_file();
        self.settings_job = Some(self.io.submit(move |_| {
            settings
                .save(&path)
                .map_err(|error| JobError::Failed(error.to_string()))
        }));
    }

    fn autosave_on_timer(&mut self) {
        if self.now < self.next_autosave {
            return;
        }
        self.next_autosave = self.now + self.autosave_interval();
        // Zero turns the timer off; the risky autosaves before a replace stay.
        if self.settings.backup_every_minutes <= 0 {
            return;
        }
        let unchanged = self.last_autosaved == Some(self.editor.doc().revision);
        if self.autosaves.is_empty() && self.editor.is_dirty() && !unchanged {
            self.request_autosave(Reason::Timer);
        }
    }

    /// Takes the document as it is now and writes it to the recovery folder on the I/O thread.
    /// Does nothing for a document without unsaved changes. Call it before an operation that
    /// may fail or replace the document.
    pub fn request_autosave(&mut self, reason: Reason) {
        self.autosave_and_remove(reason, Vec::new());
    }

    /// An autosave that removes `recovered` once it is on disk.
    pub fn autosave_and_remove(&mut self, reason: Reason, recovered: Vec<Candidate>) {
        if !self.editor.is_dirty() {
            return;
        }
        let input = self.editor.save_input(None);
        self.queue_autosave(input, reason, recovered);
    }

    /// Writes `input` to the recovery folder on the I/O thread, as an autosave of the document
    /// that is open now.
    fn queue_autosave(&mut self, input: SaveInput, reason: Reason, recovered: Vec<Candidate>) {
        self.last_autosaved = Some(input.revision);
        self.autosave_seq += 1;
        let dir = self.dirs.recovery();
        let session = self.session.clone();
        let seq = self.autosave_seq;
        let doc_id = self.editor.doc_id();
        let source = self.editor.path();
        let stamp = self.editor.source_stamp;
        let title = self.editor.name();
        self.autosaves.push(self.io.submit(move |ctx| {
            ctx.progress(0.1);
            let origin = Origin {
                session: &session,
                seq,
                doc_id,
                source: source.as_deref(),
                stamp,
                title: &title,
                reason,
            };
            let path = backup::write(&dir, &origin, &input)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            for candidate in &recovered {
                backup::discard(candidate);
            }
            Ok(path)
        }));
    }

    /// After a panic in the frame logic: the document goes back to the last good state, that state
    /// is kept in the recovery folder and the user is told. Repeated panics, or a panic of the
    /// recovery itself, stop the editing and offer the autosaves instead of looping.
    ///
    /// Every step runs under its own `catch_unwind` and none of them reads the live document
    /// again, so a state that panics cannot make the recovery panic the same way.
    pub fn recover_from_panic(&mut self, panicked: &Panicked) {
        log::error!("panic in the frame logic: {}", panicked.message);
        let restored = crash::catch("restoring the last good state", || {
            self.editor.restore_last_good();
        })
        .is_some();
        // In safe mode the frames repeat; one autosave per good state is enough.
        let kept = !self.safe_mode && self.autosave_last_good();
        let text = if restored {
            let saved = if kept {
                " and is kept in the recovery folder"
            } else {
                ""
            };
            format!(
                "Something went wrong and the last action was dropped. Your model was restored{saved}. \
                 A report is in {}.",
                self.dirs.logs().display()
            )
        } else {
            format!(
                "Something went wrong and the model could not be restored. Your work is in the \
                 recovery folder. A report is in {}.",
                self.dirs.logs().display()
            )
        };
        self.toast(Level::Error, text);
        if (panicked.repeated || !restored) && !self.safe_mode {
            self.safe_mode = true;
            self.scan_when_idle = true;
            self.dialog = None;
            self.pending = None;
            self.after_save = None;
            self.recovery_rest.clear();
        }
    }

    /// Queues an autosave of the last good state unless one holds it already. True when the
    /// recovery folder has that state after the queue ran.
    fn autosave_last_good(&mut self) -> bool {
        let good = self.editor.good_revision();
        if self.last_autosaved_good == Some(good) {
            return true;
        }
        let Some(input) = crash::catch("capturing the last good state", || {
            self.editor.good_save_input()
        }) else {
            return false;
        };
        self.last_autosaved_good = Some(good);
        self.queue_autosave(input, Reason::Panic, Vec::new());
        // The restored document holds the same content; the timer has nothing new to write.
        self.last_autosaved = Some(self.editor.doc().revision);
        true
    }

    /// The graphics device was lost: keep the work and ask for a restart.
    pub fn on_device_lost(&mut self) {
        self.request_autosave(Reason::Risky);
        self.toast(
            Level::Error,
            "The graphics device was lost. Your model was saved to the recovery folder; \
             please restart cuboid.",
        );
    }

    /// True while a background job runs.
    pub fn is_busy(&self) -> bool {
        self.doc_job.is_some()
            || !self.autosaves.is_empty()
            || self.settings_job.is_some()
            || self.scan_job.is_some()
            || self.picker.is_some()
            || self.library.is_busy()
    }

    /// When the frame logic must run again without input.
    pub fn next_wake(&self, now: Instant) -> Instant {
        let mut wake = self.next_autosave;
        if self.is_busy() {
            wake = wake.min(now + BUSY_TICK);
        }
        if let Some(soonest) = self.toasts.iter().map(|toast| toast.expires).min() {
            wake = wake.min(soonest);
        }
        wake
    }
}
