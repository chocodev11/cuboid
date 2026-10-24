//! The texture entries of a project: reading them in, and the names they carry in both
//! directions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_io::texture_files::{Form, Refusal, TextureFiles, form};
use cuboid_model::image::{Image, ImageFileKind, RgbaBuf, decode_file, encode_png, signature};
use cuboid_model::{Label, Texture, TextureEntry, TextureSource};
use glam::Vec2;
use serde_json::Value;

use crate::json::{num_at, text};
use crate::{Limits, Notice, data_url};

/// The code page of the texture signatures.
const CODE_PAGE: u16 = 1252;

/// The entries of the `textures` array, in the order the faces index them.
pub(crate) struct Table {
    pub entries: Vec<TextureEntry>,
    /// The size of the UV space of each entry, parallel to `entries`.
    pub units: Vec<Vec2>,
    /// The entries by the `uuid` the project gives them.
    pub by_uuid: HashMap<String, usize>,
    texels: u64,
}

/// How the UV rectangles of a face are measured.
#[derive(Clone, Copy)]
pub(crate) struct UvSpace {
    /// The resolution of the project, for faces without a texture and for formats that measure
    /// every face by it.
    pub project: Vec2,
    /// Every texture has a UV size of its own.
    pub per_texture: bool,
}

/// The label rule of the model (tools 5.9): ASCII letters, digits, `-` and `_`.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

/// The file name without its extension.
pub(crate) fn stem(name: &str) -> &str {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 8 => stem,
        _ => name,
    }
}

/// A 16 x 16 checkerboard for a texture that has no picture.
fn missing_picture() -> Texture {
    let mut buf = RgbaBuf::new(16, 16);
    for y in 0..16_i32 {
        for x in 0..16_i32 {
            let color = if (x / 4 + y / 4) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            buf.set(x, y, color);
        }
    }
    let mut texture = Texture::new(Arc::new(Image::from_rgba(buf)));
    texture.name = Label::from("e_error");
    texture
}

/// The Minecraft id of a texture: `namespace:folder/name`, the namespace left out for
/// `minecraft`.
fn minecraft_id(folder: &str, namespace: &str, name: &str) -> String {
    let stem = stem(name);
    if stem.is_empty() {
        return String::new();
    }
    let path = if folder.is_empty() {
        stem.to_owned()
    } else {
        format!("{}/{stem}", folder.trim_end_matches('/'))
    };
    if namespace.is_empty() || namespace == "minecraft" {
        path
    } else {
        format!("{namespace}:{path}")
    }
}

/// Reads the `textures` array. `dir` is the folder of the project, where relative paths start.
/// Every item makes an entry, so that the indices of the faces stay right; one without a usable
/// picture gets a checkerboard.
pub(crate) fn read(
    items: &[Value],
    dir: &Path,
    space: UvSpace,
    files: &dyn TextureFiles,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) -> Table {
    let mut table = Table {
        entries: Vec::with_capacity(items.len()),
        units: Vec::with_capacity(items.len()),
        by_uuid: HashMap::new(),
        texels: 0,
    };
    // The files together are bounded like the texels they may decode to.
    let mut read_left = limits.max_texels.saturating_mul(4);
    let mut taken: HashSet<String> = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let name = text(item, "name").unwrap_or_default();
        let folder = text(item, "folder").unwrap_or_default();
        let namespace = text(item, "namespace").unwrap_or_default();
        let shown = if name.is_empty() { "texture" } else { name };

        let mut label = clean(text(item, "id").unwrap_or_default());
        if label.is_empty() {
            label = clean(stem(name));
        }
        if label.is_empty() {
            label = format!("texture{index}");
        }
        let base = label.clone();
        let mut n = 1;
        while taken.contains(&label) {
            label = format!("{base}_{n}");
            n += 1;
        }
        taken.insert(label.clone());

        let units = if space.per_texture {
            let u = num_at(item, "uv_width").filter(|v| *v > 0.0);
            let v = num_at(item, "uv_height").filter(|v| *v > 0.0);
            Vec2::new(u.unwrap_or(space.project.x), v.unwrap_or(space.project.y))
        } else {
            space.project
        };

        let mut texture = None;
        let mut refused = None;
        let mut source = TextureSource::default();

        if let Some(url) = text(item, "source").and_then(data_url::payload) {
            let limit = usize::try_from(read_left).unwrap_or(usize::MAX);
            if let Some(bytes) = data_url::decode(url, limit) {
                read_left = read_left.saturating_sub(bytes.len() as u64);
                texture = decode_texels(&bytes, ImageFileKind::Other, &mut table.texels, limits);
            }
        }
        if texture.is_none() {
            let candidates = [text(item, "relative_path"), text(item, "path")];
            for (i, given) in candidates.into_iter().flatten().enumerate() {
                if given.is_empty() {
                    continue;
                }
                // The relative path starts in the folder of the project.
                let candidate = match (i, form(given)) {
                    (0, Form::Relative) => dir.join(given),
                    (_, Form::Relative | Form::Local | Form::Refused(_)) => PathBuf::from(given),
                };
                let bytes = match files.read(&candidate) {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => continue,
                    Err(reason) => {
                        refused.get_or_insert((candidate, reason));
                        continue;
                    }
                };
                let len = bytes.len() as u64;
                if len > read_left {
                    refused.get_or_insert((candidate, Refusal::TooLarge));
                    continue;
                }
                read_left -= len;
                let ext = candidate
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let kind = ImageFileKind::from_extension(&ext);
                if let Some(found) = decode_texels(&bytes, kind, &mut table.texels, limits) {
                    let full = candidate.to_string_lossy().replace('\\', "/");
                    source.path = Some(full);
                    source.file_name = Some(stem(&candidate.to_string_lossy()).to_owned());
                    source.signature = Some(signature(&bytes, CODE_PAGE));
                    source.file_size = len;
                    texture = Some(found);
                }
                // The first file that exists decides, also when it is no picture.
                break;
            }
        }

        let mut texture = texture.unwrap_or_else(|| {
            match refused {
                Some((path, reason)) => notices.push(Notice::TextureRefused {
                    name: shown.to_owned(),
                    path: path.to_string_lossy().into_owned(),
                    reason,
                }),
                None => notices.push(Notice::TextureMissing {
                    name: shown.to_owned(),
                }),
            }
            missing_picture()
        });
        if !name.is_empty() {
            texture.name = Label::from(stem(name));
        }
        source.mc_path = minecraft_id(folder, namespace, name);
        if source.file_name.is_none() && !name.is_empty() {
            source.file_name = Some(stem(name).to_owned());
        }
        texture.source = source;

        if let Some(uuid) = text(item, "uuid") {
            table.by_uuid.entry(uuid.to_owned()).or_insert(index);
        }
        table.entries.push(TextureEntry {
            label: Label::from(label),
            texture,
        });
        table.units.push(units);
    }
    if table.texels > limits.max_texels {
        notices.push(Notice::TexelBudget);
    }
    table
}

/// Decodes a picture within the budget of texels.
fn decode_texels(
    bytes: &[u8],
    kind: ImageFileKind,
    texels: &mut u64,
    limits: &Limits,
) -> Option<Texture> {
    let image = decode_file(bytes, kind).ok()?;
    let total = texels.saturating_add(u64::from(image.w) * u64::from(image.h));
    if total > limits.max_texels {
        // Anything after this is refused too.
        *texels = u64::MAX;
        return None;
    }
    *texels = total;
    Some(Texture::new(Arc::new(image)))
}

/// What the project's texture fields say about an entry that the model keeps: the folder, the
/// namespace and the file name with its extension.
pub(crate) fn names(entry: &TextureEntry) -> (String, String, String) {
    let id = entry.texture.source.mc_path.as_str();
    let id = if id.starts_with('#') { "" } else { id };
    let (namespace, path) = id.split_once(':').unwrap_or(("", id));
    let (folder, stem) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = if stem.is_empty() {
        let name = &*entry.texture.name;
        if name.is_empty() { &*entry.label } else { name }
    } else {
        stem
    };
    (
        folder.to_owned(),
        namespace.to_owned(),
        format!("{}.png", self::stem(stem)),
    )
}

/// The PNG of an entry as a `data:` URL, or `None` when it cannot be encoded.
pub(crate) fn data_url_of(entry: &TextureEntry) -> Option<String> {
    encode_png(&entry.texture.image)
        .ok()
        .map(|png| data_url::png_url(&png))
}
