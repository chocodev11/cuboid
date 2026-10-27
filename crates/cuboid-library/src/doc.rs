//! Opening a model file in a document, as File > Open does but without the app: the importers of
//! the formats, then the apply path of the model.

use std::fs;
use std::path::Path;

use cuboid_fmt_cubik::ProjectMeta;
use cuboid_fmt_cubik::cub::{self, DiskFiles};
use cuboid_model::{
    ApplyError, ApplyMode, ApplySource, Document, EditCtx, EditEnv, LoadedDoc, RenderPrefs,
    Selection, TextureList, apply_document,
};
use cuboid_num::EngineRng;

/// Why a model file is not opened.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The extension is not one of `.cubik`, `.cub`, `.bbmodel` and `.json`.
    #[error("only .cubik, .cub, .bbmodel and .json files are opened")]
    Unsupported,
    /// The file cannot be read or is not a valid model; the text names it.
    #[error("{0}")]
    Failed(String),
}

/// A model file as the app opens it: the document, the `.cubik` project settings and what the
/// importer wants to say.
pub struct Opened {
    /// The document, with the sections of the file applied.
    pub doc: Document,
    /// The `cuboid.json` entry of a `.cubik` file.
    pub project: Option<ProjectMeta>,
    /// What the importer left out or changed.
    pub notes: Vec<String>,
}

/// A document with the loaded sections applied (`ApplyMode::Open`).
pub fn open(loaded: LoadedDoc) -> Result<(Document, Selection), ApplyError> {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
        allow_negative_size: false,
    };
    let mut rng = EngineRng::seeded(1);
    apply_document(
        &mut EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        },
        ApplySource::Loaded(Box::new(loaded)),
        ApplyMode::Open,
    )?;
    Ok((doc, sel))
}

/// The content of a file after its size was checked against `limit`.
fn read_within(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path).map_err(|err| err.to_string())?.len();
    if size > limit {
        return Err(format!(
            "the file is {size} bytes, over the limit of {limit}"
        ));
    }
    fs::read(path).map_err(|err| err.to_string())
}

/// Reads a `.cubik`, `.cub`, `.bbmodel` or Minecraft model JSON file with the importers of the
/// app and opens it in a document. Textures and parent models are looked for beside the file.
pub fn load(path: &Path) -> Result<Opened, LoadError> {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let read = |limit| read_within(path, limit);
    let mut project = None;
    let imported: Result<_, String> = match extension.as_str() {
        "cubik" => cuboid_fmt_cubik::load_path(path, &cuboid_fmt_cubik::Limits::default())
            .map(|loaded| {
                project = loaded.project;
                (loaded.doc, loaded.warnings)
            })
            .map_err(|err| err.to_string()),
        "cub" => read(cuboid_fmt_cubik::Limits::default().max_text).and_then(|bytes| {
            let imported = cub::import(&bytes, path, &DiskFiles).map_err(|err| err.to_string())?;
            let notes = imported.notices.iter().map(|n| format!("{n:?}")).collect();
            Ok((imported.doc, notes))
        }),
        "bbmodel" => read(cuboid_fmt_bbmodel::Limits::default().max_text).and_then(|bytes| {
            let imported =
                cuboid_fmt_bbmodel::import(&bytes, path, &DiskFiles).map_err(|e| e.to_string())?;
            let notes = imported.notices.iter().map(ToString::to_string).collect();
            Ok((imported.doc, notes))
        }),
        "json" => read(cuboid_fmt_minecraft::Limits::default().max_text).and_then(|bytes| {
            let imported = cuboid_fmt_minecraft::import(&bytes, path, &DiskFiles)
                .map_err(|err| err.to_string())?;
            let notes = imported.notices.iter().map(ToString::to_string).collect();
            Ok((imported.doc, notes))
        }),
        _ => return Err(LoadError::Unsupported),
    };
    let failed =
        |err: &dyn std::fmt::Display| LoadError::Failed(format!("{}: {err}", path.display()));
    let (loaded, notes) = imported.map_err(|err| failed(&err))?;
    let (doc, _) = open(loaded)
        .map_err(|err| LoadError::Failed(format!("the file cannot be opened: {err}")))?;
    Ok(Opened {
        doc,
        project,
        notes,
    })
}
