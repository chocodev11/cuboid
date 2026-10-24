//! Rules about the model as a whole: kind, emptiness, budgets, overrides, display transforms,
//! the version and the parent id.

use cuboid_model::{DisplaySlotId, Document, ModelKind};
use glam::Vec3;

use crate::codes::*;
use crate::ctx::{Ctx, Scan};
use crate::issue::{FixOp, Issue, Severity};
use crate::profile::{Flavor, Profile};
use crate::resource_id;

/// The display slots in file order with their JSON names (interop 7.7).
const SLOTS: [(DisplaySlotId, &str); 8] = [
    (DisplaySlotId::ThirdRight, "thirdperson_righthand"),
    (DisplaySlotId::ThirdLeft, "thirdperson_lefthand"),
    (DisplaySlotId::FirstRight, "firstperson_righthand"),
    (DisplaySlotId::FirstLeft, "firstperson_lefthand"),
    (DisplaySlotId::Gui, "gui"),
    (DisplaySlotId::Head, "head"),
    (DisplaySlotId::Ground, "ground"),
    (DisplaySlotId::Fixed, "fixed"),
];

/// The game clamps a display translation to this many units (1/16 block) either way.
const TRANSLATION_LIMIT: f32 = 80.0;
/// The game clamps a display scale to this either way.
const SCALE_LIMIT: f32 = 4.0;

fn kind_name(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::None => "none",
        ModelKind::Block => "block",
        ModelKind::Item => "item",
        ModelKind::Entity => "entity",
        ModelKind::Optifine => "optifine",
        ModelKind::Map => "map",
    }
}

pub(crate) fn run(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, profile: &Profile) {
    let params = ctx.params.clone();
    let minecraft = &doc.props.minecraft;

    if let Some(max) = profile.max_elements
        && scan.elements.len() > max as usize
    {
        ctx.emit(BUDGET_ELEMENTS, || {
            Issue::new(
                BUDGET_ELEMENTS,
                Severity::Warning,
                Vec::new(),
                format!(
                    "the model has {} elements, over the budget of {max}",
                    scan.elements.len()
                ),
                "Merge or remove elements, or raise the budget",
            )
        });
    }

    match params.flavor {
        Flavor::Free => return,
        Flavor::Cubik => return cubik(ctx, doc),
        Flavor::Java => {}
    }

    if let Some(version) = params.version
        && params.tier.is_none()
    {
        ctx.emit(VERSION_UNKNOWN, || {
            Issue::new(
                VERSION_UNKNOWN,
                Severity::Warning,
                Vec::new(),
                format!("game version {version} is not in the version table"),
                "Rotation uses the strictest rule and folder names are not checked; pick a listed version",
            )
        });
    }

    let kind = minecraft.kind;
    let is_block_or_item = matches!(kind, ModelKind::Block | ModelKind::Item);
    let wrong = match params.kind {
        Some(want) => (kind != want).then_some(want),
        None => (!is_block_or_item).then_some(ModelKind::Block),
    };
    if let Some(want) = wrong {
        ctx.emit(MODEL_KIND, || {
            let severity = if is_block_or_item {
                Severity::Warning
            } else {
                Severity::Error
            };
            Issue::new(
                MODEL_KIND,
                severity,
                Vec::new(),
                format!(
                    "the model kind is {}, the target needs {}",
                    kind_name(kind),
                    kind_name(want)
                ),
                "Set the model kind in the model settings",
            )
            .fixed_by(vec![FixOp::SetModelKind { kind: want }])
        });
    }

    if scan.elements.is_empty() {
        ctx.emit(MODEL_EMPTY, || {
            let what = if doc.voxels.is_some() {
                "has voxels but no element; convert the voxels to elements first"
            } else if !doc.meshes.is_empty() {
                "has meshes but no element; convert the meshes to elements first"
            } else {
                "has no element to write"
            };
            Issue::new(
                MODEL_EMPTY,
                Severity::Error,
                Vec::new(),
                format!("the model {what}"),
                "The Java model JSON holds elements only",
            )
        });
    }

    if params.item_defs && !doc.overrides.is_empty() {
        ctx.emit(MODEL_OVERRIDES, || {
            Issue::new(
                MODEL_OVERRIDES,
                Severity::Warning,
                Vec::new(),
                format!(
                    "{} item override case(s) cannot be written into the model from 1.21.4",
                    doc.overrides.len()
                ),
                "Write them as an item definition, assets/<namespace>/items/<id>.json",
            )
        });
    }

    if !minecraft.parent.is_empty()
        && let Some(why) = resource_id::problem(&minecraft.parent)
    {
        ctx.emit(ID_CHARS, || {
            Issue::new(
                ID_CHARS,
                Severity::Error,
                Vec::new(),
                format!("parent \"{}\": {why}", minecraft.parent),
                "Use lower-case letters, digits and _ - . / only",
            )
        });
    }

    if minecraft.include_display {
        for ((id, name), slot) in SLOTS.into_iter().zip(&doc.display.slots) {
            if !slot.enabled {
                continue;
            }
            let (t, s) = (slot.translation, slot.scale);
            let (limit_t, limit_s) = (Vec3::splat(TRANSLATION_LIMIT), Vec3::splat(SCALE_LIMIT));
            if t.abs().cmple(limit_t).all() && s.abs().cmple(limit_s).all() {
                continue;
            }
            ctx.emit(DISPLAY_RANGE, || {
                Issue::new(
                    DISPLAY_RANGE,
                    Severity::Warning,
                    Vec::new(),
                    format!(
                        "display \"{name}\": translation {:?} or scale {:?} is outside +-{TRANSLATION_LIMIT} and +-{SCALE_LIMIT}",
                        t.to_array(),
                        s.to_array()
                    ),
                    "The game clamps the values, so the model looks different from the preview",
                )
                .fixed_by(vec![FixOp::SetDisplay {
                    slot: id,
                    translation: t.clamp(-limit_t, limit_t),
                    scale: s.clamp(-limit_s, limit_s),
                }])
            });
        }
    }
}

fn cubik(ctx: &mut Ctx, doc: &Document) {
    if !doc.overrides.is_empty() {
        ctx.emit(CUBIK_OVERRIDES, || {
            Issue::new(
                CUBIK_OVERRIDES,
                Severity::Warning,
                Vec::new(),
                format!(
                    "{} item override case(s) are not stored in a .cubik file",
                    doc.overrides.len()
                ),
                "Export the model as Minecraft JSON to keep them",
            )
        });
    }
    for ((_, name), slot) in SLOTS.into_iter().zip(&doc.display.slots) {
        if slot.scale.to_array().contains(&0.0) {
            ctx.emit(CUBIK_SCALE_ZERO, || {
                Issue::new(
                    CUBIK_SCALE_ZERO,
                    Severity::Info,
                    Vec::new(),
                    format!("display \"{name}\" has a zero scale, stored as 0.0001"),
                    "Use 0.0001 to make the result explicit",
                )
            });
        }
    }
}
