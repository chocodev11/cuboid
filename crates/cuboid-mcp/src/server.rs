//! The state of a server: the project folder, the models that are open, the revision tokens and
//! the journal of writes. The tools are `impl Server` blocks in their own files.

use std::collections::hash_map::RandomState;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cuboid_core::{Checkpoint, Compat, Core};
use cuboid_fmt_cubik::ProjectMeta;
use cuboid_library::PROJECT_DIR;
use cuboid_library::doc::load;
use cuboid_model::Document;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::paths::Root;
use crate::render::Renderer;

/// Models kept open at once.
pub const MAX_OPEN: usize = 8;
/// Checkpoints kept per model.
pub const MAX_CHECKPOINTS: usize = 16;
/// The seed of the random generator of every core, so that the same ops give the same model.
const SEED: u64 = 0x6375_626f_6964;
/// The journal of writes, in the project folder.
const JOURNAL: &str = "journal.jsonl";

/// What a server may do.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// Refuse every write: edits, saves, exports, imports and the files of renders.
    pub read_only: bool,
}

/// What a tool returns: the structured result, and the PNG pictures that go with it.
#[derive(Debug)]
pub struct Output {
    /// The result, which matches the `outputSchema` of the tool.
    pub value: Value,
    /// PNG files, sent as image content.
    pub images: Vec<Vec<u8>>,
}

impl Output {
    /// A result without pictures.
    pub fn json(value: Value) -> Output {
        Output {
            value,
            images: Vec::new(),
        }
    }
}

/// Size and time of a file, to tell whether someone else wrote it.
pub type Stamp = (u64, Option<SystemTime>);

/// The stamp of the file at `path`, `None` when there is none.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// A model that is open: the core with its history, and what a save needs to know.
pub struct OpenModel {
    /// The path relative to the project folder, joined with `/`.
    pub name: String,
    /// Where the file is, or will be.
    pub path: PathBuf,
    /// The document with its selection and undo history.
    pub core: Core,
    /// The settings of a `.cubik` file, written back by a save.
    pub project: Option<ProjectMeta>,
    /// What the importer left out or changed.
    pub notes: Vec<String>,
    /// The stamp of the file when it was read or written last; `None` for a model never saved.
    pub stamp: Option<Stamp>,
    /// Named copies of the model, for `history`.
    pub checkpoints: Vec<(String, Checkpoint)>,
    used: u64,
}

impl OpenModel {
    /// A model that was just read from `path`, or made for it.
    pub fn new(
        name: String,
        path: PathBuf,
        core: Core,
        project: Option<ProjectMeta>,
        notes: Vec<String>,
    ) -> OpenModel {
        OpenModel {
            name,
            stamp: stamp(&path),
            path,
            core,
            project,
            notes,
            checkpoints: Vec::new(),
            used: 0,
        }
    }
}

/// The server: one project folder and the models open in it.
pub struct Server<'r> {
    pub(crate) root: Root,
    pub(crate) options: Options,
    pub(crate) renderer: &'r mut dyn Renderer,
    session: String,
    /// The name the client gave, for the journal.
    pub(crate) client: String,
    pub(crate) models: Vec<OpenModel>,
    clock: u64,
}

/// The key two spellings of a name share: Windows does not tell `A.cubik` from `a.cubik`.
fn same(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

impl<'r> Server<'r> {
    /// A server for the project folder `root`.
    pub fn new(
        root: &Path,
        options: Options,
        renderer: &'r mut dyn Renderer,
    ) -> Result<Self, String> {
        let session = format!("{:08x}", RandomState::new().build_hasher().finish() >> 32);
        Ok(Server {
            root: Root::new(root)?,
            options,
            renderer,
            session,
            client: "unknown".to_owned(),
            models: Vec::new(),
            clock: 0,
        })
    }

    /// The project folder.
    pub fn root(&self) -> &Root {
        &self.root
    }

    /// The folder of the project library, resolved like every other path.
    pub(crate) fn library_dir(&self) -> Result<PathBuf, ToolError> {
        self.root.resolve(PROJECT_DIR)
    }

    /// Whether the file is a stored copy of the library: a style reference, which is read-only.
    pub(crate) fn in_library(&self, path: &Path) -> bool {
        self.library_dir()
            .is_ok_and(|library| path.starts_with(library))
    }

    /// The token of the revision a document is at: `<session>-<revision>`.
    pub fn token(&self, doc: &Document) -> String {
        format!("{}-{}", self.session, doc.revision.get())
    }

    /// Checks the revision a caller expects; `None` expects nothing.
    pub(crate) fn check_revision(
        &self,
        doc: &Document,
        expected: Option<&str>,
    ) -> Result<(), ToolError> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let current = self.token(doc);
        if expected == current {
            return Ok(());
        }
        let from_here = expected
            .strip_prefix(&self.session)
            .is_some_and(|rest| rest.starts_with('-'));
        Err(if from_here {
            ToolError::revision(expected, &current)
        } else {
            ToolError::session(expected, &current)
        })
    }

    /// Refuses a file under `cuboid-library`: the stored copies are style references, which are
    /// read-only.
    pub(crate) fn outside_library(&self, path: &Path) -> Result<(), ToolError> {
        if self.in_library(path) {
            return Err(ToolError::read_only(
                "files under cuboid-library are style references, which are read-only",
            ));
        }
        Ok(())
    }

    /// Refuses a change to a model that may not change: a style reference. (A read-only server
    /// has refused the call before it got here.)
    pub(crate) fn writable(&self, index: usize) -> Result<(), ToolError> {
        self.outside_library(&self.models[index].path)
    }

    /// The place of an open model in the list, or `None`.
    pub(crate) fn find(&self, name: &str) -> Option<usize> {
        self.models.iter().position(|model| same(&model.name, name))
    }

    /// The model at `name`, opened from disk when it is not open yet. At most [`MAX_OPEN`] stay
    /// open: a saved one makes room (see [`Server::insert`]).
    pub(crate) fn model(&mut self, name: &str) -> Result<usize, ToolError> {
        let path = self.root.resolve(name)?;
        let name = self.root.relative(&path);
        let index = match self.find(&name) {
            Some(index) => index,
            None => {
                if !path.is_file() {
                    return Err(ToolError::no_match(format!("{name} is not a file")));
                }
                let opened = load(&path)?;
                let project = self.project_of(&path, opened.project);
                let core = self.core_of(opened.doc, project.as_ref(), &path);
                self.insert(OpenModel::new(name, path, core, project, opened.notes))?
            }
        };
        self.clock += 1;
        self.models[index].used = self.clock;
        Ok(index)
    }

    /// Puts a model into the list. When it is full, a model makes room that has a file, no
    /// unsaved edits and no checkpoints: nothing is lost but its undo steps, so one without
    /// steps goes first, then the one used least recently.
    pub(crate) fn insert(&mut self, model: OpenModel) -> Result<usize, ToolError> {
        if self.models.len() >= MAX_OPEN {
            let oldest = self
                .models
                .iter()
                .enumerate()
                .filter(|(_, open)| {
                    open.stamp.is_some() && open.checkpoints.is_empty() && !open.core.is_dirty()
                })
                .min_by_key(|(_, open)| (open.core.history().undo_len() > 0, open.used))
                .map(|(index, _)| index)
                .ok_or_else(|| {
                    ToolError::limit(format!(
                        "{MAX_OPEN} models are open and none can close: each is new, has unsaved edits or has checkpoints"
                    ))
                    .hinted("Save one of the open models with project_save so that it can close; a model with checkpoints stays open.")
                })?;
            self.models.remove(oldest);
        }
        self.models.push(model);
        Ok(self.models.len() - 1)
    }

    /// The `cuboid.json` settings a model is saved with: what its file said, else, for a
    /// Blockbench or Minecraft file, the Minecraft preset of the version of the brief.
    fn project_of(&self, path: &Path, from_file: Option<ProjectMeta>) -> Option<ProjectMeta> {
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase());
        match extension.as_deref() {
            Some("bbmodel" | "json") => Some(self.minecraft_project()),
            _ => from_file,
        }
    }

    /// The Minecraft preset of the project: the game version of the brief, else the newest.
    pub(crate) fn minecraft_project(&self) -> ProjectMeta {
        let version = self
            .brief_version()
            .unwrap_or_else(cuboid_fmt_minecraft::latest_version);
        ProjectMeta {
            preset: "minecraft".to_owned(),
            version: Some(version.to_string()),
            allow_negative_size: None,
        }
    }

    /// The core of a document, with the rules of its preset.
    pub(crate) fn core_of(
        &self,
        doc: Document,
        project: Option<&ProjectMeta>,
        path: &Path,
    ) -> Core {
        let minecraft = project.is_some_and(|meta| meta.preset == "minecraft")
            || path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("bbmodel") || ext.eq_ignore_ascii_case("json")
            });
        let mut core = Core::with_document(doc, Compat::port(), SEED);
        core.env_mut().allow_negative_size = minecraft;
        core
    }

    /// Adds a line to `journal.jsonl`: when, who, what and where. A journal that cannot be
    /// written does not undo the write it records.
    pub(crate) fn journal(&self, tool: &str, value: &Value) {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let line = json!({
            "time": time,
            "client": self.client,
            "tool": tool,
            "model": value.get("model").or_else(|| value.get("ref")),
            "revision": value.get("revision"),
        });
        let written = self.root.resolve(JOURNAL).and_then(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| writeln!(file, "{line}"))
                .map_err(ToolError::from)
        });
        if let Err(error) = written {
            log::warn!("cannot write the journal: {}", error.message);
        }
    }
}
