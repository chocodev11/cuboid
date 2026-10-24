//! Helpers of the integration tests: pictures, a file tree in memory and a document to open an
//! import in.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use cuboid_fmt_minecraft::{JsonImport, TextureFiles, import};
use cuboid_io::texture_files::Refusal;
use cuboid_model::image::{Image, RgbaBuf, encode_png};
use cuboid_model::invariants::check_loaded_invariants;
use cuboid_model::{
    ApplyMode, ApplySource, Document, EditCtx, EditEnv, Element, RenderPrefs, Selection,
    TextureList, apply_document,
};
use cuboid_num::EngineRng;

/// Files kept in memory, found by their path with `/` separators.
#[derive(Default)]
pub struct Tree(HashMap<String, Vec<u8>>);

impl Tree {
    pub fn with(mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.0.insert(path.to_owned(), bytes.into());
        self
    }
}

impl TextureFiles for Tree {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        let key = path.to_string_lossy().replace('\\', "/");
        Ok(self.0.get(&key).cloned())
    }
}

/// A solid picture.
pub fn picture(w: u32, h: u32, color: [u8; 4]) -> Arc<Image> {
    let mut buf = RgbaBuf::new(w, h);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
    Arc::new(Image::from_rgba(buf))
}

pub fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    encode_png(&picture(w, h, color)).expect("encodes")
}

/// Imports model text found at `path`, with the other files of `tree`.
pub fn import_at(text: &str, path: &str, tree: &Tree) -> JsonImport {
    import(text.as_bytes(), Path::new(path), tree).expect("imports")
}

/// The document that File > Open makes of an import.
pub fn open(imported: JsonImport) -> Document {
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
        ApplySource::Loaded(Box::new(imported.doc)),
        ApplyMode::Open,
    )
    .expect("the import applies");
    check_loaded_invariants(&doc, &sel).expect("the document is sound");
    doc
}

pub fn elements(doc: &Document) -> Vec<Element> {
    doc.elements.items().iter().map(|e| (**e).clone()).collect()
}
