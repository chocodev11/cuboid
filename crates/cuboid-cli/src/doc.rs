//! Opening a loaded file in a document and capturing it again, as File > Open and Save do.

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
