//! New, open, save, save as, export of a copy, close with the unsaved-changes question, recent
//! files and recovery (spec: app 2, 11, 12; roadmap 7). Every error becomes a toast.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use cuboid_assets::mc_versions::McVersion;
use cuboid_core::{CmdOutcome, Command, ElementKind};
use cuboid_fmt_cubik::cub::{self, DiskFiles, Notice};
use cuboid_fmt_cubik::{CubikError, Limits, load_path};
use cuboid_model::image::Rev;
use cuboid_model::voxel::VoxelWorld;
use cuboid_model::{ApplyMode, ApplySource, Document, EditCtx, EditEnv, Selection, apply_document};
use cuboid_num::EngineRng;
use cuboid_ui::{
    ExportFormat, ExportOutcome, ExportReport, ExportRequest, ExportSettings, Template,
};
use cuboid_validate::{Issue, Severity};

use crate::backup::{self, Candidate, Reason, Stamp};
use crate::editor::empty_document;
use crate::frontend::Frontend;
use crate::jobs::{JobCtx, JobError, JobHandle};
use crate::preset::{BriefDefaults, ProjectSettings};
use crate::state::{Dialog, Intent, Level, Pending, Picker, State, UnsavedAnswer};
use crate::validation::{
    apply_fixes, check_profile, export_report, export_target, fixable, model_name, parse_version,
    start_check,
};

/// What the file dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickPurpose {
    Open,
    SaveAs,
    Export,
    /// A folder for the export or render dialog.
    Folder,
}

/// A file read and applied to a fresh document, ready to replace the open one.
pub struct Opened {
    pub doc: Document,
    /// The path the document is saved to; none for an import.
    pub doc_path: Option<PathBuf>,
    /// The file at `doc_path` as it was before it was read.
    pub stamp: Option<Stamp>,
    pub warnings: Vec<String>,
    /// The preset the file's format and its `cuboid.json` entry give.
    pub project: ProjectSettings,
}

pub enum DocOutput {
    Opened(Box<Opened>),
    Saved {
        revision: Rev,
        stamp: Option<Stamp>,
    },
    /// The file is written. The texts say what the format could not hold.
    Exported(Vec<String>),
    /// Nothing is written: these files of the export exist and the user has not agreed to
    /// replace them.
    Exists(Vec<PathBuf>),
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
        /// The choices of the export dialog; none for the plain copy of the old file dialog.
        plan: Option<ExportPlan>,
    },
}

/// The game version and the checks of an export that the export dialog asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportPlan {
    /// The game version of the Minecraft formats.
    pub version: McVersion,
    /// Write although the model breaks the rules of the version.
    pub force: bool,
    /// The target is a folder: the resource pack is written as files.
    pub folder: bool,
}

impl ExportPlan {
    /// The newest version, no force: what the export by file name wrote before the dialog.
    pub fn latest() -> Self {
        Self {
            version: cuboid_fmt_minecraft::latest_version(),
            force: false,
            folder: false,
        }
    }
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

/// Whether the path names a Blockbench project.
fn is_bbmodel(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bbmodel"))
}

/// Whether the path names a mesh format: Wavefront OBJ, binary glTF or glTF with a buffer file.
fn is_mesh(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        ["obj", "glb", "gltf"]
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// Whether the path names a MagicaVoxel model.
fn is_vox(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vox"))
}

/// Whether the path names a Minecraft Java block or item model.
fn is_minecraft_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

/// Whether the path names a resource pack archive.
fn is_resource_pack(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

/// The texts of the notices of an exporter; they also go to the log.
fn notice_texts<N: std::fmt::Display>(target: &Path, notices: &[N]) -> Vec<String> {
    notices
        .iter()
        .map(|notice| {
            log::warn!("{}: {notice}", target.display());
            notice.to_string()
        })
        .collect()
}

/// What a format leaves out of the animation data of the document: the animation controllers and
/// the like an opened `.bbmodel` project keeps, which only `.bbmodel` files write back, and with
/// `animations` the animations of the model, which only `.bbmodel` and `.cubik` files hold. `None`
/// when there is nothing.
fn kept_note(doc: &Document, animations: bool) -> Option<String> {
    let mut kept = cuboid_fmt_bbmodel::kept_animations(doc.retained.as_deref());
    if animations && !doc.animations.is_empty() {
        let count = doc.animations.len();
        match kept.iter_mut().find(|(what, _)| what == "animations") {
            Some((_, n)) => *n += count,
            None => kept.insert(0, ("animations".to_owned(), count)),
        }
    }
    if kept.is_empty() {
        return None;
    }
    let list: Vec<String> = kept
        .iter()
        .map(|(what, count)| format!("{count} {what}"))
        .collect();
    Some(format!(
        "{} of the project are not written to this file",
        list.join(", ")
    ))
}

/// The files among `paths` that exist, when the export would replace a file the user has not been
/// asked about: any file other than `target`, which the file dialog or the export dialog already
/// covered. Empty when nothing needs a question.
fn collisions(target: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    let taken: Vec<PathBuf> = paths.iter().filter(|path| path.exists()).cloned().collect();
    if taken.iter().all(|path| path == target) {
        return Vec::new();
    }
    taken
}

/// Writes the document as a Minecraft Java model for the version of the plan; the pictures of
/// textures without a Minecraft path go next to it. Existing files are replaced only when
/// `replace` is set, otherwise they are returned and nothing is written. The notices tell what
/// the file cannot hold.
fn export_minecraft_json(
    doc: &Document,
    target: &Path,
    plan: ExportPlan,
    replace: bool,
    ctx: &JobCtx,
) -> Result<DocOutput, JobError> {
    ctx.progress(0.1);
    ctx.check()?;
    let mut options = cuboid_fmt_minecraft::JsonOptions::new(plan.version);
    options.force = plan.force;
    let model = cuboid_fmt_minecraft::json_export::export(doc, &options)
        .map_err(|error| JobError::Failed(error.to_string()))?;
    let notices = notice_texts(target, &model.notices);
    ctx.check()?;
    let mut paths: Vec<PathBuf> = model
        .files
        .iter()
        .map(|file| cuboid_io::names::sibling(target, &file.path))
        .collect();
    if !replace {
        paths.push(target.to_owned());
        let taken = collisions(target, &paths);
        if !taken.is_empty() {
            return Ok(DocOutput::Exists(taken));
        }
        paths.pop();
    }
    for (file, path) in model.files.iter().zip(&paths) {
        cuboid_io::write_atomic(path, &file.bytes).map_err(|error| io_failure(&error))?;
    }
    cuboid_io::write_atomic(target, model.json.as_bytes()).map_err(|error| io_failure(&error))?;
    Ok(DocOutput::Exported(notices))
}

/// Whether a path inside a pack stays inside it.
fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

/// Writes the document as a resource pack for the version of the plan: an archive, or the files
/// in a folder. Files of a folder that exist are replaced only when `replace` is set, otherwise
/// they are returned and nothing is written. The notices tell the findings of the validator and
/// what the pack cannot hold.
fn export_resource_pack(
    doc: &Document,
    target: &Path,
    plan: ExportPlan,
    replace: bool,
    ctx: &JobCtx,
) -> Result<DocOutput, JobError> {
    ctx.progress(0.1);
    ctx.check()?;
    let mut options = cuboid_fmt_minecraft::PackOptions::new(plan.version);
    options.force = plan.force;
    let pack = cuboid_fmt_minecraft::export_pack(doc, &options)
        .map_err(|error| JobError::Failed(error.to_string()))?;
    let notices = notice_texts(target, &pack.notices);
    if plan.folder {
        let mut paths = Vec::with_capacity(pack.files.len());
        for file in &pack.files {
            if !stays_inside(&file.path) {
                return Err(JobError::Failed(format!(
                    "the pack names the path `{}`, which leaves the folder",
                    file.path
                )));
            }
            paths.push(target.join(&file.path));
        }
        if !replace {
            let taken = collisions(target, &paths);
            if !taken.is_empty() {
                return Ok(DocOutput::Exists(taken));
            }
        }
        for (file, path) in pack.files.iter().zip(&paths) {
            ctx.check()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| io_failure(&error))?;
            }
            cuboid_io::write_atomic(path, &file.bytes).map_err(|error| io_failure(&error))?;
        }
        return Ok(DocOutput::Exported(notices));
    }
    let bytes = pack
        .to_zip()
        .map_err(|error| JobError::Failed(error.to_string()))?;
    ctx.check()?;
    cuboid_io::write_atomic(target, &bytes).map_err(|error| io_failure(&error))?;
    Ok(DocOutput::Exported(notices))
}

/// Writes the voxel world as a MagicaVoxel model. Returns what the format changes.
fn export_vox(
    world: Option<VoxelWorld>,
    target: &Path,
    ctx: &JobCtx,
) -> Result<Vec<String>, JobError> {
    ctx.progress(0.1);
    ctx.check()?;
    let world = world
        .ok_or_else(|| JobError::Failed("the model has no voxel world to export".to_owned()))?;
    let vox =
        cuboid_fmt_voxel::vox::export(&world, &cuboid_fmt_voxel::vox::ExportOptions::default())
            .map_err(|error| JobError::Failed(error.to_string()))?;
    let notices = notice_texts(target, &vox.notices);
    ctx.check()?;
    cuboid_io::write_atomic(target, &vox.bytes).map_err(|error| io_failure(&error))?;
    Ok(notices)
}

/// Writes the document as OBJ, GLB or glTF, with the material, buffer and texture files next to
/// the target. Existing files are replaced only when `replace` is set, otherwise they are returned
/// and nothing is written. The notices tell what the format cannot hold.
fn export_mesh(
    doc: &Document,
    target: &Path,
    replace: bool,
    ctx: &JobCtx,
) -> Result<DocOutput, JobError> {
    ctx.progress(0.1);
    ctx.check()?;
    let opts = cuboid_fmt_mesh::MeshOptions::for_document(doc);
    let export = cuboid_fmt_mesh::export_for_path(doc, target, &opts)
        .map_err(|error| JobError::Failed(error.to_string()))?;
    let notices = notice_texts(target, &export.notices);
    ctx.check()?;
    if !replace {
        let dir = target.parent().unwrap_or_else(|| Path::new(""));
        let mut paths: Vec<PathBuf> = export
            .files
            .iter()
            .skip(1)
            .map(|file| dir.join(&file.name))
            .collect();
        paths.push(target.to_owned());
        let taken = collisions(target, &paths);
        if !taken.is_empty() {
            return Ok(DocOutput::Exists(taken));
        }
    }
    export.write(target).map_err(|error| io_failure(&error))?;
    Ok(DocOutput::Exported(notices))
}

/// Writes the document as a Blockbench project. Returns what the project cannot hold.
fn export_bbmodel(doc: &Document, target: &Path, ctx: &JobCtx) -> Result<Vec<String>, JobError> {
    ctx.progress(0.1);
    ctx.check()?;
    let project =
        cuboid_fmt_bbmodel::export(doc).map_err(|error| JobError::Failed(error.to_string()))?;
    let notices = notice_texts(target, &project.notices);
    ctx.check()?;
    cuboid_io::write_atomic(target, &project.bytes).map_err(|error| io_failure(&error))?;
    Ok(notices)
}

/// The content of a file, after its size was checked against `limit`: a file over it is never read.
fn read_within(path: &Path, limit: u64) -> Result<Vec<u8>, JobError> {
    let size = fs::metadata(path)
        .map_err(|error| io_failure(&error))?
        .len();
    if size > limit {
        return Err(JobError::Failed(format!(
            "the file is {size} bytes, over the limit of {limit}"
        )));
    }
    fs::read(path).map_err(|error| io_failure(&error))
}

/// Reads a `.cubik`, `.cub` or `.bbmodel` file into a fresh document. `doc_path` becomes the document's
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
    let mut project_meta = None;
    let (loaded, mut warnings) = match extension.as_str() {
        "cubik" => {
            let loaded = load_path(path, &Limits::default()).map_err(cubik_failure)?;
            project_meta = loaded.project;
            (loaded.doc, loaded.warnings)
        }
        "cub" => {
            let bytes = read_within(path, Limits::default().max_text)?;
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
        "bbmodel" => {
            let bytes = read_within(path, cuboid_fmt_bbmodel::Limits::default().max_text)?;
            let imported = cuboid_fmt_bbmodel::import(&bytes, path, &DiskFiles)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            let notes = imported.notices.iter().map(ToString::to_string);
            (imported.doc, notes.collect())
        }
        "json" => {
            let bytes = read_within(path, cuboid_fmt_minecraft::Limits::default().max_text)?;
            let imported = cuboid_fmt_minecraft::import(&bytes, path, &DiskFiles)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            let notes = imported.notices.iter().map(ToString::to_string);
            (imported.doc, notes.collect())
        }
        "vox" => {
            let bytes = read_within(path, cuboid_fmt_voxel::vox::Limits::default().max_file)?;
            let imported = cuboid_fmt_voxel::vox::import(&bytes, path)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            let notes = imported.notices.iter().map(ToString::to_string);
            (imported.doc, notes.collect())
        }
        _ => {
            return Err(JobError::Failed(
                "only .cubik, .cub, .bbmodel, .json and .vox files can be opened".to_owned(),
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
    // A file stores no geometry for the primitive meshes.
    cuboid_ops::mesh::ensure_geometry(&mut doc);
    // The path in the file is where its author saved it.
    let text = doc_path.map_or_else(String::new, |path| path.display().to_string());
    std::sync::Arc::make_mut(&mut doc.props).save_path = text;
    ctx.progress(1.0);
    Ok(Opened {
        doc,
        doc_path: doc_path.map(Path::to_owned),
        stamp,
        warnings,
        project: ProjectSettings::for_file(&extension, project_meta.as_ref()),
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
                // The brief of the project the user is working in, before the new document
                // (which has no folder yet) takes the project library away.
                let defaults = self
                    .library
                    .project_brief()
                    .map(BriefDefaults::of)
                    .unwrap_or_default();
                self.run(Pending::New);
                self.editor.use_template(template, &defaults);
                let command = match template {
                    Template::Block | Template::CubikClassic => {
                        Command::AddElement(ElementKind::Cube)
                    }
                    Template::Item => Command::AddElement(ElementKind::PlaneZ),
                    Template::Voxel { resolution } => Command::SetupVoxels { resolution },
                    Template::Empty => {
                        self.editor.mark_clean();
                        return;
                    }
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
        // An export may name a Blockbench project or a mesh file; everything else is a `.cubik`
        // file.
        let fixed = if purpose == PickPurpose::Export
            && (is_bbmodel(&path)
                || is_mesh(&path)
                || is_vox(&path)
                || is_minecraft_json(&path)
                || is_resource_pack(&path))
        {
            path.clone()
        } else {
            ensure_cubik_extension(path.clone())
        };
        if fixed != path && fixed.exists() {
            self.dialog = Some(Dialog::Overwrite {
                path: fixed,
                also: Vec::new(),
                purpose,
                plan: None,
            });
            return;
        }
        match purpose {
            PickPurpose::SaveAs => self.start_save(fixed),
            PickPurpose::Export => self.start_export_with(fixed, None, false),
            PickPurpose::Open | PickPurpose::Folder => {}
        }
    }

    fn answer_overwrite(&mut self, replace: bool) {
        let Some(Dialog::Overwrite {
            path,
            purpose,
            plan,
            ..
        }) = &self.dialog
        else {
            return;
        };
        let (path, purpose, plan) = (path.clone(), *purpose, *plan);
        self.dialog = None;
        if !replace {
            self.after_save = None;
            if plan.is_some() {
                self.export_result = Some(cuboid_ui::ExportOutcome::Declined);
            }
            return;
        }
        match purpose {
            PickPurpose::SaveAs => self.start_save(path),
            PickPurpose::Export => self.start_export_with(path, plan, true),
            PickPurpose::Open | PickPurpose::Folder => {}
        }
    }

    pub fn pick(&mut self, purpose: PickPurpose) {
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
            PickPurpose::Folder => self.folder_answer = Some(path),
        }
    }

    pub fn start_open(&mut self, path: PathBuf, recovery: Option<Candidate>) {
        if self.doc_job.is_some() {
            self.busy_toast();
            return;
        }
        let imported = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cub"))
            || is_bbmodel(&path)
            || is_vox(&path)
            || is_minecraft_json(&path);
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

    /// Writes a copy of the document in the format the extension of `path` names, or the pack
    /// folder of the plan. Without a plan the newest game version is used and a model that does
    /// not fit is refused. Files next to the target that the export would replace are asked about
    /// first, unless `replace` says the user agreed.
    pub fn start_export_with(&mut self, path: PathBuf, plan: Option<ExportPlan>, replace: bool) {
        if self.doc_job.is_some() {
            self.busy_toast();
            return;
        }
        let target = path.clone();
        let chosen = plan.unwrap_or_else(ExportPlan::latest);
        let handle = if chosen.folder || is_resource_pack(&path) {
            let doc = self.editor.core.doc().clone();
            self.io
                .submit(move |ctx| export_resource_pack(&doc, &target, chosen, replace, ctx))
        } else if is_bbmodel(&path) {
            // A pointer copy of the document, which the worker reads.
            let doc = self.editor.core.doc().clone();
            self.io
                .submit(move |ctx| export_bbmodel(&doc, &target, ctx).map(DocOutput::Exported))
        } else if is_minecraft_json(&path) {
            let doc = self.editor.core.doc().clone();
            self.io
                .submit(move |ctx| export_minecraft_json(&doc, &target, chosen, replace, ctx))
        } else if is_vox(&path) {
            let world = self.editor.core.doc().voxels.clone();
            self.io
                .submit(move |ctx| export_vox(world, &target, ctx).map(DocOutput::Exported))
        } else if is_mesh(&path) {
            let doc = self.editor.core.doc().clone();
            self.io
                .submit(move |ctx| export_mesh(&doc, &target, replace, ctx))
        } else {
            let input = self.editor.save_input(None);
            self.io.submit(move |ctx| {
                ctx.progress(0.1);
                ctx.check()?;
                input.write_copy(&target).map_err(cubik_failure)?;
                Ok(DocOutput::Exported(Vec::new()))
            })
        };
        self.doc_job = Some(DocJob {
            title: format!("Exporting {}", file_name(&path)),
            kind: DocJobKind::Export { path, plan },
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
            (DocJobKind::Export { path, plan }, Ok(DocOutput::Exported(mut notices))) => {
                // A model file written over a recent one changes its picture; other formats ask
                // for nothing.
                self.library.thumbnail_saved(&path);
                let kept = if is_bbmodel(&path) {
                    None
                } else {
                    // A `.cubik` file holds the animations of the model, not the rest.
                    let cubik = ensure_cubik_extension(path.clone()) == path;
                    kept_note(self.editor.core.doc(), !cubik)
                };
                if plan.is_some() {
                    // The export dialog shows the result and its notices.
                    notices.extend(kept);
                    self.export_result = Some(cuboid_ui::ExportOutcome::Done { path, notices });
                } else {
                    self.toast(Level::Info, format!("Exported {}", path.display()));
                    if let Some(note) = kept {
                        self.toast(Level::Warn, note);
                    }
                }
            }
            (DocJobKind::Export { path, plan }, Ok(DocOutput::Exists(also))) => {
                if self.dialog.is_some() {
                    let text =
                        "Not exported: files would be replaced and another question is open.";
                    if plan.is_some() {
                        self.export_result =
                            Some(cuboid_ui::ExportOutcome::Failed(text.to_owned()));
                    } else {
                        self.toast(Level::Warn, text);
                    }
                    return;
                }
                self.dialog = Some(Dialog::Overwrite {
                    path,
                    also,
                    purpose: PickPurpose::Export,
                    plan,
                });
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
            project,
        } = opened;
        // Edits made while the file loaded would be lost.
        self.request_autosave(Reason::Risky);
        let recovered = recovery.is_some();
        self.editor.replace_with(doc, doc_path, recovered, project);
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
        self.library.thumbnail_saved(path);
        self.toast(Level::Info, format!("Saved {}", file_name(path)));
        let doc_id = self.editor.doc_id();
        if self.editor.kept_warned != Some(doc_id)
            && let Some(note) = kept_note(self.editor.core.doc(), false)
        {
            self.editor.kept_warned = Some(doc_id);
            self.toast(Level::Warn, note);
        }
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
            DocJobKind::Export { path, .. } => ("export", path),
        };
        let name = file_name(path);
        if let DocJobKind::Export { plan: Some(_), .. } = kind
            && *error != JobError::Cancelled
        {
            log::warn!("cannot export {}: {error}", path.display());
            self.export_result = Some(cuboid_ui::ExportOutcome::Failed(format!(
                "Cannot export {name}: {error}"
            )));
            return;
        }
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
                .add_filter(
                    "Cuboid, Cubik, Blockbench and MagicaVoxel models",
                    &["cubik", "cub", "bbmodel", "json", "vox"],
                )
                .pick_file()
                .await
        }
        PickPurpose::SaveAs => {
            dialog
                .add_filter("Cubik model", &["cubik"])
                .set_file_name(name)
                .save_file()
                .await
        }
        PickPurpose::Folder => dialog.pick_folder().await,
        PickPurpose::Export => {
            dialog
                .add_filter("Cubik model", &["cubik"])
                .add_filter("Blockbench project", &["bbmodel"])
                .add_filter("MagicaVoxel model", &["vox"])
                .add_filter("Minecraft Java model", &["json"])
                .add_filter("Minecraft resource pack", &["zip"])
                .add_filter("Wavefront OBJ", &["obj"])
                .add_filter("glTF binary", &["glb"])
                .add_filter("glTF", &["gltf"])
                .set_file_name(name)
                .save_file()
                .await
        }
    };
    picked.map(|handle| handle.path().to_owned())
}

/// The checks of the export dialog: the one that runs and the issues of the last one, whose
/// fixes the dialog may apply.
#[derive(Default)]
pub struct ExportFlow {
    check: Option<(JobHandle<Vec<Issue>>, String)>,
    issues: Vec<Issue>,
}

impl ExportFlow {
    /// True while a check runs.
    pub fn is_checking(&self) -> bool {
        self.check.is_some()
    }
}

impl Frontend {
    /// What the export dialog asks for.
    pub fn export_request(&mut self, state: &mut State, request: ExportRequest) {
        match request {
            ExportRequest::Check(settings) => self.start_export_check(state, &settings),
            ExportRequest::FixErrors(settings) => {
                let doc_issues = &self.export.issues;
                let errors: Vec<&Issue> = doc_issues
                    .iter()
                    .filter(|issue| issue.severity == Severity::Error && fixable(issue))
                    .collect();
                let summary = apply_fixes(&mut state.editor.core, &errors, &mut self.gesture);
                if let Some(why) = summary.failed.first() {
                    state.toast(Level::Warn, format!("Some fixes did not apply: {why}"));
                }
                self.start_export_check(state, &settings);
            }
            ExportRequest::Write {
                settings,
                path,
                force,
            } => {
                if state.doc_job.is_some() {
                    self.ui.export_finished(ExportOutcome::Failed(
                        "Please wait, another file job is not finished.".to_owned(),
                    ));
                    return;
                }
                let plan = ExportPlan {
                    version: parse_version(&settings.version),
                    force,
                    folder: settings.format == ExportFormat::ResourcePackFolder,
                };
                state.start_export_with(path, Some(plan), false);
            }
            ExportRequest::CancelCheck => {
                if let Some((handle, _)) = self.export.check.take() {
                    handle.cancel();
                }
            }
        }
    }

    fn start_export_check(&mut self, state: &State, settings: &ExportSettings) {
        if let Some((old, _)) = self.export.check.take() {
            old.cancel();
        }
        self.export.issues.clear();
        let doc = state.editor.core.doc();
        let version = parse_version(&settings.version);
        match export_target(doc, settings.format, version) {
            Some((target, label)) => {
                let path = state.editor.path();
                let profile = check_profile(
                    target,
                    state.library.project_brief(),
                    &model_name(path.as_deref()),
                );
                self.export.check = Some((start_check(doc, profile), label));
            }
            None => self.ui.export_checked(ExportReport::default()),
        }
    }

    /// Hands the answers of the workers to the dialog: a finished check, a finished export and a
    /// chosen folder.
    pub fn poll_export(&mut self, state: &mut State) {
        let finished = self
            .export
            .check
            .as_mut()
            .and_then(|(handle, _)| handle.poll());
        if let Some(result) = finished {
            let label = self
                .export
                .check
                .take()
                .map_or_else(String::new, |(_, label)| label);
            match result {
                Ok(issues) => {
                    let report = export_report(&label, &issues);
                    self.export.issues = issues;
                    self.ui.export_checked(report);
                }
                Err(JobError::Cancelled) => {}
                Err(error) => self.ui.export_finished(ExportOutcome::Failed(format!(
                    "The check of the model failed: {error}"
                ))),
            }
        }
        if let Some(outcome) = state.export_result.take() {
            self.ui.export_finished(outcome);
        }
        if let Some(folder) = state.folder_answer.take() {
            self.ui.set_folder(&folder);
        }
    }
}

#[cfg(test)]
mod bbmodel_tests {
    use cuboid_model::RenderPrefs;

    use super::*;

    fn env() -> EditEnv {
        EditEnv {
            restrict_rotations: true,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        }
    }

    const PROJECT: &str = r#"{
        "meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
        "name": "Box",
        "resolution": {"width": 16, "height": 16},
        "elements": [{"name": "a", "from": [0, 0, 0], "to": [8, 8, 8], "type": "cube",
            "origin": [4, 4, 4], "rotation": [0, 22.5, 0],
            "faces": {"north": {"uv": [0, 0, 8, 8], "texture": null}}}],
        "textures": [{"name": "t.png", "id": "0"}]
    }"#;

    #[test]
    fn a_project_opens_as_an_untitled_import_and_exports_again() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("box.bbmodel");
        fs::write(&source, PROJECT).unwrap();
        let ctx = JobCtx::default();
        let opened = open_file(&source, None, &env(), &ctx).unwrap();
        assert!(opened.doc_path.is_none());
        assert_eq!(opened.doc.elements.len(), 1);
        // The texture has no picture: one warning.
        assert_eq!(opened.warnings.len(), 1, "{:?}", opened.warnings);

        let target = dir.path().join("copy.bbmodel");
        export_bbmodel(&opened.doc, &target, &ctx).unwrap();
        let again = open_file(&target, None, &env(), &ctx).unwrap();
        assert_eq!(again.doc.elements.len(), 1);
        assert_eq!(
            again.doc.elements.items()[0].base.e,
            opened.doc.elements.items()[0].base.e
        );
    }

    #[test]
    fn a_project_exports_as_obj_and_glb() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("box.bbmodel");
        fs::write(&source, PROJECT).unwrap();
        let ctx = JobCtx::default();
        let opened = open_file(&source, None, &env(), &ctx).unwrap();

        let obj = dir.path().join("box.obj");
        export_mesh(&opened.doc, &obj, false, &ctx).unwrap();
        assert!(fs::read_to_string(&obj).unwrap().contains("\nf "));
        assert!(dir.path().join("box.mtl").is_file());

        let glb = dir.path().join("box.glb");
        export_mesh(&opened.doc, &glb, false, &ctx).unwrap();
        assert_eq!(&fs::read(&glb).unwrap()[..4], b"glTF");

        let unknown = dir.path().join("box.stl");
        assert!(matches!(
            export_mesh(&opened.doc, &unknown, false, &ctx),
            Err(JobError::Failed(_))
        ));
        assert!(!unknown.exists());
    }

    #[test]
    fn a_damaged_project_is_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("bad.bbmodel");
        fs::write(&source, "{ not json").unwrap();
        let ctx = JobCtx::default();
        assert!(matches!(
            open_file(&source, None, &env(), &ctx),
            Err(JobError::Failed(_))
        ));
        assert!(matches!(
            open_file(&dir.path().join("none.bbmodel"), None, &env(), &ctx),
            Err(JobError::NotFound)
        ));
    }
}

#[cfg(test)]
mod minecraft_tests {
    use cuboid_model::RenderPrefs;

    use super::*;

    fn env() -> EditEnv {
        EditEnv {
            restrict_rotations: true,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        }
    }

    const MODEL: &str = r##"{
        "textures": { "all": "block/stone" },
        "elements": [{ "from": [0, 0, 0], "to": [16, 8, 16],
            "rotation": { "origin": [8, 0, 8], "axis": "y", "angle": 22.5 },
            "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#all" } } }]
    }"##;

    #[test]
    fn a_model_opens_as_an_untitled_import_and_exports_as_json_and_pack() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("slab.json");
        fs::write(&source, MODEL).unwrap();
        let ctx = JobCtx::default();
        let opened = open_file(&source, None, &env(), &ctx).unwrap();
        assert!(opened.doc_path.is_none());
        assert_eq!(opened.doc.elements.len(), 1);
        // The texture file is missing: one warning, and the path is kept.
        assert_eq!(opened.warnings.len(), 1, "{:?}", opened.warnings);

        let json = dir.path().join("copy.json");
        export_minecraft_json(&opened.doc, &json, ExportPlan::latest(), false, &ctx).unwrap();
        let again = open_file(&json, None, &env(), &ctx).unwrap();
        assert_eq!(again.doc.elements.len(), 1);
        assert_eq!(
            again.doc.elements.items()[0].base.e,
            opened.doc.elements.items()[0].base.e
        );

        let zip = dir.path().join("pack.zip");
        export_resource_pack(&opened.doc, &zip, ExportPlan::latest(), false, &ctx).unwrap();
        assert_eq!(&fs::read(&zip).unwrap()[..2], b"PK");
    }

    #[test]
    fn a_damaged_model_is_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("bad.json");
        fs::write(&source, "{ not json").unwrap();
        let ctx = JobCtx::default();
        assert!(matches!(
            open_file(&source, None, &env(), &ctx),
            Err(JobError::Failed(_))
        ));
        assert!(matches!(
            open_file(&dir.path().join("none.json"), None, &env(), &ctx),
            Err(JobError::NotFound)
        ));
        // An empty model has nothing to export.
        let empty = empty_document();
        assert!(matches!(
            export_minecraft_json(
                &empty,
                &dir.path().join("e.json"),
                ExportPlan::latest(),
                false,
                &ctx
            ),
            Err(JobError::Failed(_))
        ));
        assert!(!dir.path().join("e.json").exists());
    }
}
