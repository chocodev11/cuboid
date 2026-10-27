//! Opening a loaded file in a document and capturing it again, as File > Open and Save do.

use cuboid_library::doc::LoadError;
use cuboid_model::{CaptureScope, Categories, Document, LoadedDoc, Selection, Snapshot, capture};

use crate::Fail;

/// A document with the loaded sections applied (`ApplyMode::Open`).
pub fn open(loaded: LoadedDoc) -> Result<(Document, Selection), Fail> {
    cuboid_library::doc::open(loaded)
        .map_err(|err| Fail::Error(format!("the file cannot be opened: {err}")))
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

/// The failure of a command that opens a model file. `verb` says what the command does with the
/// file: an unknown extension is a mistake of the command line.
pub fn fail(error: LoadError, verb: &str) -> Fail {
    match error {
        LoadError::Unsupported => Fail::Usage(format!(
            "only .cubik, .cub, .bbmodel and .json files can be {verb}"
        )),
        LoadError::Failed(text) => Fail::Error(text),
    }
}
