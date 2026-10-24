//! The `textures` map of a model: finding the picture of every entry (interop 8.4), aliases
//! (`"#other"`) and the checkerboard of a missing picture.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use cuboid_io::texture_files::{Refusal, TextureFiles};
use cuboid_model::image::{Image, ImageFileKind, RgbaBuf, decode_file, signature};
use cuboid_model::{Label, Texture, TextureEntry, TextureSource};

use crate::jv::{clean_label, safe_path};
use crate::{Limits, McError, Notice};

/// The code page of the texture signatures.
const CODE_PAGE: u16 = 1252;

/// How many `#alias` links are followed.
const ALIAS_DEPTH: usize = 16;

/// A texture entry after the parents were merged.
pub(crate) struct Merged {
    pub key: String,
    pub value: String,
    /// The folder of the model file that defines it, for the file search.
    pub folder: String,
    /// Defined by a built-in parent.
    pub builtin: bool,
}

/// The entries of a model and the label of every key.
pub(crate) struct Table {
    pub entries: Vec<TextureEntry>,
    /// Keys of the file to labels.
    pub labels: HashMap<String, Label>,
    /// Whether the entry of the same position comes from a built-in parent.
    pub builtin: Vec<bool>,
    taken: HashSet<String>,
}

/// A 16 x 16 checkerboard for a texture that has no picture.
pub(crate) fn missing_picture() -> Texture {
    let mut buf = RgbaBuf::new(16, 16);
    for y in 0..16_i32 {
        for x in 0..16_i32 {
            let color = if (x / 8 + y / 8) % 2 == 0 {
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

impl Table {
    /// The label for a key that no entry has: a new entry with the checkerboard.
    pub fn label_or_add(&mut self, key: &str, notices: &mut Vec<Notice>) -> Label {
        if let Some(label) = self.labels.get(key) {
            return label.clone();
        }
        let label = self.fresh_label(key);
        notices.push(Notice::TextureMissing {
            name: key.to_owned(),
        });
        self.labels.insert(key.to_owned(), label.clone());
        self.entries.push(TextureEntry {
            label: label.clone(),
            texture: missing_picture(),
        });
        self.builtin.push(false);
        label
    }

    fn fresh_label(&mut self, key: &str) -> Label {
        let mut base = clean_label(key);
        if base.is_empty() {
            base = format!("texture{}", self.taken.len());
        }
        let mut label = base.clone();
        let mut n = 1;
        while self.taken.contains(&label) {
            label = format!("{base}_{n}");
            n += 1;
        }
        self.taken.insert(label.clone());
        Label::from(label)
    }
}

/// Reads the pictures of the merged entries. Every entry makes an entry of the model, so that the
/// faces find their labels; one without a usable picture gets a checkerboard.
pub(crate) fn read(
    merged: &[Merged],
    files: &dyn TextureFiles,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) -> Result<Table, McError> {
    if merged.len() > limits.max_textures as usize {
        return Err(McError::Limit("texture entries"));
    }
    let mut table = Table {
        entries: Vec::with_capacity(merged.len()),
        labels: HashMap::new(),
        builtin: Vec::with_capacity(merged.len()),
        taken: HashSet::new(),
    };
    for item in merged {
        let label = table.fresh_label(&item.key);
        table.labels.insert(item.key.clone(), label);
    }

    // The files together are bounded like the texels they may decode to.
    let mut read_left = limits.max_texels.saturating_mul(4);
    let mut texels = 0_u64;
    let mut loaded: HashMap<&str, Texture> = HashMap::new();
    for item in merged.iter().filter(|m| !m.value.starts_with('#')) {
        if let Some(texture) = load(item, files, limits, &mut read_left, &mut texels, notices) {
            loaded.insert(item.key.as_str(), texture);
        }
    }
    if texels > limits.max_texels {
        notices.push(Notice::TexelBudget);
    }

    let by_key: HashMap<&str, &Merged> = merged.iter().map(|m| (m.key.as_str(), m)).collect();
    for item in merged {
        let Some(label) = table.labels.get(&item.key).cloned() else {
            continue;
        };
        let texture = match item.value.strip_prefix('#') {
            None => loaded.get(item.key.as_str()).cloned(),
            Some(target) => alias(target, &by_key, &loaded, &table.labels),
        };
        let texture = texture.unwrap_or_else(|| {
            let mut texture = missing_picture();
            if let Some(direct) = item.value.strip_prefix('#') {
                if let Some(label) = table.labels.get(direct) {
                    texture.source.mc_path = format!("#{label}");
                }
                notices.push(Notice::TextureMissing {
                    name: item.key.clone(),
                });
            } else if let Some((ns, path)) = split(&item.value) {
                // The path is kept, so the model writes back as it was read.
                texture.source.mc_path = mc_path(ns, path);
            }
            texture
        });
        table.entries.push(TextureEntry { label, texture });
        table.builtin.push(item.builtin);
    }
    Ok(table)
}

/// The picture of an alias entry, which keeps the shortcut as its Minecraft path.
fn alias(
    target: &str,
    by_key: &HashMap<&str, &Merged>,
    loaded: &HashMap<&str, Texture>,
    labels: &HashMap<String, Label>,
) -> Option<Texture> {
    let mut at = target;
    for _ in 0..ALIAS_DEPTH {
        let entry = by_key.get(at)?;
        match entry.value.strip_prefix('#') {
            Some(next) => at = next,
            None => {
                let mut texture = loaded.get(at)?.clone();
                let direct = labels.get(target)?;
                texture.source.mc_path = format!("#{direct}");
                texture.source.path = None;
                texture.source.signature = None;
                texture.source.file_size = 0;
                return Some(texture);
            }
        }
    }
    None
}

/// The namespace and path of a texture value, or `None` when it is not a safe resource path.
fn split(value: &str) -> Option<(&str, &str)> {
    let (ns, path) = value.split_once(':').unwrap_or(("minecraft", value));
    (safe_path(ns) && !ns.contains('/') && safe_path(path)).then_some((ns, path))
}

/// The Minecraft path of a texture: the namespace is left out for `minecraft`.
fn mc_path(ns: &str, path: &str) -> String {
    if ns == "minecraft" {
        path.to_owned()
    } else {
        format!("{ns}:{path}")
    }
}

fn load(
    item: &Merged,
    files: &dyn TextureFiles,
    limits: &Limits,
    read_left: &mut u64,
    texels: &mut u64,
    notices: &mut Vec<Notice>,
) -> Option<Texture> {
    let missing = |notices: &mut Vec<Notice>| {
        notices.push(Notice::TextureMissing {
            name: item.key.clone(),
        });
        None
    };
    let Some((ns, path)) = split(&item.value) else {
        return missing(notices);
    };
    let mc_path = mc_path(ns, path);
    let mut refused = None;
    let mut found = None;
    for candidate in candidates(ns, path, &item.folder) {
        match files.read(&candidate) {
            Ok(Some(bytes)) => {
                found = Some((candidate, bytes));
                break;
            }
            Ok(None) => {}
            Err(reason) => {
                refused.get_or_insert((candidate, reason));
            }
        }
    }
    let Some((file, bytes)) = found else {
        return match refused {
            Some((file, reason)) => {
                notices.push(Notice::TextureRefused {
                    name: item.key.clone(),
                    path: file.to_string_lossy().into_owned(),
                    reason,
                });
                None
            }
            None => missing(notices),
        };
    };
    let len = bytes.len() as u64;
    if len > *read_left {
        notices.push(Notice::TextureRefused {
            name: item.key.clone(),
            path: file.to_string_lossy().into_owned(),
            reason: Refusal::TooLarge,
        });
        return None;
    }
    *read_left -= len;
    let image = decode_file(&bytes, ImageFileKind::from_extension("png")).ok();
    let Some(image) = image else {
        return missing(notices);
    };
    let total = texels.saturating_add(u64::from(image.w) * u64::from(image.h));
    if total > limits.max_texels {
        // Anything after this is refused too.
        *texels = u64::MAX;
        return None;
    }
    *texels = total;
    let mut texture = Texture::new(Arc::new(image));
    let stem = path.rsplit('/').next().unwrap_or(path);
    texture.name = Label::from(stem);
    texture.source = TextureSource {
        path: Some(file.to_string_lossy().replace('\\', "/")),
        file_name: Some(stem.to_owned()),
        mc_path,
        signature: Some(signature(&bytes, CODE_PAGE)),
        file_size: len,
    };
    Some(texture)
}

/// The files that can hold a texture, in the order of interop 8.4. `folder` is the model's
/// folder with a trailing `/`.
fn candidates(ns: &str, path: &str, folder: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let join = |dir: &str, rest: String| {
        if dir.is_empty() {
            PathBuf::from(rest)
        } else {
            PathBuf::from(format!("{dir}/{rest}"))
        }
    };
    let marker = |name: &str| {
        let key = format!("/{name}/");
        folder
            .rfind(&key)
            .filter(|at| *at > 0)
            .and_then(|at| folder.get(..at + key.len() - 1))
    };
    if let Some(prefix) = marker(ns).or_else(|| marker("minecraft")) {
        out.push(join(prefix, format!("textures/{path}.png")));
    }
    let dir = folder.trim_end_matches('/');
    out.push(join(dir, format!("textures/{path}.png")));
    out.push(join(dir, format!("{path}.png")));
    let last = path.rsplit('/').next().unwrap_or(path);
    out.push(join(dir, format!("{last}.png")));
    out.dedup();
    out
}
