//! New, open, save, save as, export of a copy, close with the unsaved-changes question, recent
//! files and recovery (spec: app 2, 11, 12; roadmap 7). Every error becomes a toast.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use cuboid_core::{CmdOutcome, Command, ElementKind};
use cuboid_fmt_cubik::cub::{self, DiskFiles, Notice};
use cuboid_fmt_cubik::{CubikError, Limits, load_path};
use cuboid_model::image::Rev;
use cuboid_model::{ApplyMode, ApplySource, Document, EditCtx, EditEnv, Selection, apply_document};
use cuboid_num::EngineRng;
use cuboid_ui::Template;

use crate::backup::{self, Candidate, Reason, Stamp};
use crate::editor::empty_document;
use crate::jobs::{JobCtx, JobError, JobHandle};
use crate::state::{Dialog, Intent, Level, Pending, Picker, State, UnsavedAnswer};

/// What the file dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickPurpose {
    Open,
    SaveAs,
    Export,
}

/// A file read and applied to a fresh document, ready to replace the open one.
pub struct Opened {
    pub doc: Document,
    /// The path the document is saved to; none for an import.
    pub doc_path: Option<PathBuf>,
    /// The file at `doc_path` as it was before it was read.
    pub stamp: Option<Stamp>,
    pub warnings: Vec<String>,
}

pub enum DocOutput {
    Opened(Box<Opened>),
    Saved { revision: Rev, stamp: Option<Stamp> },
    Exported,
}

pub enum DocJobKind {
    Open {
        path: PathBuf,
        /// The autosave being recovered.
        recovery: Option<Box<Candidate>>,
    },
    Save {
        path: PathBuf,
    },
    Export {
        path: PathBuf,
    },
}

/// The one open, save or export that runs at a time.
pub struct DocJob {
    pub title: String,
    pub kind: DocJobKind,
    pub handle: JobHandle<DocOutput>,
}

fn io_failure(error: &io::Error) -> JobError {
    if error.kind() == io::ErrorKind::NotFound {
        JobError::NotFound
    } else {
        JobError::Failed(error.to_string())
    }
}

fn cubik_failure(error: CubikError) -> JobError {
    match error {
        CubikError::Io(error) => io_failure(&error),
        other => JobError::Failed(other.to_string()),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Appends `.cubik` unless the name ends in it.
pub fn ensure_cubik_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cubik"));
    if has {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".cubik");
    PathBuf::from(name)
}

/// Reads a `.cubik` or `.cub` file into a fresh document. `doc_path` becomes the document's
/// own path (`None` for an import, which the user saves under a new name).
pub fn open_file(
    path: &Path,
    doc_path: Option<&Path>,
    env: &EditEnv,
    ctx: &JobCtx,
) -> Result<Opened, JobError> {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    ctx.progress(0.05);
    let stamp = doc_path.and_then(Stamp::of);
    let (loaded, mut warnings) = match extension.as_str() {
        "cubik" => {
            let loaded = load_path(path, &Limits::default()).map_err(cubik_failure)?;
            (loaded.doc, loaded.warnings)
        }
        "cub" => {
            let bytes = fs::read(path).map_err(|error| io_failure(&error))?;
            let imported = cub::import(&bytes, path, &DiskFiles)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            let notes = imported.notices.iter().map(|notice| match notice {
                Notice::Inherited { parent, .. } => {
                    format!("the model inherits from `{parent}`, which is not loaded")
                }
                Notice::NoTexture => "the file names no texture folder".to_owned(),
                Notice::TextureRefused { key, path, .. } => {
                    format!("the texture `{key}` was not read from {path}")
                }
            });
            (imported.doc, notes.collect())
        }
        _ => {
            return Err(JobError::Failed(
                "only .cubik and .cub files can be opened".to_owned(),
            ));
        }
    };
    ctx.check()?;
    ctx.progress(0.7);
    let mut doc = empty_document();
    let mut sel = Selection::default();
    let mut rng = EngineRng::seeded(1);
    let report = apply_document(
        &mut EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env,
            rng: &mut rng,
        },
        ApplySource::Loaded(Box::new(loaded)),
        ApplyMode::Open,
    )
    .map_err(|error| JobError::Failed(format!("invalid model: {error}")))?;
    warnings.extend(report.warnings);
    // The path in the file is where its author saved it.
    let text = doc_path.map_or_else(String::new, |path| path.display().to_string());
    std::sync::Arc::make_mut(&mut doc.props).save_path = text;
    ctx.progress(1.0);
    Ok(Opened {
        doc,
        doc_path: doc_path.map(Path::to_owned),
        stamp,
        warnings,
    })
}

impl State {
    pub fn dispatch(&mut self, intent: Intent) {
        match intent {
            #[cfg(test)]
            Intent::Command(command) => self.run_command(*command),
            #[cfg(test)]
            Intent::New => self.request(Pending::New),
            Intent::Open(Some(path)) => self.request(Pending::Open(path)),
            Intent::Open(None) => self.pick(PickPurpose::Open),
            Intent::Quit => self.request(Pending::Quit),
            Intent::Save => match self.editor.path() {
                Some(path) => self.start_save(path),
                None => self.pick(PickPurpose::SaveAs),
            },
            Intent::SaveAs(Some(path)) => self.confirm_target(path, PickPurpose::SaveAs),
            Intent::SaveAs(None) => self.pick(PickPurpose::SaveAs),
            Intent::ExportCopy(Some(path)) => self.confirm_target(path, PickPurpose::Export),
            Intent::ExportCopy(None) => self.pick(PickPurpose::Export),
            Intent::Unsaved(answer) => self.answer_unsaved(answer),
            Intent::Overwrite(replace) => self.answer_overwrite(replace),
            Intent::RecoveryAccept(index) => self.accept_recovery(index),
            Intent::RecoveryDiscard(index) => self.discard_recovery(index),
            Intent::RecoveryLater => {
                self.dialog = None;
                self.leave_safe_mode();
            }
            Intent::CancelJob => {
                if let Some(job) = &self.doc_job {
                    job.handle.cancel();
                }
            }
        }
    }

    /// Runs an edit and tells the user when it was refused or failed.
    pub fn run_command(&mut self, command: Command) {
        let outcome = self.editor.core.execute(command);
        self.report(outcome);
    }

    /// As [`State::run_command`] for a continuous edit: commands of one `gesture` share an undo
    /// step.
    pub fn run_merged(&mut self, command: Command, gesture: u64) {
        let outcome = self.editor.core.execute_merged(command, gesture);
        self.report(outcome);
    }

    fn report(&mut self, outcome: CmdOutcome) {
        match outcome {
            CmdOutcome::Done(_) | CmdOutcome::Unchanged => {}
            CmdOutcome::Refused(why) => self.toast(Level::Info, why.to_string()),
            CmdOutcome::NotYet(what) => {
                self.toast(Level::Info, format!("Not available yet: {what}"));
            }
            CmdOutcome::Failed(why) => self.toast(Level::Error, why),
        }
    }

    fn busy_toast(&mut self) {
        let what = self
            .doc_job
            .as_ref()
            .map_or_else(String::new, |job| job.title.clone());
        self.toast(Level::Warn, format!("Please wait, {what} is not finished."));
    }

    /// An action that replaces or ends the document: asks first when there are unsaved changes.
    fn request(&mut self, action: Pending) {
        if self.doc_job.is_some() {
            if action == Pending::Quit {
                // Leaving: stop an open, wait for a save, then ask again.
                if let Some(DocJob {
                    kind: DocJobKind::Open { .. },
                    handle,
                    ..
                }) = &self.doc_job
                {
                    handle.cancel();
                }
                self.pending = Some(action);
                self.when_idle = true;
            } else {
                self.busy_toast();
            }
            return;
        }
        if matches!(self.dialog, Some(Dialog::Unsaved { .. })) {
            return;
        }
        if self.editor.is_dirty() {
            // The autosaves on offer come back when the question is answered.
            if let Some(Dialog::Recovery { candidates }) = self.dialog.take() {
                self.recovery_rest.extend(candidates);
            }
            self.pending = Some(action);
            self.dialog = Some(Dialog::Unsaved {
                name: self.editor.name(),
            });
            return;
        }
        self.run(action);
    }

    fn run(&mut self, action: Pending) {
        match action {
            Pending::New => {
                let doc = self.editor.new_document();
                self.editor.replace(doc, None, false);
                self.last_autosaved = None;
                self.show_recovery_rest();
            }
            Pending::PickOpen => self.pick(PickPurpose::Open),
            Pending::Template(template) => {
                self.run(Pending::New);
                let command = match template {
                    Template::Block => Command::AddElement(ElementKind::Cube),
                    Template::Item => Command::AddElement(ElementKind::PlaneZ),
                    Template::Voxel { resolution } => Command::SetupVoxels { resolution },
                };
                self.run_command(command);
                // A template is a starting point, not work to save.
                self.editor.mark_clean();
            }
            Pending::Open(path) => self.start_open(path, None),
            Pending::Recover(chosen) => {
                self.leave_safe_mode();
                let path = chosen.cubik.clone();
                self.start_open(path, Some(*chosen));
            }
            Pending::Quit => self.finish_quit(),
        }
    }

    /// Opens the recovery dialog again for the autosaves that waited behind a question or an open.
    fn show_recovery_rest(&mut self) {
        if self.recovery_rest.is_empty() || self.dialog.is_some() {
            return;
        }
        let candidates = std::mem::take(&mut self.recovery_rest);
        self.dialog = Some(Dialog::Recovery { candidates });
    }

    /// The user dropped the unsaved work on purpose, but keep it until the next save or a clean
    /// exit; leaving the program drops it for good.
    fn discard_unsaved(&mut self, action: &Pending) {
        if *action != Pending::Quit {
            self.request_autosave(Reason::Risky);
        }
        self.discarded.push(self.editor.doc_id());
        // An open that fails must not leave the old document marked clean.
        if !matches!(action, Pending::Open(_) | Pending::Recover(_)) {
            self.editor.mark_clean();
        }
    }

    /// An action the UI already asked about (its own unsaved-changes question): unsaved work in
    /// the document counts as given up.
    pub fn confirmed(&mut self, action: Pending) {
        if self.doc_job.is_some() || matches!(self.dialog, Some(Dialog::Unsaved { .. })) {
            self.request(action);
            return;
        }
        if self.editor.is_dirty() {
            self.discard_unsaved(&action);
        }
        self.run(action);
    }

    /// The answer "Save" of the question of the UI: save, then run the action.
    pub fn save_then(&mut self, action: Pending) {
        self.after_save = Some(action);
        self.dispatch(Intent::Save);
    }

    fn answer_unsaved(&mut self, answer: UnsavedAnswer) {
        if !matches!(self.dialog, Some(Dialog::Unsaved { .. })) {
            return;
        }
        self.dialog = None;
        let Some(action) = self.pending.take() else {
            return;
        };
        match answer {
            UnsavedAnswer::Cancel => {
                if let Pending::Recover(chosen) = action {
                    self.recovery_rest.insert(0, *chosen);
                }
                self.show_recovery_rest();
            }
            UnsavedAnswer::Discard => {
                self.discard_unsaved(&action);
                self.run(action);
            }
            UnsavedAnswer::Save => {
                self.after_save = Some(action);
                self.dispatch(Intent::Save);
            }
        }
    }

    /// Leaving the program: the settings are written, the autosaves of the open document and of the
    /// documents dropped on purpose are removed (the work is saved or dropped) and the shell is
    /// told to stop. Autosaves of other documents, sessions and instances stay.
    fn finish_quit(&mut self) {
        // The queued autosaves would only be removed again.
        for autosave in &self.autosaves {
            autosave.cancel();
        }
        let settings = &mut self.settings;
        if let Some((x, y)) = self.window.position {
            settings.window_x = x;
            settings.window_y = y;
        }
        if let Some((width, height)) = self.window.size {
            settings.cuboid.window_width = width;
            settings.cuboid.window_height = height;
        }
        self.save_settings();
        let dir = self.dirs.recovery();
        let session = self.session.clone();
        let mut documents = std::mem::take(&mut self.discarded);
        documents.push(self.editor.doc_id());
        // The queue is in order: the autosave that already runs is written first and then removed.
        drop(self.io.submit(move |_| {
            backup::purge_docs(&dir, &session, &documents);
            Ok(())
        }));
        self.quit = true;
        log::info!("quit");
    }

    /// The system dialog asked about overwriting the name the user typed. When the extension
    /// changes the name and the new name is taken, the user is asked again.
    fn confirm_target(&mut self, path: PathBuf, purpose: PickPurpose) {
        let fixed = ensure_cubik_extension(path.clone());
        if fixed != path && fixed.exists() {
            self.dialog = Some(Dialog::Overwrite {
                path: fixed,
                purpose,
            });
            return;
        }
        self.write_target(fixed, purpose);
    }

    fn write_target(&mut self, path: PathBuf, purpose: PickPurpose) {
        match purpose {
            PickPurpose::SaveAs => self.start_save(path),
            PickPurpose::Export => self.start_export(path),
            PickPurpose::Open => {}
        }
    }

    fn answer_overwrite(&mut self, replace: bool) {
        let Some(Dialog::Overwrite { path, purpose }) = &self.dialog else {
            return;
        };
        let (path, purpose) = (path.clone(), *purpose);
        self.dialog = None;
        if replace {
            self.write_target(path, purpose);
        } else {
            self.after_save = None;
        }
    }

    fn pick(&mut self, purpose: PickPurpose) {
        if self.picker.is_some() {
            return;
        }
        let start = self
            .editor
            .path()
            .and_then(|path| path.parent().map(Path::to_owned));
        let name = format!("{}.cubik", self.editor.name());
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("cuboid-dialog".to_owned())
            .spawn(move || {
                let _ = tx.send(pollster::block_on(ask_for_path(purpose, start, &name)));
            });
        match spawned {
            Ok(_) => self.picker = Some(Picker { rx, purpose }),
            Err(error) => {
                log::error!("cannot start the file dialog: {error}");
                self.after_save = None;
                self.toast(Level::Error, "Cannot open the file dialog.");
            }
        }
    }

    pub fn poll_picker(&mut self) {
        let Some(picker) = &self.picker else { return };
        let answer = match picker.rx.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        let purpose = picker.purpose;
        self.picker = None;
        let Some(path) = answer else {
            // Cancelled: whatever waited for the save does not happen.
            self.after_save = None;
            return;
        };
        // The UI asked about unsaved changes before it started the open dialog.
        match purpose {
            PickPurpose::Open => self.confirmed(Pending::Open(path)),
            PickPurpose::SaveAs => self.dispatch(Intent::SaveAs(Some(path))),
            PickPurpose::Export => self.dispatch(Intent::ExportCopy(Some(path))),
        }
    }

    pub fn start_open(&mut self, path: PathBuf, recovery: Option<Candidate>) {
        if self.doc_job.is_some() {
            self.busy_toast();
            return;
        }
        let imported = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cub"));
        let doc_path = match &recovery {
            Some(candidate) => candidate.source.clone(),
            None => (!imported).then(|| path.clone()),
        };
        let env = self.editor.core.env_mut().clone();
        let source = path.clone();
        let handle = self.io.submit(move |ctx| {
            open_file(&source, doc_path.as_deref(), &env, ctx)
                .map(|opened| DocOutput::Opened(Box::new(opened)))
        });
        self.doc_job = Some(DocJob {
            title: format!("Opening {}", file_name(&path)),
            kind: DocJobKind::Open {
                path,
                recovery: recovery.map(Box::new),
            },
            handle,
        });
    }

    fn start_save(&mut self, path: PathBuf) {
        if self.doc_job.is_some() {
            self.after_save = None;
            self.busy_toast();
            return;
        }
        let input = self.editor.save_input(Some(&path));
        let revision = input.revision;
        let target = path.clone();
        let handle = self.io.submit(move |ctx| {
            ctx.progress(0.1);
            ctx.check()?;
            input.write(&target, true).map_err(cubik_failure)?;
            Ok(DocOutput::Saved {
                revision,
                stamp: Stamp::of(&target),
            })
        });
        self.doc_job = Some(DocJob {
            title: format!("Saving {}", file_name(&path)),
            kind: DocJobKind::Save { path },
            handle,
        });
    }

    fn start_export(&mut self, path: PathBuf) {
        if self.doc_job.is_some() {
            self.busy_toast();
            return;
        }
        let input = self.editor.save_input(None);
        let target = path.clone();
        let handle = self.io.submit(move |ctx| {
            ctx.progress(0.1);
            ctx.check()?;
            input.write_copy(&target).map_err(cubik_failure)?;
            Ok(DocOutput::Exported)
        });
        self.doc_job = Some(DocJob {
            title: format!("Exporting {}", file_name(&path)),
            kind: DocJobKind::Export { path },
            handle,
        });
    }

    pub fn poll_doc_job(&mut self) {
        let Some(result) = self.doc_job.as_mut().and_then(|job| job.handle.poll()) else {
            return;
        };
        let Some(job) = self.doc_job.take() else {
            return;
        };
        self.finish_doc_job(job.kind, result);
        if self.when_idle {
            self.when_idle = false;
            if let Some(action) = self.pending.take() {
                self.request(action);
            }
        }
    }

    fn finish_doc_job(&mut self, kind: DocJobKind, result: Result<DocOutput, JobError>) {
        match (kind, result) {
            (DocJobKind::Open { path, recovery }, Ok(DocOutput::Opened(opened))) => {
                self.finish_open(&path, recovery, *opened);
            }
            (DocJobKind::Save { path }, Ok(DocOutput::Saved { revision, stamp })) => {
                self.finish_save(&path, revision, stamp);
            }
            (DocJobKind::Export { path }, Ok(DocOutput::Exported)) => {
                self.toast(Level::Info, format!("Exported {}", path.display()));
            }
            (kind, Ok(_)) => {
                log::error!("a job returned a result of another kind");
                drop(kind);
            }
            (kind, Err(error)) => self.fail_doc_job(&kind, &error),
        }
    }

    fn finish_open(&mut self, path: &Path, recovery: Option<Box<Candidate>>, opened: Opened) {
        let Opened {
            doc,
            doc_path,
            stamp,
            warnings,
        } = opened;
        // Edits made while the file loaded would be lost.
        self.request_autosave(Reason::Risky);
        let recovered = recovery.is_some();
        self.editor.replace(doc, doc_path, recovered);
        // A recovered document keeps what its session knew of the file, not how it looks now.
        self.editor.source_stamp = recovery.as_ref().map_or(stamp, |candidate| candidate.stamp);
        self.last_autosaved = None;
        for warning in warnings.iter().take(20) {
            log::warn!("{}: {warning}", path.display());
        }
        let name = file_name(path);
        if let Some(candidate) = recovery {
            // The recovered work is unsaved: protect it, then drop the old autosave.
            self.autosave_and_remove(Reason::Risky, vec![*candidate]);
            self.toast(
                Level::Info,
                format!("Recovered {name}. Save it to keep it."),
            );
            self.show_recovery_rest();
            return;
        }
        let note = match warnings.len() {
            0 => String::new(),
            1 => " (1 warning, see the log)".to_owned(),
            count => format!(" ({count} warnings, see the log)"),
        };
        self.toast(Level::Info, format!("Opened {name}{note}"));
        self.add_recent(path);
        self.show_recovery_rest();
    }

    fn finish_save(&mut self, path: &Path, revision: Rev, stamp: Option<Stamp>) {
        self.editor.mark_saved(path, revision);
        self.editor.source_stamp = stamp;
        self.add_recent(path);
        self.toast(Level::Info, format!("Saved {}", file_name(path)));
        if !self.editor.is_dirty() {
            // The saved file is the newest state of this document; its autosaves are redundant.
            // Other documents of the session, and other sessions, are not touched.
            let dir = self.dirs.recovery();
            let session = self.session.clone();
            let documents = [self.editor.doc_id()];
            drop(self.io.submit(move |_| {
                backup::purge_docs(&dir, &session, &documents);
                Ok(())
            }));
        }
        if let Some(action) = self.after_save.take() {
            self.request(action);
        }
    }

    fn fail_doc_job(&mut self, kind: &DocJobKind, error: &JobError) {
        self.after_save = None;
        let (verb, path) = match kind {
            DocJobKind::Open { path, .. } => ("open", path),
            DocJobKind::Save { path } => ("save", path),
            DocJobKind::Export { path } => ("export", path),
        };
        let name = file_name(path);
        match error {
            JobError::Cancelled => {
                self.toast(Level::Info, format!("Cancelled: {verb} {name}"));
            }
            JobError::NotFound if verb == "open" => {
                self.settings
                    .recent_files
                    .remove(&path.display().to_string());
                self.settings_dirty = true;
                self.toast(
                    Level::Error,
                    format!("Cannot {verb} {name}: it does not exist."),
                );
            }
            other => {
                log::warn!("cannot {verb} {}: {other}", path.display());
                self.toast(Level::Error, format!("Cannot {verb} {name}: {other}"));
            }
        }
        if let DocJobKind::Open { recovery, .. } = kind {
            // The autosaves on offer stay on offer, the one that failed to open included.
            if let Some(candidate) = recovery.as_ref().filter(|_| *error != JobError::NotFound) {
                self.recovery_rest.insert(0, (**candidate).clone());
            }
            self.show_recovery_rest();
        }
    }

    fn add_recent(&mut self, path: &Path) {
        self.settings.recent_files.add(&path.display().to_string());
        self.settings_dirty = true;
    }

    fn accept_recovery(&mut self, index: usize) {
        let Some(Dialog::Recovery { candidates }) = &mut self.dialog else {
            return;
        };
        if index >= candidates.len() {
            return;
        }
        if self.doc_job.is_some() {
            self.busy_toast();
            return;
        }
        let chosen = candidates.remove(index);
        self.recovery_rest = std::mem::take(candidates);
        self.dialog = None;
        // A document with unsaved changes is asked about first; the dialog returns on Cancel.
        self.request(Pending::Recover(Box::new(chosen)));
    }

    fn discard_recovery(&mut self, index: usize) {
        let Some(Dialog::Recovery { candidates }) = &mut self.dialog else {
            return;
        };
        if index >= candidates.len() {
            return;
        }
        let dropped = candidates.remove(index);
        if candidates.is_empty() {
            self.dialog = None;
            self.leave_safe_mode();
        }
        drop(self.io.submit(move |_| {
            backup::discard(&dropped);
            Ok(())
        }));
    }
}

/// Shows the system file dialog. Runs on its own thread; the UI thread keeps drawing.
async fn ask_for_path(purpose: PickPurpose, start: Option<PathBuf>, name: &str) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    let picked = match purpose {
        PickPurpose::Open => {
            dialog
                .add_filter("Cuboid and Cubik models", &["cubik", "cub"])
                .pick_file()
                .await
        }
        PickPurpose::SaveAs | PickPurpose::Export => {
            dialog
                .add_filter("Cubik model", &["cubik"])
                .set_file_name(name)
                .save_file()
                .await
        }
    };
    picked.map(|handle| handle.path().to_owned())
}
