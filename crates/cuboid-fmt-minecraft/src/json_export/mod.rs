//! Minecraft Java block and item model export (spec: interop 7, 31.2).
//!
//! The model is written for a game version. The rules of that version (rotation, bounds, UV and
//! texture references) are decided by `cuboid-validate` through [`cuboid_validate::fits`]; a model
//! that does not fit is refused unless [`JsonOptions::force`] is set. The text is built by hand in
//! the layout of the reference (tabs, LF, `X9` numbers) with every string escaped, and is the same
//! for the same model.

mod text;

use std::collections::HashSet;

use cuboid_assets::mc_versions::{TextureDirs, Tier, mc_versions};
use cuboid_model::image::encode_png;
use cuboid_model::{Document, ModelKind, ObjRef, TextureSource};
use cuboid_validate::{
    GEO_BOUNDS, GEO_ROTATION, NUM_NONFINITE, Profile, Severity, TEX_FACE_NONE, TEX_UNRESOLVED,
    Target, UV_RANGE, fits, validate,
};

use crate::{McError, McVersion, Notice};
pub(crate) use text::model_text;

/// How a model is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonOptions {
    /// The game version to write for.
    pub version: McVersion,
    /// Write the model although it breaks the rules of the version.
    pub force: bool,
}

impl JsonOptions {
    /// Options for a version; a model that does not fit is refused.
    pub fn new(version: McVersion) -> Self {
        Self {
            version,
            force: false,
        }
    }
}

/// A file written next to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    /// The path relative to the folder of the model file, with `/` separators.
    pub path: String,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// A written model.
#[derive(Debug)]
pub struct JsonExport {
    /// The text of the model file.
    pub json: String,
    /// The pictures of the textures that had no Minecraft path (interop 7.4), as PNG files beside
    /// the model. The model refers to them as `block/<name>` or `item/<name>`.
    pub files: Vec<ExportFile>,
    /// What the file does not hold.
    pub notices: Vec<Notice>,
}

/// Whether the document is written as a block or an item model.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    /// A single model file.
    Plain,
    /// The model of a resource pack (interop 7.1, forced mode): hidden elements stay, every
    /// display member is written.
    Pack,
}

/// A document ready to be written: every texture has a Minecraft path.
pub(crate) struct Prepared {
    pub doc: Document,
    pub tier: &'static Tier,
    pub item: bool,
    /// The entries that were given a path.
    pub fresh: Vec<usize>,
}

/// The Minecraft path of a texture (interop 7.4): the stored one, else the part of the source
/// file path after the last `/textures/`, else the file name; without the extension. Empty when
/// the texture was never saved.
pub(crate) fn texture_path(source: &TextureSource) -> String {
    if !source.mc_path.is_empty() {
        return source.mc_path.clone();
    }
    let Some(path) = source.path.as_deref().filter(|p| !p.is_empty()) else {
        return String::new();
    };
    let path = path.replace('\\', "/");
    let tail = match path.rsplit_once("/textures/") {
        Some((head, tail)) if !head.is_empty() => tail,
        _ => path.rsplit('/').next().unwrap_or_default(),
    };
    tail.rsplit_once('.')
        .map_or(tail, |(stem, _)| stem)
        .to_owned()
}

/// The rules every export checks, the same set as `cuboid_validate::fits`.
const FIT_CODES: [&str; 6] = [
    NUM_NONFINITE,
    GEO_BOUNDS,
    GEO_ROTATION,
    UV_RANGE,
    TEX_FACE_NONE,
    TEX_UNRESOLVED,
];

/// Checks the kind, the version and the fit, and gives every texture a path. `fresh` makes the
/// path of a texture that has none from the folder name (`block` or `item`) and a file name.
pub(crate) fn prepare(
    doc: &Document,
    version: McVersion,
    target: impl Fn(bool) -> Target,
    force: bool,
    fresh: &dyn Fn(&str, &str) -> String,
) -> Result<Prepared, McError> {
    let item = match doc.props.minecraft.kind {
        ModelKind::Block => false,
        ModelKind::Item => true,
        _ => return Err(McError::Kind),
    };
    let tier = mc_versions()
        .tier_for(version)
        .ok_or_else(|| McError::UnknownVersion(version.to_string()))?;
    let exclude_hidden = doc.props.general.exclude_hidden;
    let writes = doc
        .elements
        .ids()
        .iter()
        .zip(doc.elements.items())
        .any(|(&id, el)| {
            !(exclude_hidden && doc.is_hidden(ObjRef::Element(id)))
                && el.faces.iter().any(|face| face.active)
        });
    if !writes {
        return Err(McError::Empty);
    }

    let mut view = doc.clone();
    let mut taken: HashSet<String> = view
        .textures
        .entries
        .iter()
        .map(|entry| texture_path(&entry.texture.source))
        .filter(|path| !path.is_empty())
        .collect();
    let dir = match (tier.texture_dirs, item) {
        (TextureDirs::Plural, false) => "blocks",
        (TextureDirs::Plural, true) => "items",
        (TextureDirs::Singular, false) => "block",
        (TextureDirs::Singular, true) => "item",
    };
    let mut generated = Vec::new();
    for (at, entry) in view.textures.entries.iter_mut().enumerate() {
        if !texture_path(&entry.texture.source).is_empty() {
            continue;
        }
        let name = file_name(&entry.label);
        let mut path = fresh(dir, &name);
        let mut n = 1;
        while taken.contains(&path) {
            path = fresh(dir, &format!("{name}_{n}"));
            n += 1;
        }
        taken.insert(path.clone());
        entry.texture.source.mc_path = path;
        generated.push(at);
    }

    if !force {
        check_fit(&view, target(item), version)?;
    }
    Ok(Prepared {
        doc: view,
        tier,
        item,
        fresh: generated,
    })
}

/// A file name for a texture label: lower-case ASCII letters, digits, `_` and `-`.
fn file_name(label: &str) -> String {
    let name: String = label
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "texture".to_owned()
    } else {
        name
    }
}

fn check_fit(doc: &Document, target: Target, version: McVersion) -> Result<(), McError> {
    if fits(doc, &target) {
        return Ok(());
    }
    let mut profile = Profile::new(target);
    profile.min_severity = Severity::Warning;
    profile.only = FIT_CODES.to_vec();
    let issues = validate(doc, &profile);
    Err(McError::DoesNotFit {
        version: version.to_string(),
        first: issues
            .first()
            .map_or_else(String::new, |issue| issue.message.clone()),
        count: issues.len(),
    })
}

/// The PNG bytes of a texture entry.
pub(crate) fn png(entry: &cuboid_model::TextureEntry) -> Result<Vec<u8>, McError> {
    encode_png(&entry.texture.image).map_err(|error| McError::Texture {
        name: entry.label.to_string(),
        reason: error.to_string(),
    })
}

/// Writes the document as a block or item model for a game version.
///
/// The textures that have no Minecraft path get `block/<name>` (`item/<name>`, and the plural
/// folders before 1.13), and their pictures are returned as files beside the model. Item
/// overrides are written before 1.21.4; from 1.21.4 an item definition replaces them, which
/// [`crate::export_pack`] writes.
pub fn export(doc: &Document, options: &JsonOptions) -> Result<JsonExport, McError> {
    let prepared = prepare(
        doc,
        options.version,
        |item| {
            if item {
                Target::JavaItem {
                    version: options.version,
                }
            } else {
                Target::JavaBlock {
                    version: options.version,
                }
            }
        },
        options.force,
        &|dir, name| format!("{dir}/{name}"),
    )?;
    let mut notices = Vec::new();
    let json = model_text(&prepared, Mode::Plain, &mut notices);
    let mut files = Vec::new();
    for &at in &prepared.fresh {
        let Some(entry) = prepared.doc.textures.entries.get(at) else {
            continue;
        };
        let path = texture_path(&entry.texture.source);
        let name = path.rsplit('/').next().unwrap_or(&path);
        files.push(ExportFile {
            path: format!("{name}.png"),
            bytes: png(entry)?,
        });
    }
    if !files.is_empty() {
        notices.push(Notice::UnsavedTextures { count: files.len() });
    }
    Ok(JsonExport {
        json,
        files,
        notices,
    })
}
