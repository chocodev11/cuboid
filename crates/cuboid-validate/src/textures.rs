//! Texture-list rules: unused entries, Minecraft paths, folder names, shape, colour budget and
//! the size `.cubik` can store.

use cuboid_assets::mc_versions::TextureDirs;
use cuboid_model::image::{Image, check_size};
use cuboid_model::{Document, Label, TextureEntry, TextureSource};
use rustc_hash::FxHashSet;

use crate::codes::*;
use crate::ctx::{Ctx, Scan};
use crate::issue::{FixOp, Issue, Severity};
use crate::profile::{Flavor, Profile};
use crate::resource_id;

/// The Minecraft path of an entry as the exporter writes it (interop 7.4): the stored path, else
/// the part of the source file path after the last `/textures/`, else the file name; all without
/// the extension. Empty when the texture was never saved.
pub(crate) fn effective_path(source: &TextureSource) -> String {
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
    let name = tail.rsplit_once('.').map_or(tail, |(stem, _)| stem);
    name.to_owned()
}

/// The folder names that belong to the other layout, as `(wrong, right)`.
fn swapped_dirs(dirs: TextureDirs) -> [(&'static str, &'static str); 2] {
    match dirs {
        TextureDirs::Singular => [("blocks", "block"), ("items", "item")],
        TextureDirs::Plural => [("block", "blocks"), ("item", "items")],
    }
}

/// Counts the distinct colours of the visible pixels, stopping once `limit` is passed.
fn colors_over(images: &[&Image], limit: usize) -> Option<usize> {
    let mut seen: FxHashSet<u32> = FxHashSet::default();
    for image in images {
        for px in image.rgba().chunks_exact(4) {
            if let [r, g, b, a] = *px
                && a > 0
            {
                seen.insert(u32::from_be_bytes([0, r, g, b]));
                if seen.len() > limit {
                    return Some(seen.len());
                }
            }
        }
    }
    None
}

pub(crate) fn run(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, profile: &Profile) {
    match ctx.params.flavor {
        Flavor::Free => {}
        Flavor::Cubik => cubik(ctx, doc),
        Flavor::Java => java(ctx, doc, scan, profile),
    }
}

fn cubik(ctx: &mut Ctx, doc: &Document) {
    for entry in &doc.textures.entries {
        let image = &entry.texture.image;
        if let Err(err) = check_size(image.w, image.h) {
            ctx.emit(CUBIK_TEXTURE_SIZE, || {
                Issue::new(
                    CUBIK_TEXTURE_SIZE,
                    Severity::Error,
                    Vec::new(),
                    format!("texture \"{}\": {err}", entry.label),
                    "Scale the texture down before saving",
                )
            });
        }
    }
}

fn java(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, profile: &Profile) {
    let entries = &doc.textures.entries;
    // A shortcut `#label` makes its target used.
    let mut used = scan.used.clone();
    for (at, entry) in entries.iter().enumerate() {
        let in_use = used.get(at).copied().unwrap_or(false);
        let path = entry.texture.source.mc_path.as_str();
        if let Some(target) = path.strip_prefix('#').filter(|_| in_use)
            && let Some(flag) = scan.tex_index.get(target).and_then(|&t| used.get_mut(t))
        {
            *flag = true;
        }
    }

    for (at, entry) in entries.iter().enumerate() {
        let label: &Label = &entry.label;
        let image = &entry.texture.image;
        if !used.get(at).copied().unwrap_or(false) && &**label != "particle" {
            ctx.emit(TEX_UNUSED, || {
                Issue::new(
                    TEX_UNUSED,
                    Severity::Warning,
                    Vec::new(),
                    format!("texture \"{label}\" is used by no visible face"),
                    "Remove the entry, or assign it to a face",
                )
                .fixed_by(vec![FixOp::RemoveTexture {
                    label: label.clone(),
                }])
            });
        }
        path_rules(ctx, scan, entry, &effective_path(&entry.texture.source));
        if image.w != image.h {
            ctx.emit(TEX_NOT_SQUARE, || {
                Issue::new(
                    TEX_NOT_SQUARE,
                    Severity::Warning,
                    Vec::new(),
                    format!("texture \"{label}\" is {} x {}, not square", image.w, image.h),
                    "Minecraft does not load a texture that is not square, except an animation strip",
                )
            });
        }
    }

    if let Some(limit) = profile.max_colors.filter(|_| ctx.on(TEX_COLORS)) {
        let images: Vec<&Image> = entries
            .iter()
            .zip(&used)
            .filter(|&(_, &in_use)| in_use)
            .map(|(entry, _)| &*entry.texture.image)
            .collect();
        if let Some(count) = colors_over(&images, limit as usize) {
            let more = if count > limit as usize {
                "more than "
            } else {
                ""
            };
            ctx.emit(TEX_COLORS, || {
                Issue::new(
                    TEX_COLORS,
                    Severity::Warning,
                    Vec::new(),
                    format!(
                        "the textures in use hold {more}{limit} colours, over the budget of {limit}"
                    ),
                    "Merge similar colours or raise the budget",
                )
            });
        }
    }
}

fn path_rules(ctx: &mut Ctx, scan: &Scan<'_>, entry: &TextureEntry, path: &str) {
    let label = &entry.label;
    if path.is_empty() {
        ctx.emit(TEX_NO_PATH, || {
            Issue::new(
                TEX_NO_PATH,
                Severity::Warning,
                Vec::new(),
                format!("texture \"{label}\" has no Minecraft path"),
                "Set a path such as block/name; the export would write the PNG beside the JSON instead",
            )
        });
        return;
    }
    if let Some(target) = path.strip_prefix('#') {
        if !scan.tex_index.contains_key(target) {
            ctx.emit(TEX_UNRESOLVED, || {
                Issue::new(
                    TEX_UNRESOLVED,
                    Severity::Error,
                    Vec::new(),
                    format!("texture \"{label}\" is a shortcut to #{target}, which does not exist"),
                    "Point the shortcut at an existing label",
                )
            });
        }
        return;
    }
    if let Some(why) = resource_id::problem(path) {
        ctx.emit(ID_CHARS, || {
            Issue::new(
                ID_CHARS,
                Severity::Error,
                Vec::new(),
                format!("texture \"{label}\", path \"{path}\": {why}"),
                "Use lower-case letters, digits and _ - . / only",
            )
        });
    }
    let Some(dirs) = ctx.params.dirs else { return };
    let (prefix, body) = path.split_at(path.find(':').map_or(0, |at| at + 1));
    let (first, rest) = body.split_once('/').unwrap_or((body, ""));
    let Some((wrong, right)) = swapped_dirs(dirs)
        .into_iter()
        .find(|&(wrong, _)| wrong == first)
    else {
        return;
    };
    ctx.emit(TEX_LAYOUT, || {
        let fixed = if rest.is_empty() {
            format!("{prefix}{right}")
        } else {
            format!("{prefix}{right}/{rest}")
        };
        Issue::new(
            TEX_LAYOUT,
            Severity::Error,
            Vec::new(),
            format!("texture \"{label}\", path \"{path}\": this game version reads {right}/, not {wrong}/"),
            format!("Use \"{fixed}\""),
        )
        .fixed_by(vec![FixOp::SetTexturePath {
            label: label.clone(),
            path: fixed,
        }])
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: Option<&str>, mc: &str) -> TextureSource {
        TextureSource {
            path: path.map(str::to_owned),
            mc_path: mc.to_owned(),
            ..TextureSource::default()
        }
    }

    #[test]
    fn the_path_follows_interop_7_4() {
        assert_eq!(effective_path(&source(None, "")), "");
        assert_eq!(effective_path(&source(None, "block/x")), "block/x");
        let pack = source(
            Some("C:/packs/a/assets/minecraft/textures/block/stone.png"),
            "",
        );
        assert_eq!(effective_path(&pack), "block/stone");
        assert_eq!(effective_path(&source(Some("C:/tmp/rock.png"), "")), "rock");
        // `/textures/` at index 0 does not count.
        assert_eq!(
            effective_path(&source(Some("/textures/rock.png"), "")),
            "rock"
        );
    }
}
