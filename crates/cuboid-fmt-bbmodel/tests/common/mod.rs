//! Helpers of the integration tests: pictures, a project from JSON, and a document to open it in.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use cuboid_fmt_bbmodel::data_url::png_url;
use cuboid_fmt_bbmodel::{BbImport, TextureFiles, import};
use cuboid_model::image::{Image, RgbaBuf, encode_png};
use cuboid_model::invariants::check_loaded_invariants;
use cuboid_model::{
    ApplyMode, ApplySource, Document, EditCtx, EditEnv, Element, RenderPrefs, Selection,
    TextureList, apply_document,
};
use cuboid_num::EngineRng;
use glam::Vec3;
use serde_json::Value;

/// Texture files that do not exist.
pub struct NoFiles;

impl TextureFiles for NoFiles {
    fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, cuboid_io::texture_files::Refusal> {
        Ok(None)
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

/// A picture whose texels differ, to see that they arrive unchanged.
pub fn gradient(w: u32, h: u32) -> Arc<Image> {
    let mut buf = RgbaBuf::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) as usize * 4;
            buf.pixels[at..at + 4].copy_from_slice(&[
                (x * 40 % 256) as u8,
                (y * 60 % 256) as u8,
                ((x + y) * 20 % 256) as u8,
                255,
            ]);
        }
    }
    Arc::new(Image::from_rgba(buf))
}

pub fn png_of(image: &Image) -> Vec<u8> {
    encode_png(image).expect("encodes")
}

/// The `data:` URL of a solid picture.
pub fn data_url(w: u32, h: u32, color: [u8; 4]) -> String {
    png_url(&png_of(&picture(w, h, color)))
}

pub fn text_of(project: &Value) -> Vec<u8> {
    serde_json::to_vec(project).expect("serialises")
}

/// Imports a project from JSON with no texture files, as `dir/Model.bbmodel`.
pub fn import_json(project: &Value) -> BbImport {
    import(&text_of(project), Path::new("dir/Model.bbmodel"), &NoFiles).expect("imports")
}

/// The document that File > Open makes of an import.
pub fn open(imported: BbImport) -> Document {
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

/// The elements of an import before the apply path normalises them.
pub fn elements_of(imported: &BbImport) -> Vec<Element> {
    imported
        .doc
        .sections
        .elements
        .as_ref()
        .map(|snap| snap.list.items.iter().map(|e| (**e).clone()).collect())
        .unwrap_or_default()
}

/// The centre of an element in model space (Z negated back), from its stored fields.
pub fn center_m(el: &Element) -> Vec3 {
    let rot = cuboid_num::euler_to_quat(el.base.e);
    let e = el.base.p + rot * (el.base.s * Vec3::new(0.5, 0.5, -0.5));
    Vec3::new(e.x, e.y, -e.z)
}

pub fn near(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 2e-3)
}
