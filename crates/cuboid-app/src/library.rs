//! The library in the app: the two scopes, their import, edit and thumbnail jobs, and the plain
//! view the UI shows. Everything that touches files runs on a thread of its own (a second
//! `IoThread`, so a long import never delays a save); the UI thread only moves values, searches
//! the index in memory and builds the view when something changed.
//!
//! A library is moved into the job that changes it and comes back with the result, so there is
//! never a lock to wait for; while it is away the view says `busy` and edits are refused. A job
//! that fails to return it (a panic) makes the service read the index again.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use cuboid_io::texture_files::{DiskFiles, TextureFiles};
use cuboid_library::thumb::{Pixels, read_thumb, recent_thumb_path, reference_thumb};
use cuboid_library::{
    Brief, Control, DisplayPreset as LibDisplayPreset, Entry, INDEX_FILE, ImportOptions,
    ImportReport, Kind, Library, LibraryError, Meta, NameCase, Naming, PROJECT_DIR, Platform,
    Query, Role, Scope, Suggestion, SuggestionKind, View, within,
};
use cuboid_mcp::Root;
use cuboid_mcp::connect::DESKTOP_CONFIG;
use cuboid_model::image::{Image, ImageFileKind, decode_file, encode_png};
use cuboid_ui::{
    BriefCardView, BriefInputs, ClientsView, DisplayPreset as UiDisplayPreset, ImageData, JobView,
    LibEntryView, LibKind, LibScope, LibraryRequest, LibraryView, StyleSourceView,
    SuggestionAction, SuggestionGoal, SuggestionView, Template, ThumbState,
};

use crate::io_thread::IoThread;
use crate::jobs::{JobCtx, JobError, JobHandle};
use crate::mcp::client_lines;
use crate::platform::Dirs;
use crate::state::{Level, State};

/// Rows the list holds at most; the view says how many more match.
const MAX_ROWS: usize = 500;
/// Newest entries kept in the summary.
const RECENT_IN_SUMMARY: usize = 0;
/// Pending thumbnail renders; the rest are asked for again when their rows show.
const MAX_RENDERS: usize = 64;
/// Id of the library job in the job list of the UI.
pub const JOB_ID: u64 = 2;
/// File name of the pinned recent files, next to `settings.json`.
const PINS_FILE: &str = "pins.json";
/// Extensions the thumbnail renderer can open.
const RENDERABLE: [&str; 3] = ["cubik", "cub", "bbmodel"];
const IMPORT_FILTER: [&str; 12] = [
    "cubik", "cub", "bbmodel", "json", "mcmeta", "png", "jpg", "jpeg", "webp", "gif", "md", "txt",
];

/// A picture and where it is kept.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ThumbKey {
    /// An entry of a library.
    Entry(Scope, String),
    /// A file of the recent list.
    Recent(PathBuf),
}

/// A model to draw into a 160 px picture; the render rig of the frontend takes these one by one.
#[derive(Clone, Debug)]
pub struct RenderRequest {
    /// Whose picture it is.
    pub key: ThumbKey,
    /// The model file to open.
    pub source: PathBuf,
    /// Where the PNG goes.
    pub out: PathBuf,
}

enum Thumb {
    Loading,
    Missing,
    Ready(ImageData),
}

/// A library texture read and decoded off the UI thread, ready for `Command::AddTexture`.
pub struct Prepared {
    /// The file stem; the core makes it a clean, free label.
    pub stem: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Drop of a library texture on the model: the file is read on the library thread and the next
/// frame adds it with `Command::AddTexture`, one undo step.
pub fn use_texture(state: &mut State, id: &str) {
    state.library.prepare_texture(id);
}

/// What a job brings back with the library.
struct Output {
    library: Library,
    summary: View,
    report: Option<ImportReport>,
    note: Option<String>,
    /// What the job did or why it could not, as a toast.
    said: Option<(Level, String)>,
}

enum JobKind {
    Load,
    Import,
    Edit,
}

struct Job {
    kind: JobKind,
    root: PathBuf,
    label: String,
    handle: JobHandle<Output>,
}

impl Job {
    /// The job for the UI with its progress now; a read of the index is not shown.
    fn view(&self) -> Option<JobView> {
        let import = matches!(self.kind, JobKind::Import);
        (!matches!(self.kind, JobKind::Load)).then(|| JobView {
            id: JOB_ID,
            label: self.label.clone(),
            fraction: import.then(|| self.handle.progress()),
            cancellable: import,
        })
    }
}

#[derive(Default)]
struct Slot {
    /// Where the library lives; `None` for a project library without a saved document.
    root: Option<PathBuf>,
    lib: Option<Library>,
    job: Option<Job>,
    summary: Option<View>,
    /// The brief inputs of the library, kept while a job holds the library.
    brief: Option<Brief>,
    /// The style references of the library, kept while a job holds it.
    styles: Styles,
}

/// The style folders of a library with what each brought, and all the references.
#[derive(Default)]
struct Styles {
    sources: Vec<StyleSourceView>,
    models: usize,
    textures: usize,
}

/// What a picked folder or list of files is for.
#[derive(Clone)]
enum PickFor {
    /// Import into the library shown.
    Library,
    /// Import as style references with this licence, into the library of the project folder it
    /// was asked for, which may not be the one shown or open any more when the pick returns.
    Style {
        licence: String,
        root: Option<PathBuf>,
    },
}

/// What a row needs to find its picture.
struct Row {
    id: String,
    kind: Kind,
    file: Option<PathBuf>,
    thumb: Option<PathBuf>,
}

struct LoadItem {
    key: ThumbKey,
    kind: Kind,
    file: Option<PathBuf>,
    thumb: Option<PathBuf>,
}

type Loaded = Vec<(ThumbKey, Kind, Option<PathBuf>, Option<Pixels>)>;

/// Progress and cancel of a job as the library import sees them.
struct JobControl(JobCtx);

impl Control for JobControl {
    fn progress(&self, fraction: f32) {
        self.0.progress(fraction);
    }

    fn cancelled(&self) -> bool {
        self.0.check().is_err()
    }
}

pub struct LibraryService {
    io: Option<IoThread>,
    recent_dir: PathBuf,
    pins_file: PathBuf,
    slots: [Slot; 2],
    scope: Scope,
    query: Query,
    rev: u64,
    image_rev: u64,
    view: LibraryView,
    rows: Vec<Row>,
    thumbs: HashMap<ThumbKey, Thumb>,
    loads: Vec<JobHandle<Loaded>>,
    writes: Vec<(ThumbKey, JobHandle<()>)>,
    adds: Vec<JobHandle<Prepared>>,
    renders: VecDeque<RenderRequest>,
    tried: HashSet<ThumbKey>,
    pins: Vec<PathBuf>,
    pins_load: Option<JobHandle<Vec<PathBuf>>>,
    pins_save: Option<JobHandle<()>>,
    /// The brief of the project library while the Project scope is not shown, read without
    /// changing anything on disk: the root it was read for and its brief (`None` when the
    /// library is missing or cannot be read).
    read_brief: Option<(PathBuf, Option<Brief>)>,
    /// The read that fills `read_brief`.
    brief_read: Option<(PathBuf, JobHandle<Option<Brief>>)>,
    pickers: Vec<(PickFor, Receiver<Vec<PathBuf>>)>,
    toasts: Vec<(Level, String)>,
    dirty: bool,
}

fn slot_index(scope: Scope) -> usize {
    usize::from(scope == Scope::Project)
}

fn ui_scope(scope: Scope) -> LibScope {
    match scope {
        Scope::Global => LibScope::Global,
        Scope::Project => LibScope::Project,
    }
}

fn ui_kind(kind: Kind) -> LibKind {
    match kind {
        Kind::Model => LibKind::Model,
        Kind::Json => LibKind::Json,
        Kind::Texture => LibKind::Texture,
        Kind::Reference => LibKind::Reference,
        Kind::Lore => LibKind::Lore,
    }
}

fn kind_of(kind: LibKind) -> Kind {
    match kind {
        LibKind::Model => Kind::Model,
        LibKind::Json => Kind::Json,
        LibKind::Texture => Kind::Texture,
        LibKind::Reference => Kind::Reference,
        LibKind::Lore => Kind::Lore,
    }
}

/// Whether the renderer can open the file as a model.
pub fn renderable(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        RENDERABLE
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// The second line of a row.
fn detail_of(entry: &Entry) -> String {
    let size = size_text(entry.size);
    match &entry.meta {
        Some(Meta::Model(m)) => {
            let mut parts = Vec::new();
            if m.elements > 0 {
                parts.push(format!("{} elements", m.elements));
            }
            if m.meshes > 0 {
                parts.push(format!("{} meshes", m.meshes));
            }
            if m.voxels {
                parts.push("voxels".to_owned());
            }
            if m.textures > 0 {
                parts.push(format!("{} textures", m.textures));
            }
            parts.push(size);
            parts.join(", ")
        }
        Some(Meta::Texture(t)) => {
            let frames = if t.frames > 1 {
                format!(", {} frames", t.frames)
            } else {
                String::new()
            };
            format!("{} x {}{frames}, {size}", t.width, t.height)
        }
        Some(Meta::Json { class }) => format!("{}, {size}", class.name()),
        Some(Meta::Lore(l)) => format!("{} words, {size}", l.words),
        None => size,
    }
}

impl LibraryService {
    pub fn new(dirs: &Dirs) -> LibraryService {
        let io = match IoThread::spawn() {
            Ok(io) => Some(io),
            Err(error) => {
                log::error!("cannot start the library thread: {error}");
                None
            }
        };
        let global_root = dirs.data.join("library");
        let mut service = LibraryService {
            io,
            recent_dir: dirs.data.join("cache").join("thumbs"),
            pins_file: dirs.config.join(PINS_FILE),
            slots: [
                Slot {
                    root: Some(global_root),
                    ..Slot::default()
                },
                Slot::default(),
            ],
            scope: Scope::Global,
            query: Query::default(),
            rev: 1,
            image_rev: 0,
            view: LibraryView::default(),
            rows: Vec::new(),
            thumbs: HashMap::new(),
            loads: Vec::new(),
            writes: Vec::new(),
            adds: Vec::new(),
            renders: VecDeque::new(),
            tried: HashSet::new(),
            pins: Vec::new(),
            pins_load: None,
            pins_save: None,
            read_brief: None,
            brief_read: None,
            pickers: Vec::new(),
            toasts: Vec::new(),
            dirty: true,
        };
        let file = service.pins_file.clone();
        service.pins_load = service
            .io
            .as_ref()
            .map(|io| io.submit(move |_| Ok(read_pins(&file))));
        service
    }

    /// The thread that renders and loads pictures run on.
    pub fn io(&self) -> Option<&IoThread> {
        self.io.as_ref()
    }

    /// The view for the UI.
    pub fn view(&self) -> &LibraryView {
        &self.view
    }

    /// The jobs for the status bar.
    pub fn jobs(&self) -> Vec<JobView> {
        self.slots
            .iter()
            .filter_map(|slot| slot.job.as_ref())
            .filter_map(Job::view)
            .collect()
    }

    /// True while a job or a picture load runs.
    pub fn is_busy(&self) -> bool {
        self.slots.iter().any(|slot| slot.job.is_some())
            || !self.loads.is_empty()
            || !self.writes.is_empty()
            || !self.adds.is_empty()
            || !self.pickers.is_empty()
            || self.pins_load.is_some()
            || self.pins_save.is_some()
            || self.brief_read.is_some()
    }

    /// Stops a running import after its current file.
    pub fn cancel(&self) {
        for job in self.slots.iter().filter_map(|slot| slot.job.as_ref()) {
            if matches!(job.kind, JobKind::Import) {
                job.handle.cancel();
            }
        }
    }

    /// One frame of work: follows the document's folder, starts reads, collects results and
    /// rebuilds the view when something changed. Returns the messages for the user.
    pub fn step(&mut self, project_dir: Option<&Path>) -> Vec<(Level, String)> {
        self.follow_project(project_dir);
        self.read_project_brief();
        self.poll_pins();
        self.poll_read_brief();
        self.poll_pickers();
        self.poll_loads();
        self.poll_writes();
        for scope in [Scope::Global, Scope::Project] {
            self.poll_job(scope);
            self.ensure_loaded(scope);
        }
        if std::mem::take(&mut self.dirty) {
            self.rebuild();
        } else if self.view.job.is_some() {
            // The progress moves without a new view.
            self.view.job = self.slots[slot_index(self.scope)]
                .job
                .as_ref()
                .and_then(Job::view);
        }
        std::mem::take(&mut self.toasts)
    }

    fn slot(&mut self, scope: Scope) -> &mut Slot {
        &mut self.slots[slot_index(scope)]
    }

    fn follow_project(&mut self, project_dir: Option<&Path>) {
        let wanted = project_dir.map(|dir| dir.join(PROJECT_DIR));
        if self.slots[1].root == wanted {
            return;
        }
        // A job for the old folder is told to stop and its result is dropped when it arrives.
        if let Some(job) = &self.slots[1].job {
            job.handle.cancel();
        }
        self.slots[1] = Slot {
            root: wanted,
            ..Slot::default()
        };
        self.read_brief = None;
        if let Some((_, job)) = self.brief_read.take() {
            job.cancel();
        }
        if self.scope == Scope::Project && self.slots[1].root.is_none() {
            self.scope = Scope::Global;
        }
        self.dirty = true;
    }

    /// The brief of the project library of the open document, `None` without a document folder
    /// or a readable library. It follows the library when that is loaded and is otherwise read
    /// on the library thread without changing anything on disk.
    pub fn project_brief(&self) -> Option<&Brief> {
        let slot = &self.slots[1];
        slot.brief.as_ref().or_else(|| {
            self.read_brief
                .as_ref()
                .and_then(|(_, brief)| brief.as_ref())
        })
    }

    /// Starts the read-only read of the project brief when the library is not loaded and the
    /// brief of this folder was not read yet.
    fn read_project_brief(&mut self) {
        let slot = &self.slots[1];
        let Some(root) = slot.root.clone() else {
            return;
        };
        let read = self
            .read_brief
            .as_ref()
            .is_some_and(|(known, _)| *known == root)
            || self
                .brief_read
                .as_ref()
                .is_some_and(|(known, _)| *known == root);
        if read || slot.brief.is_some() || slot.lib.is_some() || slot.job.is_some() {
            return;
        }
        let Some(io) = &self.io else { return };
        let path = root.clone();
        let handle = io.submit(move |_| {
            Ok(Library::read(&path, Scope::Project)
                .ok()
                .map(|library| library.brief().clone()))
        });
        self.brief_read = Some((root, handle));
    }

    fn poll_read_brief(&mut self) {
        let Some((root, handle)) = self.brief_read.as_mut() else {
            return;
        };
        let Some(result) = handle.poll() else {
            return;
        };
        let root = root.clone();
        self.brief_read = None;
        // A folder that was left meanwhile has its own read.
        if self.slots[1].root.as_ref() == Some(&root) {
            self.read_brief = Some((root, result.unwrap_or(None)));
        }
    }

    /// Reads the index of a library that is wanted and not here.
    fn ensure_loaded(&mut self, scope: Scope) {
        let wanted = scope == Scope::Global || self.scope == Scope::Project;
        let slot = &self.slots[slot_index(scope)];
        let Some(root) = slot.root.clone() else {
            return;
        };
        if !wanted || slot.lib.is_some() || slot.job.is_some() {
            return;
        }
        let Some(io) = &self.io else { return };
        let load_root = root.clone();
        let handle = io.submit(move |_| {
            let (library, warning) = Library::open(&load_root, scope)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            let summary = library.view(RECENT_IN_SUMMARY);
            Ok(Output {
                library,
                summary,
                report: None,
                note: warning,
                said: None,
            })
        });
        self.slot(scope).job = Some(Job {
            kind: JobKind::Load,
            root,
            label: "Reading the library".to_owned(),
            handle,
        });
        self.dirty = true;
    }

    fn poll_job(&mut self, scope: Scope) {
        let Some(result) = self
            .slot(scope)
            .job
            .as_mut()
            .and_then(|job| job.handle.poll())
        else {
            return;
        };
        let Some(job) = self.slot(scope).job.take() else {
            return;
        };
        self.dirty = true;
        if self.slot(scope).root.as_ref() != Some(&job.root) {
            return;
        }
        match result {
            Ok(output) => self.accept(scope, output),
            Err(JobError::Cancelled) if matches!(job.kind, JobKind::Load) => {}
            Err(error) => {
                // The library was inside the job; the next frame reads it from disk again.
                log::warn!("library job failed: {error}");
                self.toasts
                    .push((Level::Error, format!("The library job failed: {error}")));
                if matches!(job.kind, JobKind::Load) {
                    // Do not retry a read that cannot work; the view stays empty and busy.
                    self.slot(scope).root = None;
                }
            }
        }
    }

    fn accept(&mut self, scope: Scope, output: Output) {
        let Output {
            library,
            summary,
            report,
            note,
            said,
        } = output;
        if let Some(note) = note {
            self.toasts.push((Level::Warn, note));
        }
        if let Some(report) = report {
            let level = if report.added.is_empty() && !report.failed.is_empty() {
                Level::Error
            } else {
                Level::Info
            };
            self.toasts.push((level, report.summary()));
            let notices = report.notices();
            for line in notices.iter().take(3) {
                self.toasts.push((Level::Info, line.clone()));
            }
            if notices.len() > 3 {
                self.toasts.push((
                    Level::Info,
                    format!("{} more notices are in the log.", notices.len() - 3),
                ));
            }
            for line in &notices {
                log::info!("library import: {line}");
            }
        }
        self.toasts.extend(said);
        // Pictures of entries that are gone or replaced are not valid any more.
        self.thumbs
            .retain(|key, _| !matches!(key, ThumbKey::Entry(s, id) if *s == scope && library.entry(id).is_none()));
        let styles = styles_of(&library);
        let slot = self.slot(scope);
        slot.styles = styles;
        slot.brief = Some(library.brief().clone());
        slot.lib = Some(library);
        slot.summary = Some(summary);
    }

    /// Applies a request of the UI. `UseTexture` and `PinReference` are the frontend's.
    pub fn apply(&mut self, request: LibraryRequest) {
        match request {
            LibraryRequest::ImportFolder => self.pick(true, PickFor::Library),
            LibraryRequest::ImportFiles => self.pick(false, PickFor::Library),
            LibraryRequest::Import(paths) => self.import(paths),
            LibraryRequest::SetScope(scope) => {
                let scope = match scope {
                    LibScope::Global => Scope::Global,
                    LibScope::Project => Scope::Project,
                };
                if scope == Scope::Project && self.slots[1].root.is_none() {
                    self.toasts.push((
                        Level::Info,
                        "Save the model first: the project library sits next to it.".to_owned(),
                    ));
                    return;
                }
                self.scope = scope;
                self.dirty = true;
            }
            LibraryRequest::Query { text, kind, tag } => {
                let query = Query {
                    text,
                    kind: kind.map(kind_of),
                    tag,
                };
                if query != self.query {
                    self.query = query;
                    self.dirty = true;
                }
            }
            LibraryRequest::Thumbs(ids) => self.want_thumbs(&ids),
            LibraryRequest::SetTags { id, tags } => {
                self.edit("Saving tags", true, move |library| {
                    library.set_tags(&id, &tags);
                    None
                });
            }
            LibraryRequest::Remove(id) => {
                let scope = self.scope;
                self.thumbs.remove(&ThumbKey::Entry(scope, id.clone()));
                self.edit("Removing from the library", true, move |library| {
                    library.remove(&id);
                    None
                });
            }
            LibraryRequest::SetBrief(inputs) => {
                let brief = brief_of(&inputs);
                // The dialog does not edit the style folders: they stay as they are.
                self.edit("Saving the brief", true, move |library| {
                    let style_sources = library.brief().style_sources.clone();
                    library.set_brief(Brief {
                        style_sources,
                        ..brief
                    });
                    None
                });
            }
            LibraryRequest::AddStyleFolder { licence } => {
                let root = self.slots[1].root.clone();
                self.pick(true, PickFor::Style { licence, root });
            }
            LibraryRequest::RescanStyle(path) => {
                let scope = self.scope;
                self.import_job(
                    scope,
                    "Rescanning style references",
                    "rescan",
                    move |library, control| library.rescan_style(&path, control),
                );
            }
            LibraryRequest::RemoveStyleSource(path) => {
                // The library saves the index itself.
                self.edit("Removing style references", false, move |library| {
                    Some(match library.remove_style_source(&path) {
                        Ok(1) => (Level::Info, "Removed 1 style reference.".to_owned()),
                        Ok(count) => (Level::Info, format!("Removed {count} style references.")),
                        Err(error) => (Level::Error, error.to_string()),
                    })
                });
            }
            LibraryRequest::SetRole { id, style } => {
                let role = if style { Role::Style } else { Role::Work };
                self.edit("Saving the entry", true, move |library| {
                    library.set_role(&id, role);
                    None
                });
            }
            LibraryRequest::TogglePin(path) => self.toggle_pin(path),
            LibraryRequest::UseTexture(_) | LibraryRequest::PinReference(_) => {}
        }
    }

    fn pick(&mut self, folder: bool, goal: PickFor) {
        // A style folder lies in the project, so the dialog starts there.
        let start = match &goal {
            PickFor::Style { root, .. } => {
                root.as_deref().and_then(Path::parent).map(Path::to_owned)
            }
            PickFor::Library => None,
        };
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("cuboid-dialog".to_owned())
            .spawn(move || {
                let paths = pollster::block_on(async {
                    let mut dialog = rfd::AsyncFileDialog::new();
                    if let Some(start) = &start {
                        dialog = dialog.set_directory(start);
                    }
                    if folder {
                        dialog
                            .pick_folder()
                            .await
                            .map(|h| vec![h.path().to_owned()])
                    } else {
                        dialog
                            .add_filter("Library files", &IMPORT_FILTER)
                            .pick_files()
                            .await
                            .map(|list| list.iter().map(|h| h.path().to_owned()).collect())
                    }
                });
                let _ = tx.send(paths.unwrap_or_default());
            });
        match spawned {
            Ok(_) => self.pickers.push((goal, rx)),
            Err(error) => {
                log::error!("cannot start the file dialog: {error}");
                self.toasts
                    .push((Level::Error, "Cannot open the file dialog.".to_owned()));
            }
        }
    }

    fn poll_pickers(&mut self) {
        let mut chosen = Vec::new();
        self.pickers.retain(|(goal, rx)| match rx.try_recv() {
            Ok(paths) => {
                chosen.push((goal.clone(), paths));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for (goal, paths) in chosen {
            match goal {
                PickFor::Library => self.import(paths),
                PickFor::Style { licence, root } => {
                    let Some(folder) = paths.into_iter().next() else {
                        continue;
                    };
                    if self.slots[1].root != root {
                        self.toasts.push((
                            Level::Info,
                            "The project changed while you picked a folder; add it again."
                                .to_owned(),
                        ));
                        continue;
                    }
                    let options = ImportOptions {
                        licence,
                        ..ImportOptions::default()
                    };
                    // Whatever library is shown now, the folder belongs to the project's.
                    self.import_job(
                        Scope::Project,
                        "Importing style references",
                        "import",
                        move |library, control| library.import_style(&folder, &options, control),
                    );
                }
            }
        }
    }

    /// Starts the import of files and folders into the library shown.
    pub fn import(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let label = if paths.len() == 1 {
            "Importing into the library".to_owned()
        } else {
            format!("Importing {} items into the library", paths.len())
        };
        let scope = self.scope;
        self.import_job(scope, &label, "import", move |library, control| {
            Ok(library.import(&paths, &ImportOptions::default(), control))
        });
    }

    /// Runs `work`, an import into the library of `scope`, on the library thread with progress and
    /// cancel. A refusal is told as a toast, "The `verb` failed: ..."; the library comes back
    /// either way.
    fn import_job(
        &mut self,
        scope: Scope,
        label: &str,
        verb: &str,
        work: impl FnOnce(&mut Library, &JobControl) -> Result<ImportReport, LibraryError>
        + Send
        + 'static,
    ) {
        let Some(io) = &self.io else { return };
        let slot = &mut self.slots[slot_index(scope)];
        let (Some(root), Some(_), None) = (slot.root.clone(), &slot.lib, &slot.job) else {
            self.toasts.push((
                Level::Info,
                "The library is busy; try again in a moment.".to_owned(),
            ));
            return;
        };
        let Some(mut library) = slot.lib.take() else {
            return;
        };
        let verb = verb.to_owned();
        let handle = io.submit(move |ctx| {
            let control = JobControl(ctx.clone());
            let (report, said) = match work(&mut library, &control) {
                Ok(report) => (Some(report), None),
                Err(error) => (
                    None,
                    Some((Level::Error, format!("The {verb} failed: {error}"))),
                ),
            };
            let note = brief_md_note(&library);
            let summary = library.view(RECENT_IN_SUMMARY);
            Ok(Output {
                library,
                summary,
                report,
                note,
                said,
            })
        });
        slot.job = Some(Job {
            kind: JobKind::Import,
            root,
            label: label.to_owned(),
            handle,
        });
        self.dirty = true;
    }

    /// Runs a change of the index on the library thread and saves it, unless `save` is false
    /// because the change saves it itself. What the change returns is told as a toast. A change
    /// while a job holds the library is refused with a toast too.
    fn edit(
        &mut self,
        label: &str,
        save: bool,
        change: impl FnOnce(&mut Library) -> Option<(Level, String)> + Send + 'static,
    ) {
        let scope = self.scope;
        let Some(io) = &self.io else { return };
        let slot = &mut self.slots[slot_index(scope)];
        let (Some(root), Some(_), None) = (slot.root.clone(), &slot.lib, &slot.job) else {
            self.toasts.push((
                Level::Info,
                "The library is busy; try again in a moment.".to_owned(),
            ));
            return;
        };
        let Some(mut library) = slot.lib.take() else {
            return;
        };
        let handle = io.submit(move |_| {
            let said = change(&mut library);
            let saved = if save { library.save() } else { Ok(()) };
            let note = match saved {
                Ok(()) => brief_md_note(&library),
                Err(error) => Some(format!("The library could not be saved: {error}")),
            };
            let summary = library.view(RECENT_IN_SUMMARY);
            Ok(Output {
                library,
                summary,
                report: None,
                note,
                said,
            })
        });
        slot.job = Some(Job {
            kind: JobKind::Edit,
            root,
            label: label.to_owned(),
            handle,
        });
        self.dirty = true;
    }

    /// The stored copy of a texture of the shown library and the name it had.
    fn texture_source(&self, id: &str) -> Option<(PathBuf, String)> {
        let slot = &self.slots[slot_index(self.scope)];
        let library = slot.lib.as_ref()?;
        let entry = library.entry(id)?;
        if entry.kind != Kind::Texture {
            return None;
        }
        Some((library.file_path(entry)?, entry.stem().to_owned()))
    }

    /// Reads a texture of the library on the library thread, to be added to the document by
    /// the frame that takes it from [`LibraryService::take_prepared`].
    fn prepare_texture(&mut self, id: &str) {
        let Some((file, stem)) = self.texture_source(id) else {
            self.toasts
                .push((Level::Info, "That texture is not available.".to_owned()));
            return;
        };
        let Some(io) = &self.io else { return };
        self.adds.push(io.submit(move |ctx| {
            let bytes = match DiskFiles.read(&file) {
                Ok(Some(bytes)) => bytes,
                _ => {
                    return Err(JobError::Failed(
                        "the stored texture cannot be read".to_owned(),
                    ));
                }
            };
            ctx.check()?;
            let image = decode_file(&bytes, ImageFileKind::Other)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            Ok(Prepared {
                stem,
                width: image.w,
                height: image.h,
                rgba: image.rgba().to_vec(),
            })
        }));
    }

    /// Textures read and ready to be added, and the reasons for the ones that failed.
    pub fn take_prepared(&mut self) -> Vec<Result<Prepared, String>> {
        let mut done = Vec::new();
        self.adds.retain_mut(|handle| match handle.poll() {
            None => true,
            Some(result) => {
                done.push(result.map_err(|error| error.to_string()));
                false
            }
        });
        done
    }

    // Pictures.

    fn key(&self, id: &str) -> ThumbKey {
        ThumbKey::Entry(self.scope, id.to_owned())
    }

    fn thumb_state(&self, key: &ThumbKey) -> ThumbState {
        match self.thumbs.get(key) {
            Some(Thumb::Ready(image)) => ThumbState::Ready(image.clone()),
            Some(Thumb::Missing) => ThumbState::Missing,
            Some(Thumb::Loading) | None => ThumbState::Unknown,
        }
    }

    fn want_thumbs(&mut self, ids: &[String]) {
        let mut items = Vec::new();
        for id in ids {
            let key = self.key(id);
            if self.thumbs.contains_key(&key) {
                continue;
            }
            let Some(row) = self.rows.iter().find(|row| row.id == *id) else {
                continue;
            };
            let has_picture = matches!(row.kind, Kind::Texture | Kind::Model | Kind::Reference);
            if !has_picture {
                self.thumbs.insert(key, Thumb::Missing);
                self.dirty = true;
                continue;
            }
            items.push(LoadItem {
                key: key.clone(),
                kind: row.kind,
                file: row.file.clone(),
                thumb: row.thumb.clone(),
            });
            self.thumbs.insert(key, Thumb::Loading);
        }
        self.load(items);
    }

    /// Pictures of recent files, in order. Starts reading the ones not read yet.
    pub fn recent_thumbs(&mut self, paths: &[PathBuf]) -> Vec<Option<ImageData>> {
        let mut items = Vec::new();
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let key = ThumbKey::Recent(path.clone());
            match self.thumbs.get(&key) {
                Some(Thumb::Ready(image)) => out.push(Some(image.clone())),
                Some(_) => out.push(None),
                None => {
                    out.push(None);
                    items.push(LoadItem {
                        key: key.clone(),
                        kind: Kind::Model,
                        file: Some(path.clone()),
                        thumb: Some(recent_thumb_path(&self.recent_dir, path)),
                    });
                    self.thumbs.insert(key, Thumb::Loading);
                }
            }
        }
        self.load(items);
        out
    }

    fn load(&mut self, items: Vec<LoadItem>) {
        if items.is_empty() {
            return;
        }
        let Some(io) = &self.io else { return };
        let handle = io.submit(move |ctx| {
            let mut done: Loaded = Vec::with_capacity(items.len());
            for item in items {
                ctx.check()?;
                let pixels = match item.kind {
                    Kind::Reference => item.file.as_deref().and_then(reference_thumb),
                    _ => item.thumb.as_deref().and_then(read_thumb),
                };
                // A model that is not there cannot be drawn.
                let file = item.file.filter(|file| pixels.is_some() || file.is_file());
                done.push((item.key, item.kind, file, pixels));
            }
            Ok(done)
        });
        self.loads.push(handle);
    }

    fn poll_loads(&mut self) {
        let mut finished = Vec::new();
        self.loads.retain_mut(|handle| match handle.poll() {
            None => true,
            Some(result) => {
                finished.push(result);
                false
            }
        });
        for result in finished {
            let Ok(done) = result else { continue };
            for (key, kind, file, pixels) in done {
                self.dirty = true;
                match pixels {
                    Some(px) => {
                        self.image_rev += 1;
                        let image = ImageData {
                            size: [px.w as usize, px.h as usize],
                            rgba: Arc::from(px.rgba),
                            rev: self.image_rev,
                        };
                        self.thumbs.insert(key, Thumb::Ready(image));
                    }
                    None => {
                        self.thumbs.insert(key.clone(), Thumb::Missing);
                        if kind == Kind::Model
                            && let Some(source) = file
                        {
                            self.queue_render(key, source);
                        }
                    }
                }
            }
        }
    }

    fn out_path(&self, key: &ThumbKey) -> Option<PathBuf> {
        match key {
            ThumbKey::Recent(path) => Some(recent_thumb_path(&self.recent_dir, path)),
            ThumbKey::Entry(scope, id) => self.slots[slot_index(*scope)]
                .lib
                .as_ref()
                .and_then(|lib| lib.thumb_path(id)),
        }
    }

    fn queue_render(&mut self, key: ThumbKey, source: PathBuf) {
        if !renderable(&source)
            || self.renders.len() >= MAX_RENDERS
            || !self.tried.insert(key.clone())
        {
            return;
        }
        if let Some(out) = self.out_path(&key) {
            self.renders.push_back(RenderRequest { key, source, out });
        }
    }

    /// The next model to draw, if any.
    pub fn take_render(&mut self) -> Option<RenderRequest> {
        self.renders.pop_front()
    }

    /// True while models wait to be drawn.
    pub fn has_renders(&self) -> bool {
        !self.renders.is_empty()
    }

    /// A model file was saved or exported: draw its picture again.
    pub fn thumbnail_saved(&mut self, path: &Path) {
        let key = ThumbKey::Recent(path.to_owned());
        self.tried.remove(&key);
        self.renders.retain(|request| request.key != key);
        self.queue_render(key, path.to_owned());
    }

    /// The rig drew a picture: write it, then read it back. `None` when the model could not be
    /// opened or drawn.
    pub fn render_done(&mut self, request: RenderRequest, image: Option<Image>) {
        let Some(image) = image else {
            self.thumbs.insert(request.key, Thumb::Missing);
            self.dirty = true;
            return;
        };
        let Some(io) = &self.io else { return };
        let out = request.out;
        let handle = io.submit(move |_| {
            let png = encode_png(&image).map_err(|e| JobError::Failed(e.to_string()))?;
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| JobError::Failed(e.to_string()))?;
            }
            cuboid_io::write_atomic(&out, &png).map_err(|e| JobError::Failed(e.to_string()))
        });
        self.writes.push((request.key, handle));
    }

    fn poll_writes(&mut self) {
        let mut finished = Vec::new();
        self.writes.retain_mut(|(key, handle)| match handle.poll() {
            None => true,
            Some(result) => {
                finished.push((key.clone(), result));
                false
            }
        });
        for (key, result) in finished {
            match result {
                Ok(()) => {
                    // Read the new picture the usual way.
                    let (kind, file) = match &key {
                        ThumbKey::Recent(path) => (Kind::Model, Some(path.clone())),
                        ThumbKey::Entry(..) => (Kind::Model, None),
                    };
                    let thumb = self.out_path(&key);
                    self.thumbs.insert(key.clone(), Thumb::Loading);
                    self.load(vec![LoadItem {
                        key,
                        kind,
                        file,
                        thumb,
                    }]);
                }
                Err(error) => {
                    log::warn!("cannot write a picture: {error}");
                    self.thumbs.insert(key, Thumb::Missing);
                    self.dirty = true;
                }
            }
        }
    }

    // Pinned recent files.

    fn toggle_pin(&mut self, path: PathBuf) {
        if let Some(at) = self.pins.iter().position(|p| *p == path) {
            self.pins.remove(at);
        } else {
            self.pins.push(path);
        }
        self.dirty = true;
        self.save_pins();
    }

    fn save_pins(&mut self) {
        let Some(io) = &self.io else { return };
        let file = self.pins_file.clone();
        let pins = self.pins.clone();
        self.pins_save = Some(io.submit(move |_| {
            write_pins(&file, &pins).map_err(|e| JobError::Failed(e.to_string()))
        }));
    }

    fn poll_pins(&mut self) {
        if let Some(result) = self.pins_load.as_mut().and_then(JobHandle::poll) {
            self.pins_load = None;
            if let Ok(mut pins) = result {
                // Pins toggled before the file arrived stay.
                pins.retain(|p| !self.pins.contains(p));
                self.pins.extend(pins);
                self.dirty = true;
            }
        }
        if let Some(result) = self.pins_save.as_mut().and_then(JobHandle::poll) {
            self.pins_save = None;
            if let Err(error) = result {
                log::warn!("cannot save the pinned files: {error}");
            }
        }
    }

    // The view.

    fn rebuild(&mut self) {
        let scope = self.scope;
        let slot = &self.slots[slot_index(scope)];
        let mut view = LibraryView {
            scope: ui_scope(scope),
            project_available: self.slots[1].root.is_some(),
            busy: slot.lib.is_none() || slot.job.is_some(),
            job: slot.job.as_ref().and_then(Job::view),
            clients: self.slots[1]
                .root
                .as_deref()
                .and_then(Path::parent)
                .map(clients_of),
            pinned: Arc::from(self.pins.clone()),
            ..LibraryView::default()
        };
        if let Some(summary) = &slot.summary {
            view.counts = summary.counts;
            view.total = summary.total;
            view.tags = Arc::from(summary.tags.clone());
            view.brief = brief_card(summary, &slot.styles);
            view.suggestions = summary
                .suggestions
                .iter()
                .map(|s| SuggestionView {
                    title: s.title.clone(),
                    detail: s.detail.clone(),
                    action: suggestion_action(s, slot.lib.as_ref()),
                })
                .collect();
        }
        let mut entries = Vec::new();
        let mut rows = Vec::new();
        let mut matches = 0;
        if let Some(library) = &slot.lib {
            let found = library.search(&self.query, usize::MAX);
            matches = found.len();
            for entry in found.into_iter().take(MAX_ROWS) {
                let key = ThumbKey::Entry(scope, entry.id.clone());
                entries.push(LibEntryView {
                    id: entry.id.clone(),
                    kind: ui_kind(entry.kind),
                    name: entry.name.clone(),
                    detail: detail_of(entry),
                    tags: entry.tags.clone(),
                    style: entry.role == Role::Style,
                    thumb: self.thumb_state(&key),
                });
                rows.push(Row {
                    id: entry.id.clone(),
                    kind: entry.kind,
                    file: library.file_path(entry),
                    thumb: library.thumb_path(&entry.id),
                });
            }
            view.counts = library.counts();
            view.total = library.entries().len();
        } else if slot.root.is_none() {
            // No library to read: nothing is coming.
            view.busy = false;
        }
        self.rows = rows;
        view.matches = matches;
        view.entries = Arc::from(entries);
        self.rev += 1;
        view.rev = self.rev;
        self.view = view;
    }
}

/// The button of a suggestion: a lore name without a model starts a new item, a model with an
/// odd texel density opens. A texture has nothing to do on the start screen.
fn suggestion_action(
    suggestion: &Suggestion,
    library: Option<&Library>,
) -> Option<SuggestionAction> {
    match suggestion.kind {
        SuggestionKind::LoreWithoutModel => Some(SuggestionAction {
            label: "Create".to_owned(),
            goal: SuggestionGoal::New(Template::Item),
        }),
        SuggestionKind::DensityMismatch => {
            let library = library?;
            let entry = library
                .entries()
                .iter()
                .find(|e| e.id == suggestion.entry && e.kind == Kind::Model)?;
            Some(SuggestionAction {
                label: "Open".to_owned(),
                goal: SuggestionGoal::Open(library.file_path(entry)?),
            })
        }
        SuggestionKind::TextureOutsidePalette => None,
    }
}

/// Writes `brief.md` next to the saved index when its text changed. The text of a failure, for
/// the user.
fn brief_md_note(library: &Library) -> Option<String> {
    if !library.root().join(INDEX_FILE).is_file() {
        return None;
    }
    library
        .write_brief_md()
        .err()
        .map(|error| format!("brief.md could not be written: {error}"))
}

/// The brief as the dialog sends it.
fn brief_of(inputs: &BriefInputs) -> Brief {
    Brief {
        project_name: inputs.project_name.clone(),
        mc_version: inputs.mc_version.clone(),
        style_tags: inputs.style_tags.clone(),
        goal: inputs.goal.clone(),
        platform: Platform::from_id(&inputs.platform),
        namespace: inputs.namespace.clone(),
        texel_density: inputs.texel_density,
        texture_size: inputs.texture_size,
        palette: inputs.palette.clone(),
        palette_strict: inputs.palette_strict,
        max_elements: inputs.max_elements,
        naming: Naming {
            case: NameCase::from_id(&inputs.name_case),
            prefix: inputs.name_prefix.clone(),
        },
        display_preset: inputs
            .display_preset
            .and_then(|preset| LibDisplayPreset::from_id(preset.id())),
        rules: inputs.rules.clone(),
        style_sources: Vec::new(),
    }
}

/// The brief as the dialog shows it.
fn inputs_of(brief: &Brief) -> BriefInputs {
    BriefInputs {
        project_name: brief.project_name.clone(),
        mc_version: brief.mc_version.clone(),
        style_tags: brief.style_tags.clone(),
        goal: brief.goal.clone(),
        platform: brief.platform.id().to_owned(),
        namespace: brief.namespace.clone(),
        texel_density: brief.texel_density,
        texture_size: brief.texture_size,
        palette: brief.palette.clone(),
        palette_strict: brief.palette_strict,
        max_elements: brief.max_elements,
        name_case: brief.naming.case.id().to_owned(),
        name_prefix: brief.naming.prefix.clone(),
        display_preset: brief.display_preset.and_then(|preset| {
            UiDisplayPreset::ALL
                .into_iter()
                .find(|p| p.id() == preset.id())
        }),
        rules: brief.rules.clone(),
    }
}

fn brief_card(summary: &View, styles: &Styles) -> BriefCardView {
    let brief = &summary.brief;
    BriefCardView {
        inputs: inputs_of(&brief.brief),
        observed_palette: brief.palette.clone(),
        observed_density: brief.density,
        models: brief.models,
        textures: brief.textures,
        project: summary.scope == Scope::Project,
        style_sources: styles.sources.clone(),
        style_models: styles.models,
        style_textures: styles.textures,
    }
}

/// The lines that start the MCP server of this program on the project folder in each client, and
/// the command that tries it. A folder that `cuboid mcp` would refuse gets the reason and no
/// lines; the text check is the one `cuboid mcp` runs first, so no disk is read here.
fn clients_of(folder: &Path) -> ClientsView {
    let mut view = ClientsView {
        folder: folder.display().to_string(),
        refused: Root::refusal(folder),
        desktop_config: DESKTOP_CONFIG.to_owned(),
        ..ClientsView::default()
    };
    if view.refused.is_some() {
        return view;
    }
    let lines = client_lines(folder);
    view.claude_code = lines.claude_code;
    view.claude_desktop = lines.claude_desktop;
    view.codex_command = lines.codex_command;
    view.codex_toml = lines.codex_toml;
    view.check = lines.check;
    view
}

/// The style folders of a library, each with the models and textures that came from it.
fn styles_of(library: &Library) -> Styles {
    let mut styles = Styles {
        sources: library
            .brief()
            .style_sources
            .iter()
            .map(|source| StyleSourceView {
                path: source.path.clone(),
                licence: source.licence.clone(),
                ..StyleSourceView::default()
            })
            .collect(),
        ..Styles::default()
    };
    for entry in library
        .entries()
        .iter()
        .filter(|entry| entry.role == Role::Style)
    {
        let source = styles
            .sources
            .iter_mut()
            .find(|source| within(&entry.source, &source.path));
        match entry.kind {
            Kind::Model => {
                styles.models += 1;
                if let Some(source) = source {
                    source.models += 1;
                }
            }
            Kind::Texture => {
                styles.textures += 1;
                if let Some(source) = source {
                    source.textures += 1;
                }
            }
            _ => {}
        }
    }
    styles
}

fn read_pins(file: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .map(|list| list.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn write_pins(file: &Path, pins: &[PathBuf]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let list: Vec<String> = pins.iter().map(|p| p.display().to_string()).collect();
    let text = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    cuboid_io::write_atomic(file, &text)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use cuboid_mcp::snippets;
    use cuboid_model::image::RgbaBuf;

    use super::*;

    struct Rig {
        service: LibraryService,
        toasts: Vec<(Level, String)>,
        /// Dropped after the service, whose jobs may still write into it.
        dir: tempfile::TempDir,
    }

    impl Rig {
        fn new() -> Rig {
            let dir = tempfile::tempdir().expect("temp dir");
            let service = LibraryService::new(&Dirs::under(dir.path()));
            let mut rig = Rig {
                dir,
                service,
                toasts: Vec::new(),
            };
            rig.settle(None);
            rig
        }

        /// Steps until nothing is running.
        fn settle(&mut self, folder: Option<&Path>) {
            let start = Instant::now();
            loop {
                self.toasts.extend(self.service.step(folder));
                if !self.service.is_busy() && !self.service.view().busy {
                    return;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(20),
                    "the library never settled"
                );
                thread::sleep(Duration::from_millis(2));
            }
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir.path().join("src").join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(&path, bytes).expect("write");
            path
        }
    }

    fn png(color: [u8; 4]) -> Vec<u8> {
        let mut buf = RgbaBuf::new(4, 4);
        for texel in buf.pixels.chunks_exact_mut(4) {
            texel.copy_from_slice(&color);
        }
        encode_png(&Image::from_rgba(buf)).expect("encodes")
    }

    #[test]
    fn an_import_shows_up_in_the_view_with_progress_and_a_toast() {
        let mut rig = Rig::new();
        assert!(!rig.service.view().busy && rig.service.view().total == 0);
        let a = rig.write("stone.png", &png([9, 9, 9, 255]));
        let b = rig.write("story.md", b"# Story\n## Weapons\n- Gun\n");
        rig.service.apply(LibraryRequest::Import(vec![a, b]));
        let jobs = rig.service.jobs();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].cancellable && jobs[0].id == JOB_ID);
        rig.settle(None);
        let view = rig.service.view();
        assert_eq!(view.total, 2);
        assert_eq!(view.counts, [0, 1, 0, 0, 1]);
        assert_eq!(view.entries.len(), 2);
        assert!(
            view.entries
                .iter()
                .any(|e| e.name == "stone.png" && e.detail.starts_with("4 x 4"))
        );
        assert_eq!(view.suggestions.len(), 1, "Gun has no model");
        assert!(
            rig.toasts
                .iter()
                .any(|(_, text)| text.starts_with("Imported 2 files")),
            "{:?}",
            rig.toasts
        );
        assert!(rig.service.jobs().is_empty());
    }

    #[test]
    fn a_queued_import_can_be_cancelled_and_the_library_comes_back_from_disk() {
        let mut rig = Rig::new();
        let file = rig.write("note.md", b"# note\n");
        // Keep the library thread busy so the import is still queued when it is cancelled.
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let _gate = rig.service.io().expect("thread").submit(move |_| {
            let _ = gate_rx.recv_timeout(Duration::from_secs(10));
            Ok(())
        });
        rig.service.apply(LibraryRequest::Import(vec![file]));
        rig.service.cancel();
        gate_tx.send(()).expect("gate");
        rig.settle(None);
        let view = rig.service.view();
        assert!(!view.busy, "the index was read again");
        assert_eq!(view.total, 0);
        // And it works afterwards.
        let file = rig.write("again.md", b"# again\n");
        rig.service.apply(LibraryRequest::Import(vec![file]));
        rig.settle(None);
        assert_eq!(rig.service.view().total, 1);
    }

    #[test]
    fn search_tags_remove_and_brief_go_through_jobs() {
        let mut rig = Rig::new();
        let a = rig.write("gun.md", b"# gun\n");
        let b = rig.write("axe.md", b"# axe\n");
        rig.service.apply(LibraryRequest::Import(vec![a, b]));
        rig.settle(None);
        rig.service.apply(LibraryRequest::Query {
            text: "gun".to_owned(),
            kind: None,
            tag: None,
        });
        rig.settle(None);
        let view = rig.service.view();
        assert_eq!((view.entries.len(), view.matches, view.total), (1, 1, 2));
        let id = view.entries[0].id.clone();

        rig.service.apply(LibraryRequest::SetTags {
            id: id.clone(),
            tags: vec!["weapon".to_owned(), "pixel".to_owned()],
        });
        rig.settle(None);
        assert_eq!(rig.service.view().entries[0].tags, ["weapon", "pixel"]);
        assert_eq!(rig.service.view().tags.len(), 2);

        rig.service
            .apply(LibraryRequest::SetBrief(Box::new(BriefInputs {
                project_name: "Ink".to_owned(),
                mc_version: "1.21.6".to_owned(),
                style_tags: vec!["pixel".to_owned()],
                goal: "Splat weapons".to_owned(),
                platform: "oraxen".to_owned(),
                namespace: "splat".to_owned(),
                texel_density: Some(16.0),
                texture_size: Some(32),
                palette: vec![[1, 2, 3]],
                palette_strict: true,
                max_elements: Some(64),
                name_case: "snake".to_owned(),
                name_prefix: "ink_".to_owned(),
                display_preset: Some(UiDisplayPreset::ItemHandheld),
                rules: vec!["No gradients".to_owned()],
            })));
        rig.settle(None);
        let inputs = &rig.service.view().brief.inputs;
        assert_eq!(inputs.project_name, "Ink");
        assert_eq!(
            (inputs.platform.as_str(), inputs.name_case.as_str()),
            ("oraxen", "snake")
        );
        assert_eq!(inputs.display_preset, Some(UiDisplayPreset::ItemHandheld));
        assert_eq!(inputs.palette, [[1, 2, 3]]);
        // The brief is written for agents next to the index, and not again when it is the same.
        let md = rig.dir.path().join("data").join("library").join("brief.md");
        let text = std::fs::read_to_string(&md).expect("brief.md");
        assert!(text.contains("| Namespace | splat |"), "{text}");
        assert!(text.contains("- No gradients"), "{text}");

        rig.service.apply(LibraryRequest::Remove(id));
        rig.settle(None);
        assert_eq!(rig.service.view().total, 1);
        assert!(
            rig.service.view().entries.is_empty(),
            "the query still says gun"
        );

        // The edits were saved: a new service reads them.
        let mut other = LibraryService::new(&Dirs::under(rig.dir.path()));
        let start = Instant::now();
        while other.view().busy || other.view().total == 0 {
            other.step(None);
            assert!(start.elapsed() < Duration::from_secs(20));
            thread::sleep(Duration::from_millis(2));
        }
        let again = other.view();
        assert_eq!(
            (again.total, again.brief.inputs.project_name.as_str()),
            (1, "Ink")
        );
    }

    #[test]
    fn the_project_library_sits_next_to_the_document() {
        let mut rig = Rig::new();
        let project = rig.dir.path().join("work");
        std::fs::create_dir_all(&project).expect("dir");
        assert!(!rig.service.view().project_available);
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(None);
        assert_eq!(
            rig.service.view().scope,
            LibScope::Global,
            "no saved document, no project library"
        );

        rig.settle(Some(&project));
        assert!(rig.service.view().project_available);
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(Some(&project));
        assert_eq!(rig.service.view().scope, LibScope::Project);
        let file = rig.write("note.md", b"# note\n");
        rig.service.apply(LibraryRequest::Import(vec![file]));
        rig.settle(Some(&project));
        assert_eq!(rig.service.view().total, 1);
        assert!(project.join(PROJECT_DIR).join("library.json").is_file());
        // The global library stayed empty.
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Global));
        rig.settle(Some(&project));
        assert_eq!(rig.service.view().total, 0);
        // Closing the document falls back to the global library.
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(Some(&project));
        rig.settle(None);
        assert_eq!(rig.service.view().scope, LibScope::Global);
    }

    #[test]
    fn the_view_carries_the_lines_that_connect_a_client_to_the_project_folder() {
        let mut rig = Rig::new();
        assert_eq!(
            rig.service.view().clients,
            None,
            "no saved document, no folder"
        );
        let project = rig.dir.path().join("work");
        std::fs::create_dir_all(&project).expect("dir");
        rig.settle(Some(&project));
        let exe = std::env::current_exe().expect("the running program");
        let (exe_text, folder) = (exe.display().to_string(), project.display().to_string());
        let clients = rig.service.view().clients.clone().expect("a folder");
        // The server works on the project folder, not on the library inside it.
        assert_eq!(clients.folder, folder);
        assert_eq!(clients.refused, None);
        assert_eq!(clients.desktop_config, DESKTOP_CONFIG);
        assert_eq!(
            clients.claude_code,
            format!(
                "claude mcp add --transport stdio cuboid \"--\" \"{exe_text}\" mcp --root \"{folder}\""
            )
        );
        // A release build on Windows is a GUI program: the check runs its console twin.
        let console =
            cfg!(all(windows, not(debug_assertions))).then(|| exe.with_file_name("cuboid.com"));
        let lines = snippets(&exe, &project, console.as_deref());
        assert_eq!(clients.claude_desktop, lines.claude_desktop);
        assert_eq!(clients.codex_command, lines.codex_command);
        assert_eq!(clients.codex_toml, lines.codex_toml);
        assert_eq!(clients.check, lines.check);
        let checked = console.as_deref().unwrap_or(&exe).display();
        assert!(
            clients
                .check
                .ends_with(&format!("\"{checked}\" mcp --root \"{folder}\" --check"))
        );
        // The Project scope does not change the folder.
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(Some(&project));
        assert_eq!(rig.service.view().clients.as_ref(), Some(&clients));
        // Closing the document takes the lines away.
        rig.settle(None);
        assert_eq!(rig.service.view().clients, None);
    }

    #[test]
    fn a_folder_that_cuboid_mcp_would_refuse_gets_the_reason_and_no_lines() {
        let drive_root = Path::new(if cfg!(windows) { r"D:\" } else { "/" });
        for (folder, word) in [
            (drive_root, "root"),
            // The parent of a project library that was opened by a relative path.
            (Path::new(""), "absolute"),
            (Path::new("models/lamp"), "absolute"),
        ] {
            let view = clients_of(folder);
            let why = view.refused.as_deref().expect("a reason");
            assert!(why.contains(word), "{folder:?}: {why}");
            assert_eq!(Root::refusal(folder).as_deref(), Some(why));
            assert_eq!(view.folder, folder.display().to_string());
            let lines = [
                &view.claude_code,
                &view.claude_desktop,
                &view.codex_command,
                &view.codex_toml,
                &view.check,
            ];
            assert!(lines.iter().all(|line| line.is_empty()), "{folder:?}");
        }
        if cfg!(windows) {
            let share = clients_of(Path::new(r"\\nas\models\lamp"));
            let why = share.refused.expect("a share is refused");
            assert!(why.contains("network"), "{why}");
        }
    }

    fn lamp(height: u32) -> Vec<u8> {
        format!(r#"{{"elements": [{{"from": [0, 0, 0], "to": [4, {height}, 4]}}]}}"#).into_bytes()
    }

    /// A project with a folder of one model and one texture, its library shown.
    fn project_with_lamps(rig: &mut Rig) -> (PathBuf, PathBuf) {
        let project = rig.dir.path().join("work");
        let lamps = project.join("refs").join("lamps");
        std::fs::create_dir_all(&lamps).expect("dir");
        std::fs::write(lamps.join("post.json"), lamp(12)).expect("write");
        std::fs::write(lamps.join("wood.png"), png([120, 80, 40, 255])).expect("write");
        rig.settle(Some(&project));
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(Some(&project));
        (project, lamps)
    }

    /// A folder dialog that was opened for the project library, as it is while the user picks:
    /// the sender gives the answer.
    fn style_picker(rig: &mut Rig, project: &Path, licence: &str) -> mpsc::Sender<Vec<PathBuf>> {
        let (tx, rx) = mpsc::channel();
        let goal = PickFor::Style {
            licence: licence.to_owned(),
            root: Some(project.join(PROJECT_DIR)),
        };
        rig.service.pickers.push((goal, rx));
        tx
    }

    /// What the folder dialog gives for a style folder, without opening one.
    fn pick_style(rig: &mut Rig, project: &Path, folder: PathBuf, licence: &str) {
        style_picker(rig, project, licence)
            .send(vec![folder])
            .expect("sent");
        rig.settle(Some(project));
    }

    #[test]
    fn a_picked_folder_becomes_style_references_that_the_view_counts_apart() {
        let mut rig = Rig::new();
        let (project, lamps) = project_with_lamps(&mut rig);
        pick_style(&mut rig, &project, lamps, "CC0");
        let view = rig.service.view();
        let brief = &view.brief;
        assert!(brief.project);
        assert_eq!(
            brief.style_sources,
            [StyleSourceView {
                path: "refs/lamps".to_owned(),
                models: 1,
                textures: 1,
                licence: "CC0".to_owned(),
            }]
        );
        assert_eq!((brief.style_models, brief.style_textures), (1, 1));
        assert_eq!(
            (brief.models, brief.textures),
            (0, 0),
            "not the project's own"
        );
        assert!(view.entries.len() == 2 && view.entries.iter().all(|entry| entry.style));
        assert!(
            rig.toasts
                .iter()
                .any(|(_, text)| text.starts_with("Imported 2 files")),
            "{:?}",
            rig.toasts
        );

        // A folder outside the project is refused as an error, and the library stays usable.
        let outside = rig.dir.path().join("src");
        std::fs::create_dir_all(&outside).expect("dir");
        pick_style(&mut rig, &project, outside, "CC0");
        assert!(
            rig.toasts.iter().any(|(level, text)| *level == Level::Error
                && text.contains("not a folder inside the project")),
            "{:?}",
            rig.toasts
        );
        let view = rig.service.view();
        assert!(!view.busy && view.total == 2 && view.brief.style_sources.len() == 1);
    }

    #[test]
    fn style_folders_are_rescanned_and_removed_without_touching_the_folder() {
        let mut rig = Rig::new();
        let (project, lamps) = project_with_lamps(&mut rig);
        // No licence typed: the folder is unknown.
        pick_style(&mut rig, &project, lamps.clone(), "");
        assert_eq!(rig.service.view().brief.style_sources[0].licence, "unknown");
        std::fs::write(lamps.join("torch.json"), lamp(8)).expect("write");
        rig.toasts.clear();
        rig.service
            .apply(LibraryRequest::RescanStyle("refs/lamps".to_owned()));
        assert!(rig.service.jobs()[0].cancellable, "a rescan shows progress");
        rig.settle(Some(&project));
        let brief = &rig.service.view().brief;
        assert_eq!(brief.style_sources[0].models, 2);
        assert_eq!((brief.style_models, brief.style_textures), (2, 1));
        assert!(
            rig.toasts
                .iter()
                .any(|(_, text)| text.starts_with("Imported 1 file")),
            "{:?}",
            rig.toasts
        );

        rig.toasts.clear();
        rig.service
            .apply(LibraryRequest::RemoveStyleSource("refs/lamps".to_owned()));
        rig.settle(Some(&project));
        let view = rig.service.view();
        assert!(view.brief.style_sources.is_empty() && view.total == 0);
        assert_eq!((view.brief.style_models, view.brief.style_textures), (0, 0));
        assert!(lamps.join("post.json").is_file() && lamps.join("wood.png").is_file());
        assert!(
            rig.toasts
                .iter()
                .any(|(level, text)| *level == Level::Info && text == "Removed 3 style references."),
            "{:?}",
            rig.toasts
        );

        // A folder that is not one gives a message instead of a silent nothing.
        rig.toasts.clear();
        rig.service
            .apply(LibraryRequest::RemoveStyleSource("nowhere".to_owned()));
        rig.settle(Some(&project));
        assert!(
            rig.toasts
                .iter()
                .any(|(level, text)| *level == Level::Error && text.contains("not a style folder")),
            "{:?}",
            rig.toasts
        );

        // A rescan that fails says it was a rescan.
        rig.toasts.clear();
        rig.service
            .apply(LibraryRequest::RescanStyle("nowhere".to_owned()));
        rig.settle(Some(&project));
        assert!(
            rig.toasts.iter().any(
                |(level, text)| *level == Level::Error && text.starts_with("The rescan failed")
            ),
            "{:?}",
            rig.toasts
        );
    }

    #[test]
    fn a_refused_removal_leaves_the_index_alone_and_a_removal_reaches_the_disk() {
        let mut rig = Rig::new();
        let (project, lamps) = project_with_lamps(&mut rig);
        pick_style(&mut rig, &project, lamps, "CC0");
        let index = project.join(PROJECT_DIR).join(INDEX_FILE);
        let written = || std::fs::metadata(&index).expect("the index").modified();
        let before = written();
        thread::sleep(Duration::from_millis(50));
        rig.service
            .apply(LibraryRequest::RemoveStyleSource("nowhere".to_owned()));
        rig.settle(Some(&project));
        assert_eq!(
            written().ok(),
            before.ok(),
            "nothing changed, nothing saved"
        );

        rig.service
            .apply(LibraryRequest::RemoveStyleSource("refs/lamps".to_owned()));
        rig.settle(Some(&project));
        let saved = Library::read(&project.join(PROJECT_DIR), Scope::Project).expect("reads");
        assert!(saved.brief().style_sources.is_empty() && saved.entries().is_empty());
    }

    #[test]
    fn an_edit_while_a_job_holds_the_library_is_refused_with_a_toast() {
        let mut rig = Rig::new();
        let (project, lamps) = project_with_lamps(&mut rig);
        // Keep the library thread busy so the import is still running when the brief arrives.
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let _gate = rig.service.io().expect("thread").submit(move |_| {
            let _ = gate_rx.recv_timeout(Duration::from_secs(10));
            Ok(())
        });
        style_picker(&mut rig, &project, "CC0")
            .send(vec![lamps])
            .expect("sent");
        rig.toasts.extend(rig.service.step(Some(&project)));
        let view = rig.service.view();
        let job = view.job.as_ref().expect("the import is shown as the job");
        assert!(view.busy && job.cancellable && job.id == JOB_ID);
        assert_eq!(job.label, "Importing style references");

        // The dialog does not let Save through then, but a request that arrives is told.
        let goal = BriefInputs {
            goal: "Splat weapons".to_owned(),
            ..BriefInputs::default()
        };
        rig.service.apply(LibraryRequest::SetBrief(Box::new(goal)));
        rig.toasts.extend(rig.service.step(Some(&project)));
        assert!(
            rig.toasts
                .iter()
                .any(|(level, text)| *level == Level::Info && text.contains("busy")),
            "{:?}",
            rig.toasts
        );
        gate_tx.send(()).expect("gate");
        rig.settle(Some(&project));
        let view = rig.service.view();
        assert!(view.job.is_none() && view.brief.inputs.goal.is_empty());
        assert_eq!(view.brief.style_sources.len(), 1);

        // Once the library is free, the same request goes through.
        let goal = BriefInputs {
            goal: "Splat weapons".to_owned(),
            ..BriefInputs::default()
        };
        rig.service.apply(LibraryRequest::SetBrief(Box::new(goal)));
        rig.settle(Some(&project));
        assert_eq!(rig.service.view().brief.inputs.goal, "Splat weapons");
    }

    #[test]
    fn a_picked_style_folder_goes_to_the_project_it_was_asked_for() {
        let mut rig = Rig::new();
        let (project, lamps) = project_with_lamps(&mut rig);
        // The user switches to the global library while the dialog is open.
        let picker = style_picker(&mut rig, &project, "CC0");
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Global));
        picker.send(vec![lamps.clone()]).expect("sent");
        rig.settle(Some(&project));
        assert!(
            !rig.toasts.iter().any(|(level, _)| *level == Level::Error),
            "{:?}",
            rig.toasts
        );
        assert_eq!(rig.service.view().total, 0, "the global library is shown");
        rig.service
            .apply(LibraryRequest::SetScope(LibScope::Project));
        rig.settle(Some(&project));
        let view = rig.service.view();
        assert_eq!(view.total, 2);
        assert_eq!(view.brief.style_sources.len(), 1);

        // A pick for a project that is not the open one any more is dropped, and told.
        rig.toasts.clear();
        let other = rig.dir.path().join("elsewhere");
        let picker = style_picker(&mut rig, &other, "CC0");
        picker.send(vec![lamps]).expect("sent");
        rig.settle(Some(&project));
        assert!(
            rig.toasts
                .iter()
                .any(|(level, text)| *level == Level::Info && text.contains("project changed")),
            "{:?}",
            rig.toasts
        );
        assert_eq!(rig.service.view().total, 2);
    }

    #[test]
    fn a_model_of_the_project_library_changes_its_role() {
        let mut rig = Rig::new();
        let (project, _) = project_with_lamps(&mut rig);
        let file = rig.write("gun.json", &lamp(10));
        rig.service.apply(LibraryRequest::Import(vec![file]));
        rig.settle(Some(&project));
        let id = rig.service.view().entries[0].id.clone();
        assert!(!rig.service.view().entries[0].style);
        assert_eq!(rig.service.view().brief.models, 1);

        rig.service.apply(LibraryRequest::SetRole {
            id: id.clone(),
            style: true,
        });
        rig.settle(Some(&project));
        let view = rig.service.view();
        assert!(view.entries[0].style);
        assert_eq!((view.brief.models, view.brief.style_models), (0, 1));
        assert!(
            view.brief.style_sources.is_empty(),
            "marked one by one, from no folder"
        );

        rig.service
            .apply(LibraryRequest::SetRole { id, style: false });
        rig.settle(Some(&project));
        let view = rig.service.view();
        assert!(!view.entries[0].style);
        assert_eq!((view.brief.models, view.brief.style_models), (1, 0));
    }

    #[test]
    fn pictures_load_when_asked_and_models_without_one_wait_for_the_rig() {
        let mut rig = Rig::new();
        let tex = rig.write("stone.png", &png([200, 10, 10, 255]));
        rig.service.apply(LibraryRequest::Import(vec![tex]));
        rig.settle(None);
        let id = rig.service.view().entries[0].id.clone();
        assert!(matches!(
            rig.service.view().entries[0].thumb,
            ThumbState::Unknown
        ));
        rig.service.apply(LibraryRequest::Thumbs(vec![id]));
        rig.settle(None);
        let ThumbState::Ready(image) = &rig.service.view().entries[0].thumb else {
            panic!("{:?}", rig.service.view().entries[0].thumb);
        };
        assert_eq!(image.size, [4, 4]);
        assert_eq!(&image.rgba[..4], &[200, 10, 10, 255]);
        assert!(!rig.service.has_renders());

        // A recent model without a picture is queued for the rig once.
        let model = rig.write("tower.cubik", b"PK");
        let first = rig.service.recent_thumbs(std::slice::from_ref(&model));
        assert!(first[0].is_none());
        rig.settle(None);
        let request = rig.service.take_render().expect("a render request");
        assert_eq!(request.source, model);
        assert!(
            request
                .out
                .starts_with(rig.dir.path().join("data").join("cache"))
        );
        assert!(rig.service.take_render().is_none());
        rig.service.recent_thumbs(std::slice::from_ref(&model));
        rig.settle(None);
        assert!(!rig.service.has_renders(), "asked once");

        // The rig delivers a picture: it is written and read back.
        let mut buf = RgbaBuf::new(2, 2);
        buf.pixels[..4].copy_from_slice(&[1, 2, 3, 255]);
        rig.service
            .render_done(request, Some(Image::from_rgba(buf)));
        rig.settle(None);
        let shown = rig.service.recent_thumbs(std::slice::from_ref(&model));
        assert!(
            shown[0].as_ref().is_some_and(|image| image.size == [2, 2]),
            "{shown:?}"
        );
        // A save asks for a new picture of the same file.
        rig.service.thumbnail_saved(&model);
        assert!(rig.service.take_render().is_some());
    }

    #[test]
    fn pinned_recent_files_are_stored() {
        let mut rig = Rig::new();
        let path = PathBuf::from("C:/models/tower.cubik");
        rig.service.apply(LibraryRequest::TogglePin(path.clone()));
        rig.settle(None);
        assert_eq!(&rig.service.view().pinned[..], std::slice::from_ref(&path));
        let mut other = LibraryService::new(&Dirs::under(rig.dir.path()));
        let start = Instant::now();
        while other.view().pinned.is_empty() {
            other.step(None);
            assert!(start.elapsed() < Duration::from_secs(20));
            thread::sleep(Duration::from_millis(2));
        }
        rig.service.apply(LibraryRequest::TogglePin(path));
        rig.settle(None);
        assert!(rig.service.view().pinned.is_empty());
    }

    fn state_in(dir: &Path) -> State {
        let settings = cuboid_settings::Settings::first_run(&cuboid_settings::Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        State::new(
            Dirs::under(dir),
            settings,
            IoThread::spawn().expect("thread"),
            1252,
            Instant::now(),
        )
    }

    fn wait(state: &mut State, done: &dyn Fn(&State) -> bool) {
        let start = Instant::now();
        while !done(state) {
            state.step(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_dropped_texture_is_added_to_the_document() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut state = state_in(dir.path());
        let file = dir.path().join("src").join("cobble stone.png");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        std::fs::write(&file, png([5, 6, 7, 255])).expect("write");
        wait(&mut state, &|s| !s.library.view().busy);
        state.library.apply(LibraryRequest::Import(vec![file]));
        wait(&mut state, &|s| s.library.view().total == 1);
        wait(&mut state, &|s| !s.library.view().busy);
        let id = state.library.view().entries[0].id.clone();
        let before = state.editor.core.doc().textures.entries.len();
        use_texture(&mut state, &id);
        wait(&mut state, &|s| {
            s.editor.core.doc().textures.entries.len() > before
        });
        let entries = &state.editor.core.doc().textures.entries;
        let added = entries.last().expect("a texture");
        assert_eq!(&*added.label, "cobblestone");
        assert_eq!((added.texture.image.w, added.texture.image.h), (4, 4));
        assert!(state.editor.is_dirty());
        assert!(
            state
                .toasts
                .iter()
                .any(|t| t.text.starts_with("Added the texture cobblestone"))
        );
        // The same texture again gets a free label.
        use_texture(&mut state, &id);
        wait(&mut state, &|s| {
            s.editor.core.doc().textures.entries.len() > before + 1
        });
        let doc = state.editor.core.doc();
        let labels: Vec<&str> = doc.textures.entries.iter().map(|e| &*e.label).collect();
        assert!(labels.contains(&"cobblestone1"), "{labels:?}");
        // Each drop is one undo step.
        let count = |state: &State| state.editor.core.doc().textures.entries.len();
        assert_eq!(count(&state), before + 2);
        assert!(state.editor.core.undo().is_done());
        assert_eq!(count(&state), before + 1);
        assert!(state.editor.core.undo().is_done());
        assert_eq!(count(&state), before);
        assert!(state.editor.core.redo().is_done());
        assert_eq!(count(&state), before + 1);
    }

    #[test]
    fn size_and_detail_text() {
        assert_eq!(size_text(12), "12 B");
        assert_eq!(size_text(2048), "2.0 KB");
        assert_eq!(size_text(30 << 20), "30 MB");
        assert!(renderable(Path::new("a.CUBIK")) && !renderable(Path::new("a.json")));
    }
}
