//! Resource pack export (spec: interop 9.1, 31.3, 31.4).
//!
//! A pack holds `pack.mcmeta` for the target version, the model under
//! `assets/<namespace>/models/<block|item>/<id>.json`, the textures as PNG files, and for the
//! versions that need them the item definition `assets/<namespace>/items/<id>.json`. A block can
//! replace vanilla blocks (their blockstate and block item are written); the default replaces
//! `glass` and `white_stained_glass`, as the reference does. The pack is checked with
//! `cuboid-validate`'s resource pack rules before anything is written, and the findings come back
//! as notices.
//!
//! The same files can be written to a folder or packed into a zip with fixed entry order, fixed
//! timestamps and no leading slash in the names.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Cursor, Write};
use std::path::Path;

use cuboid_assets::mc_versions::{TextureDirs, mc_versions};
use cuboid_io::text::push_json_string;
use cuboid_model::Document;
use cuboid_validate::{PackInfo, PackMeta, PackModel, Profile, Target, validate};

use crate::json_export::{Mode, model_text, png, prepare, texture_path};
use crate::jv::safe_path;
use crate::{McError, McVersion, Notice, PackFormat};

/// How a pack is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOptions {
    /// The game version.
    pub version: McVersion,
    /// The namespace of the model.
    pub namespace: String,
    /// The id of the model, possibly with folders.
    pub id: String,
    /// The vanilla blocks (for a block model) or items (for an item model) that show the model
    /// instead. `None` replaces `glass` and `white_stained_glass` for a block and nothing for an
    /// item.
    pub replace: Option<Vec<String>>,
    /// The text of `pack.mcmeta`; the model description or name when `None`.
    pub description: Option<String>,
    /// The pack format, when the version table has none for the version or the table is wrong.
    pub pack_format: Option<PackFormat>,
    /// Write the pack although the model breaks the rules of the version.
    pub force: bool,
}

impl PackOptions {
    /// A pack for a version with the model `minecraft:cubik_model`.
    pub fn new(version: McVersion) -> Self {
        Self {
            version,
            namespace: "minecraft".to_owned(),
            id: "cubik_model".to_owned(),
            replace: None,
            description: None,
            pack_format: None,
            force: false,
        }
    }
}

/// One file of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    /// The path inside the pack, with `/` separators and no leading slash.
    pub path: String,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// A built pack.
#[derive(Debug)]
pub struct PackExport {
    /// The files in a fixed order.
    pub files: Vec<PackFile>,
    /// The pack format written to `pack.mcmeta`.
    pub format: PackFormat,
    /// What the pack does not hold, and the findings of the validator.
    pub notices: Vec<Notice>,
}

impl PackExport {
    /// The pack as a zip archive: entries in the order of [`PackExport::files`], the timestamp
    /// of every entry 1980-01-01.
    pub fn to_zip(&self) -> Result<Vec<u8>, McError> {
        let archive = |error: &dyn std::fmt::Display| McError::Archive(error.to_string());
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for file in &self.files {
            if !safe_path(&file.path) {
                return Err(McError::UnsafePath(file.path.clone()));
            }
            zip.start_file(file.path.as_str(), options)
                .map_err(|e| archive(&e))?;
            zip.write_all(&file.bytes).map_err(|e| archive(&e))?;
        }
        let cursor = zip.finish().map_err(|e| archive(&e))?;
        Ok(cursor.into_inner())
    }

    /// Writes the pack into a folder, each file atomically. Folders are created as needed.
    pub fn write_folder(&self, dir: &Path) -> io::Result<()> {
        for file in &self.files {
            if !safe_path(&file.path) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    McError::UnsafePath(file.path.clone()).to_string(),
                ));
            }
            let target = file
                .path
                .split('/')
                .fold(dir.to_path_buf(), |p, part| p.join(part));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            cuboid_io::write_atomic(&target, &file.bytes)?;
        }
        Ok(())
    }
}

/// A lower-case resource name: letters, digits, `_`, `-`, `.` and, when `slash`, `/`.
fn name_ok(name: &str, slash: bool) -> bool {
    let chars = name.chars().all(|c| {
        c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || matches!(c, '_' | '-' | '.')
            || (slash && c == '/')
    });
    chars && safe_path(name)
}

/// `{"model": {"type": "minecraft:model", "model": "<model>"}}`: the item definition of a plain
/// model (interop 31.3).
fn definition(model: &str) -> String {
    let mut out =
        String::from("{\n\t\"model\": {\n\t\t\"type\": \"minecraft:model\",\n\t\t\"model\": ");
    push_json_string(&mut out, model);
    out.push_str("\n\t}\n}");
    out
}

fn parent_model(parent: &str) -> String {
    let mut out = String::from("{\n\t\"parent\": ");
    push_json_string(&mut out, parent);
    out.push_str("\n}");
    out
}

fn blockstate(key: &str, model: &str) -> String {
    let mut out = format!("{{\n\t\"variants\": {{\n\t\t\"{key}\": {{ \"model\": ");
    push_json_string(&mut out, model);
    out.push_str(" }\n\t}\n}");
    out
}

fn mcmeta(format: PackFormat, description: &str) -> String {
    let show = |f: PackFormat| {
        if f.minor == 0 {
            f.major.to_string()
        } else {
            format!("[{}, {}]", f.major, f.minor)
        }
    };
    let mut text = String::from("{\n\t\"pack\": {\n");
    if format.major > mc_versions().min_max_format_above {
        let shown = show(format);
        text.push_str(&format!(
            "\t\t\"min_format\": {shown},\n\t\t\"max_format\": {shown},\n"
        ));
    } else {
        text.push_str(&format!("\t\t\"pack_format\": {},\n", format.major));
    }
    text.push_str("\t\t\"description\": ");
    push_json_string(&mut text, description);
    text.push_str("\n\t}\n}");
    text
}

/// Builds a resource pack for the model. See [`PackOptions`].
///
/// The textures without a Minecraft path are written under `block/<id>/<name>` (`item/...`), so
/// they cannot replace a vanilla texture. Source pictures are re-encoded as PNG.
pub fn export_pack(doc: &Document, options: &PackOptions) -> Result<PackExport, McError> {
    let (version, namespace, id) = (
        options.version,
        options.namespace.as_str(),
        options.id.as_str(),
    );
    if !name_ok(namespace, false) {
        return Err(McError::UnsafePath(namespace.to_owned()));
    }
    if !name_ok(id, true) {
        return Err(McError::UnsafePath(id.to_owned()));
    }
    if let Some(bad) = options
        .replace
        .iter()
        .flatten()
        .find(|name| !name_ok(name, false))
    {
        return Err(McError::UnsafePath(bad.clone()));
    }
    let format = options
        .pack_format
        .or_else(|| mc_versions().pack_format_for(version).map(|e| e.format))
        .ok_or_else(|| McError::PackFormat(version.to_string()))?;

    let prepared = prepare(
        doc,
        version,
        |_| Target::ResourcePack { version },
        options.force,
        &|dir, name| {
            if namespace == "minecraft" {
                format!("{dir}/{id}/{name}")
            } else {
                format!("{namespace}:{dir}/{id}/{name}")
            }
        },
    )?;
    let mut notices = Vec::new();
    let item = prepared.item;
    let legacy = prepared.tier.texture_dirs == TextureDirs::Plural;
    let kind_dir = if item { "item" } else { "block" };

    let mut files: Vec<PackFile> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |path: String, bytes: Vec<u8>| -> Result<(), McError> {
        if !seen.insert(path.clone()) {
            return Err(McError::PathClash(path));
        }
        files.push(PackFile { path, bytes });
        Ok(())
    };

    let description = options
        .description
        .clone()
        .or_else(|| Some(doc.props.general.description.clone()).filter(|text| !text.is_empty()))
        .unwrap_or_else(|| doc.props.general.name.clone());
    add(
        "pack.mcmeta".to_owned(),
        mcmeta(format, &description).into_bytes(),
    )?;

    let model_path = format!("assets/{namespace}/models/{kind_dir}/{id}.json");
    let reference = format!("{namespace}:{kind_dir}/{id}");
    let default_replace: &[&str] = if item {
        &[]
    } else {
        &["glass", "white_stained_glass"]
    };
    let replace: Vec<String> = options.replace.clone().unwrap_or_else(|| {
        default_replace
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    });
    if !item {
        for name in &replace {
            let model = if legacy && namespace == "minecraft" {
                id.to_owned()
            } else if legacy {
                format!("{namespace}:{id}")
            } else {
                reference.clone()
            };
            let key = if legacy { "normal" } else { "" };
            add(
                format!("assets/minecraft/blockstates/{name}.json"),
                blockstate(key, &model).into_bytes(),
            )?;
        }
    }
    add(
        model_path.clone(),
        model_text(&prepared, Mode::Pack, &mut notices).into_bytes(),
    )?;
    if prepared.tier.item_definitions {
        if item {
            add(
                format!("assets/{namespace}/items/{id}.json"),
                definition(&reference).into_bytes(),
            )?;
        }
        for name in &replace {
            add(
                format!("assets/minecraft/items/{name}.json"),
                definition(&if item {
                    reference.clone()
                } else {
                    format!("{namespace}:block/{id}")
                })
                .into_bytes(),
            )?;
        }
    } else {
        for name in &replace {
            let parent = if item || !legacy || namespace != "minecraft" {
                reference.clone()
            } else {
                format!("block/{id}")
            };
            let path = format!("assets/minecraft/models/item/{name}.json");
            if path != model_path {
                add(path, parent_model(&parent).into_bytes())?;
            }
        }
    }

    // Textures, sorted by path. Two textures with one path keep the first.
    let mut pictures: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in &prepared.doc.textures.entries {
        let path = texture_path(&entry.texture.source);
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        let (ns, rest) = path.split_once(':').unwrap_or(("minecraft", path.as_str()));
        if !safe_path(ns) || ns.contains('/') || !safe_path(rest) {
            return Err(McError::UnsafePath(path));
        }
        let file = format!("assets/{ns}/textures/{rest}.png");
        let bytes = png(entry)?;
        match pictures.get(&file) {
            Some(first) if *first != bytes => notices.push(Notice::TextureClash { path }),
            Some(_) => {}
            None => {
                pictures.insert(file, bytes);
            }
        }
    }
    if !prepared.fresh.is_empty() {
        notices.push(Notice::UnsavedTextures {
            count: prepared.fresh.len(),
        });
    }
    for (path, bytes) in pictures {
        add(path, bytes)?;
    }

    // The validator reads the pack as the game would.
    let mut profile = Profile::new(Target::ResourcePack { version });
    let declared = |f: PackFormat| Some(f);
    profile.pack = Some(PackInfo {
        meta: Some(if format.major > mc_versions().min_max_format_above {
            PackMeta {
                min_format: declared(format),
                max_format: declared(format),
                ..PackMeta::default()
            }
        } else {
            PackMeta {
                pack_format: declared(format),
                ..PackMeta::default()
            }
        }),
        files: files.iter().map(|file| file.path.clone()).collect(),
        model: Some(PackModel {
            namespace: namespace.to_owned(),
            id: id.to_owned(),
        }),
    });
    notices.extend(
        validate(&prepared.doc, &profile)
            .into_iter()
            .map(Notice::Validation),
    );

    Ok(PackExport {
        files,
        format,
        notices,
    })
}
