//! Resource-pack rules: `pack.mcmeta` formats (interop 9.1, 31.4), item definitions (31.3) and
//! missing files.

use cuboid_assets::mc_versions::{PackFormat, mc_versions};
use cuboid_model::{Document, ModelKind};
use rustc_hash::FxHashSet;

use crate::codes::*;
use crate::ctx::Ctx;
use crate::issue::{Issue, Severity};
use crate::profile::{PackInfo, PackMeta, Profile};
use crate::resource_id;
use crate::textures::effective_path;

fn show(format: PackFormat) -> String {
    if format.minor == 0 {
        format.major.to_string()
    } else {
        format!("{}.{}", format.major, format.minor)
    }
}

fn error(code: &'static str, message: String, hint: &str) -> Issue {
    Issue::new(code, Severity::Error, Vec::new(), message, hint)
}

pub(crate) fn run(ctx: &mut Ctx, doc: &Document, profile: &Profile) {
    let (Some(pack), Some(version)) = (&profile.pack, ctx.params.version) else {
        return;
    };
    if !ctx.params.pack {
        return;
    }
    let table = mc_versions();
    let known = table.pack_format_for(version);
    formats(
        ctx,
        pack,
        known.map(|entry| (entry.format, entry.verified)),
        version.to_string(),
    );
    if known.is_none() {
        ctx.emit(PACK_FORMAT_UNKNOWN, || {
            Issue::new(
                PACK_FORMAT_UNKNOWN,
                Severity::Info,
                Vec::new(),
                format!("the version table has no pack format for {version}"),
                "Check the format against the game; the table lists only formats the specs confirm (unverified for this version)",
            )
        });
    }
    files(ctx, doc, pack);
}

fn formats(ctx: &mut Ctx, pack: &PackInfo, known: Option<(PackFormat, bool)>, version: String) {
    let threshold = mc_versions().min_max_format_above;
    let Some(meta) = &pack.meta else {
        ctx.emit(PACK_MCMETA, || {
            error(
                PACK_MCMETA,
                "the pack has no pack.mcmeta".to_owned(),
                "Add a pack.mcmeta with a pack section",
            )
        });
        return;
    };
    let declared = [meta.pack_format, meta.min_format, meta.max_format];
    if declared.iter().all(Option::is_none) {
        ctx.emit(PACK_MCMETA, || {
            error(
                PACK_MCMETA,
                "pack.mcmeta declares no pack_format, min_format or max_format".to_owned(),
                "Declare the format of the game version",
            )
        });
    }
    if let Some((format, verified)) = known
        && !declared.iter().all(Option::is_none)
        && !covers(meta, format)
    {
        let severity = if verified {
            Severity::Error
        } else {
            Severity::Warning
        };
        ctx.emit(PACK_FORMAT, || {
            Issue::new(
                PACK_FORMAT,
                severity,
                Vec::new(),
                format!(
                    "pack.mcmeta does not cover pack format {} of game version {version}",
                    show(format)
                ),
                "Declare this format as pack_format, or inside min_format and max_format",
            )
        });
    }
    let top = declared
        .into_iter()
        .flatten()
        .chain(known.map(|(format, _)| format))
        .map(|format| format.major)
        .max()
        .unwrap_or(0);
    if top > threshold {
        if meta.min_format.is_none() || meta.max_format.is_none() {
            ctx.emit(PACK_FORMAT_KEYS, || {
                error(
                    PACK_FORMAT_KEYS,
                    format!("pack formats above {threshold} need both min_format and max_format"),
                    "Add min_format and max_format to the pack section",
                )
            });
        }
        if meta.supported_formats {
            ctx.emit(PACK_FORMAT_KEYS, || {
                error(
                    PACK_FORMAT_KEYS,
                    format!(
                        "supported_formats is rejected as deprecated above pack format {threshold}"
                    ),
                    "Remove supported_formats and use min_format and max_format",
                )
            });
        }
    }
}

fn covers(meta: &PackMeta, format: PackFormat) -> bool {
    let exact = meta.pack_format == Some(format);
    let in_range = matches!((meta.min_format, meta.max_format), (Some(lo), Some(hi)) if lo <= format && format <= hi);
    exact || in_range
}

fn files(ctx: &mut Ctx, doc: &Document, pack: &PackInfo) {
    let Some(model) = &pack.model else { return };
    let (ns, id) = (&model.namespace, &model.id);
    if let Some(why) = resource_id::problem(&format!("{ns}:{id}")) {
        ctx.emit(ID_CHARS, || {
            error(
                ID_CHARS,
                format!("pack model \"{ns}:{id}\": {why}"),
                "Use lower-case letters, digits and _ - . / only",
            )
        });
    }
    let present: FxHashSet<&str> = pack.files.iter().map(String::as_str).collect();
    let is_item = doc.props.minecraft.kind == ModelKind::Item;
    let dir = if is_item { "item" } else { "block" };

    let model_file = format!("assets/{ns}/models/{dir}/{id}.json");
    if !present.contains(model_file.as_str()) {
        ctx.emit(PACK_FILE_MISSING, || {
            error(
                PACK_FILE_MISSING,
                format!("the pack lacks {model_file}"),
                "Export the model into the pack",
            )
        });
    }
    if is_item && ctx.params.item_defs {
        let definition = format!("assets/{ns}/items/{id}.json");
        if !present.contains(definition.as_str()) {
            let severity = if ctx.params.item_defs_verified {
                Severity::Error
            } else {
                Severity::Warning
            };
            ctx.emit(PACK_ITEMDEF, || {
                Issue::new(
                    PACK_ITEMDEF,
                    severity,
                    Vec::new(),
                    format!("the pack lacks the item definition {definition}"),
                    "From 1.21.4 an item needs an item definition that names its model",
                )
            });
        }
    }
    for entry in &doc.textures.entries {
        let path = effective_path(&entry.texture.source);
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        let (tex_ns, tex_path) = path.split_once(':').unwrap_or(("minecraft", path.as_str()));
        let file = format!("assets/{tex_ns}/textures/{tex_path}.png");
        if !present.contains(file.as_str()) {
            ctx.emit(PACK_FILE_MISSING, || {
                error(
                    PACK_FILE_MISSING,
                    format!(
                        "texture \"{}\" needs {file}, which the pack lacks",
                        entry.label
                    ),
                    "Copy the PNG into the pack",
                )
            });
        }
    }
}
