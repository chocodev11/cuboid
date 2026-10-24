//! The files of an export and writing them next to the target.

use std::io;
use std::path::Path;

use cuboid_model::Document;
use cuboid_model::image::encode_png;

use crate::gather::{MeshOptions, Tex};
use crate::{GltfFormat, MeshError, Notice, export_gltf, export_obj};

/// One file of an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    /// The file name, without a folder.
    pub name: String,
    /// The content.
    pub bytes: Vec<u8>,
}

/// The files of an export. The first one is the main file; the others (`.mtl`, `.bin`, PNG
/// textures) sit next to it and are referenced by name.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshExport {
    /// The main file first.
    pub files: Vec<ExportFile>,
    /// What was left out or changed.
    pub notices: Vec<Notice>,
}

impl MeshExport {
    /// Writes the main file to `target` and the others into the same folder, each atomically.
    /// The others go first, so a failure leaves no main file that points at missing files.
    pub fn write(&self, target: &Path) -> io::Result<()> {
        let Some((main, rest)) = self.files.split_first() else {
            return Ok(());
        };
        let dir = target.parent().unwrap_or_else(|| Path::new(""));
        for file in rest {
            cuboid_io::write_atomic(&dir.join(&file.name), &file.bytes)?;
        }
        cuboid_io::write_atomic(target, &main.bytes)
    }
}

/// Exports the document in the format the extension of `target` names: `obj` (with `.mtl` and
/// PNG textures), `glb` (textures embedded) or `gltf` (with `.bin` and PNG textures). The names
/// of the other files start with the stem of `target`.
pub fn export_for_path(
    doc: &Document,
    target: &Path,
    opts: &MeshOptions,
) -> Result<MeshExport, MeshError> {
    let ext = target
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match ext.as_str() {
        "obj" => export_obj(doc, &stem, opts),
        "glb" => export_gltf(doc, &stem, opts, GltfFormat::Glb),
        "gltf" => export_gltf(doc, &stem, opts, GltfFormat::Separate),
        _ => Err(MeshError::Extension(ext)),
    }
}

/// The PNG file of a texture.
pub(crate) fn png(tex: &Tex) -> Result<Vec<u8>, MeshError> {
    encode_png(&tex.image).map_err(|e| MeshError::Texture {
        name: tex.name.clone(),
        reason: e.to_string(),
    })
}
