//! Opening a loaded file in a document and capturing it again, as File > Open and Save do.

use std::fs;
use std::path::Path;

use cuboid_fmt_cubik::ProjectMeta;
use cuboid_fmt_cubik::cub::{self, DiskFiles};
use cuboid_model::{
    ApplyMode, ApplySource, CaptureScope, Categories, Document, EditCtx, EditEnv, LoadedDoc,
    RenderPrefs, Selection, Snapshot, TextureList, apply_document, capture,
};
use cuboid_num::EngineRng;

use crate::Fail;

/// A document with the loaded sections applied (`ApplyMode::Open`).
pub fn open(loaded: LoadedDoc) -> Result<(Document, Selection), Fail> {
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
    )
    .map_err(|err| Fail::Error(format!("the file cannot be opened: {err}")))?;
    Ok((doc, sel))
}

/// A snapshot of all categories, the input of a save.
pub fn snapshot(doc: &Document, sel: &Selection) -> Snapshot {
    capture(
        doc,
        sel,
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    )
}

/// A model file as the app opens it: the document, the `.cubik` project settings and what the
/// importer wants to say.
pub struct Opened {
    pub doc: Document,
    /// The `cuboid.json` entry of a `.cubik` file.
    pub project: Option<ProjectMeta>,
    pub notes: Vec<String>,
}

/// The content of a file after its size was checked against `limit`.
fn read_within(path: &Path, limit: u64) -> Result<Vec<u8>, Fail> {
    let failed = |err: &dyn std::fmt::Display| Fail::Error(format!("{}: {err}", path.display()));
    let size = fs::metadata(path).map_err(|err| failed(&err))?.len();
    if size > limit {
        return Err(failed(&format!(
            "the file is {size} bytes, over the limit of {limit}"
        )));
    }
    fs::read(path).map_err(|err| failed(&err))
}

/// Reads a `.cubik`, `.cub`, `.bbmodel` or Minecraft model JSON file with the importers of the
/// app and opens it in a document.
pub fn load(path: &Path) -> Result<Opened, Fail> {
    let failed = |err: &dyn std::fmt::Display| Fail::Error(format!("{}: {err}", path.display()));
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut project = None;
    let (loaded, notes) = match extension.as_str() {
        "cubik" => {
            let loaded = cuboid_fmt_cubik::load_path(path, &cuboid_fmt_cubik::Limits::default())
                .map_err(|err| failed(&err))?;
            project = loaded.project;
            (loaded.doc, loaded.warnings)
        }
        "cub" => {
            let bytes = read_within(path, cuboid_fmt_cubik::Limits::default().max_text)?;
            let imported = cub::import(&bytes, path, &DiskFiles).map_err(|err| failed(&err))?;
            let notes = imported.notices.iter().map(|n| format!("{n:?}")).collect();
            (imported.doc, notes)
        }
        "bbmodel" => {
            let bytes = read_within(path, cuboid_fmt_bbmodel::Limits::default().max_text)?;
            let imported =
                cuboid_fmt_bbmodel::import(&bytes, path, &DiskFiles).map_err(|err| failed(&err))?;
            let notes = imported.notices.iter().map(ToString::to_string).collect();
            (imported.doc, notes)
        }
        "json" => {
            let bytes = read_within(path, cuboid_fmt_minecraft::Limits::default().max_text)?;
            let imported = cuboid_fmt_minecraft::import(&bytes, path, &DiskFiles)
                .map_err(|err| failed(&err))?;
            let notes = imported.notices.iter().map(ToString::to_string).collect();
            (imported.doc, notes)
        }
        _ => {
            return Err(Fail::Usage(
                "only .cubik, .cub, .bbmodel and .json files can be checked".to_owned(),
            ));
        }
    };
    let (doc, _) = open(loaded)?;
    Ok(Opened {
        doc,
        project,
        notes,
    })
}
