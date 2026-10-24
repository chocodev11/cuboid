//! The rule registry: every code with the targets it applies to, its worst severity, and the
//! defect injector that the self test uses to prove the rule still fires.

use std::sync::Arc;

use cuboid_assets::mc_versions::{McVersion, RotationRule, TextureDirs, mc_versions};
use cuboid_model::image::{Image, RgbaBuf, StoredPixels};
use cuboid_model::{
    DisplaySlotId, Document, Element, ElementType, FaceFlags, Label, ModelKind, ObjId,
    OverrideCase, Texture, TextureEntry, TextureSource,
};
use glam::{Vec2, Vec3};

use crate::codes::*;
use crate::issue::Severity;
use crate::profile::{Flavor, PackInfo, PackMeta, PackModel, Params, Profile, Target};

pub(crate) struct Rule {
    pub code: &'static str,
    pub summary: &'static str,
    pub max: Severity,
    pub applies: fn(&Params) -> bool,
    /// Puts a defect of this rule into a copy of the document and the profile.
    pub inject: fn(&mut Document, &mut Profile),
}

fn any(_: &Params) -> bool {
    true
}

fn java(p: &Params) -> bool {
    p.flavor == Flavor::Java
}

fn cubik(p: &Params) -> bool {
    p.flavor == Flavor::Cubik
}

const SELFTEST_TEXTURE: &str = "selftest_ok";

/// A texture of one colour. With `cutout`, every other texel is clear and the first is half clear.
pub(crate) fn image(w: u32, h: u32, cutout: bool) -> Arc<Image> {
    let mut buf = RgbaBuf::new(w, h);
    for (i, px) in buf.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let alpha = match (cutout, i) {
            (false, _) => 255,
            (true, 0) => 128,
            (true, i) if i % 2 == 0 => 0,
            (true, _) => 255,
        };
        *px = [200, 100, 50, alpha];
    }
    Arc::new(Image::from_rgba(buf))
}

pub(crate) fn texture(label: &str, mc_path: &str, image: Arc<Image>) -> TextureEntry {
    TextureEntry {
        label: Label::from(label),
        texture: Texture {
            source: TextureSource {
                mc_path: mc_path.to_owned(),
                ..TextureSource::default()
            },
            ..Texture::new(image)
        },
    }
}

/// The label of an opaque 16 x 16 texture with a valid path, added when missing.
pub(crate) fn base_texture(doc: &mut Document) -> Label {
    let label = Label::from(SELFTEST_TEXTURE);
    if !doc
        .textures
        .entries
        .iter()
        .any(|entry| entry.label == label)
    {
        doc.textures
            .entries
            .push(texture(SELFTEST_TEXTURE, "selftest", image(16, 16, false)));
    }
    label
}

pub(crate) fn add_cube(doc: &mut Document, label: &Label) -> ObjId {
    let element = Element::new(
        ElementType::Cube,
        &doc.blueprint,
        doc.elements.next_display_id(),
        [1.0; 4],
        label,
    );
    doc.elements.push(element)
}

/// A clean cube, then `edit` on it.
pub(crate) fn cube_with(doc: &mut Document, edit: impl FnOnce(&mut Element)) -> ObjId {
    let label = base_texture(doc);
    let id = add_cube(doc, &label);
    if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
        edit(el);
    }
    id
}

fn inject_nonfinite(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| el.base.p.x = f32::NAN);
}

fn inject_bounds(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        el.base.p.x = 100.0;
        el.base.o.x = 100.0;
    });
}

fn inject_rotation(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| el.base.e = Vec3::new(0.0, 90.0, 0.0));
}

fn inject_inverted(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| el.base.s.x = -2.0);
}

fn inject_zfight(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |_| {});
    cube_with(doc, |_| {});
}

fn inject_uv_range(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        if let Some(face) = el.faces.first_mut() {
            face.uv.set_unclamped(Vec2::splat(3.0), Vec2::splat(4.0));
        }
    });
}

fn inject_density(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |_| {});
    cube_with(doc, |_| {});
    cube_with(doc, |el| {
        for face in &mut el.faces {
            face.uv.set(Vec2::ZERO, Vec2::splat(0.01));
        }
    });
}

fn inject_cull(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        if let Some(face) = el.faces.first_mut() {
            face.cull = FaceFlags::ALL;
        }
    });
}

fn inject_face_none(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        if let Some(face) = el.faces.first_mut() {
            face.texture = Label::from("");
        }
    });
}

fn inject_unresolved(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        if let Some(face) = el.faces.first_mut() {
            face.texture = Label::from("selftest_missing_label");
        }
    });
}

fn inject_unused(doc: &mut Document, _: &mut Profile) {
    doc.textures.entries.push(texture(
        "selftest_unused",
        "block/unused",
        image(16, 16, false),
    ));
}

fn inject_no_path(doc: &mut Document, _: &mut Profile) {
    doc.textures
        .entries
        .push(texture("selftest_no_path", "", image(16, 16, false)));
}

fn inject_not_square(doc: &mut Document, _: &mut Profile) {
    doc.textures
        .entries
        .push(texture("selftest_wide", "block/wide", image(16, 8, false)));
}

fn inject_cutout(doc: &mut Document, _: &mut Profile) {
    doc.textures.entries.push(texture(
        "selftest_cutout",
        "block/cutout",
        image(16, 16, true),
    ));
    cube_with(doc, |el| {
        for face in &mut el.faces {
            face.texture = Label::from("selftest_cutout");
        }
    });
}

fn inject_colors(doc: &mut Document, profile: &mut Profile) {
    cube_with(doc, |_| {});
    profile.max_colors = Some(0);
}

fn inject_layout(doc: &mut Document, profile: &mut Profile) {
    let wrong = match profile.params().dirs {
        Some(TextureDirs::Singular) => "blocks/selftest",
        _ => "block/selftest",
    };
    doc.textures
        .entries
        .push(texture("selftest_layout", wrong, image(16, 16, false)));
}

fn inject_id_chars(doc: &mut Document, _: &mut Profile) {
    doc.textures.entries.push(texture(
        "selftest_chars",
        "Block/Bad Name",
        image(16, 16, false),
    ));
}

fn inject_budget(doc: &mut Document, profile: &mut Profile) {
    cube_with(doc, |_| {});
    profile.max_elements = Some(0);
}

fn inject_display(doc: &mut Document, _: &mut Profile) {
    Arc::make_mut(&mut doc.props).minecraft.include_display = true;
    let slot = &mut Arc::make_mut(&mut doc.display)[DisplaySlotId::Gui];
    slot.enabled = true;
    slot.translation = Vec3::new(200.0, 0.0, 0.0);
}

fn inject_kind(doc: &mut Document, _: &mut Profile) {
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Entity;
}

fn inject_empty(doc: &mut Document, _: &mut Profile) {
    Arc::make_mut(&mut doc.props).general.exclude_elements = true;
}

fn inject_overrides(doc: &mut Document, _: &mut Profile) {
    doc.overrides = Arc::from(vec![OverrideCase::default()]);
}

/// A version below every tier, with no pack format either.
const OLD_VERSION: McVersion = McVersion::new(1, 0, 0);

fn with_version(target: Target, version: McVersion) -> Target {
    match target {
        Target::JavaBlock { .. } => Target::JavaBlock { version },
        Target::JavaItem { .. } => Target::JavaItem { version },
        Target::ResourcePack { .. } => Target::ResourcePack { version },
        other => other,
    }
}

fn inject_version(_: &mut Document, profile: &mut Profile) {
    profile.target = with_version(profile.target, OLD_VERSION);
}

fn inject_uv_folded(doc: &mut Document, _: &mut Profile) {
    cube_with(doc, |el| {
        if let Some(face) = el.faces.first_mut() {
            face.uv
                .set_unclamped(Vec2::new(-0.25, 0.0), Vec2::new(0.5, 1.0));
        }
    });
}

fn inject_scale_zero(doc: &mut Document, _: &mut Profile) {
    Arc::make_mut(&mut doc.display)[DisplaySlotId::Gui].scale.x = 0.0;
}

fn inject_huge_texture(doc: &mut Document, _: &mut Profile) {
    let huge = Image::from_stored(100_000, 100_000, StoredPixels::rgba_in_memory());
    doc.textures
        .entries
        .push(texture("selftest_huge", "block/huge", Arc::new(huge)));
}

fn pack(profile: &mut Profile, info: PackInfo) {
    profile.pack = Some(info);
}

fn some_meta() -> PackMeta {
    PackMeta {
        pack_format: Some(cuboid_assets::mc_versions::PackFormat { major: 1, minor: 0 }),
        ..PackMeta::default()
    }
}

fn inject_mcmeta(_: &mut Document, profile: &mut Profile) {
    pack(profile, PackInfo::default());
}

fn has_pack_format(p: &Params) -> bool {
    p.pack
        && p.version
            .is_some_and(|v| mc_versions().pack_format_for(v).is_some())
}

fn inject_pack_format(_: &mut Document, profile: &mut Profile) {
    let known = profile
        .target
        .version()
        .and_then(|v| mc_versions().pack_format_for(v))
        .map(|entry| entry.format);
    let wrong = known.map_or(1, |f| f.major + 1);
    let off = cuboid_assets::mc_versions::PackFormat {
        major: wrong,
        minor: 0,
    };
    pack(
        profile,
        PackInfo {
            meta: Some(PackMeta {
                pack_format: Some(off),
                min_format: Some(off),
                max_format: Some(off),
                supported_formats: false,
            }),
            ..PackInfo::default()
        },
    );
}

fn inject_format_keys(_: &mut Document, profile: &mut Profile) {
    let big = cuboid_assets::mc_versions::PackFormat {
        major: mc_versions().min_max_format_above + 1,
        minor: 0,
    };
    pack(
        profile,
        PackInfo {
            meta: Some(PackMeta {
                pack_format: Some(big),
                ..PackMeta::default()
            }),
            ..PackInfo::default()
        },
    );
}

fn inject_format_unknown(_: &mut Document, profile: &mut Profile) {
    profile.target = with_version(profile.target, OLD_VERSION);
    pack(
        profile,
        PackInfo {
            meta: Some(some_meta()),
            ..PackInfo::default()
        },
    );
}

fn selftest_model() -> PackModel {
    PackModel {
        namespace: "selftest".to_owned(),
        id: "model".to_owned(),
    }
}

fn inject_itemdef(doc: &mut Document, profile: &mut Profile) {
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Item;
    pack(
        profile,
        PackInfo {
            meta: Some(some_meta()),
            files: vec!["assets/selftest/models/item/model.json".to_owned()],
            model: Some(selftest_model()),
        },
    );
}

fn inject_file_missing(_: &mut Document, profile: &mut Profile) {
    pack(
        profile,
        PackInfo {
            meta: Some(some_meta()),
            files: Vec::new(),
            model: Some(selftest_model()),
        },
    );
}

macro_rules! rule {
    ($code:expr, $max:expr, $applies:expr, $inject:expr, $summary:expr) => {
        Rule {
            code: $code,
            summary: $summary,
            max: $max,
            applies: $applies,
            inject: $inject,
        }
    };
}

fn rotates(p: &Params) -> bool {
    java(p) && p.rotation != RotationRule::MultiAxisFree
}

fn has_layout(p: &Params) -> bool {
    java(p) && p.dirs.is_some()
}

fn has_versioned_java(p: &Params) -> bool {
    java(p) && p.version.is_some()
}

fn needs_item_definition(p: &Params) -> bool {
    java(p) && p.item_defs
}

fn pack_target(p: &Params) -> bool {
    p.pack
}

fn pack_item_definitions(p: &Params) -> bool {
    p.pack && p.item_defs
}

use Severity::{Error, Info, Warning};

pub(crate) static RULES: &[Rule] = &[
    rule!(
        NUM_NONFINITE,
        Error,
        any,
        inject_nonfinite,
        "a position, size, rotation or pivot is not finite"
    ),
    rule!(
        GEO_BOUNDS,
        Error,
        java,
        inject_bounds,
        "element box outside -16..32 (inflate aware)"
    ),
    rule!(
        GEO_ROTATION,
        Error,
        rotates,
        inject_rotation,
        "rotation the version does not accept"
    ),
    rule!(
        GEO_INVERTED,
        Info,
        java,
        inject_inverted,
        "negative size, valid in Java"
    ),
    rule!(
        GEO_ZFIGHT,
        Warning,
        java,
        inject_zfight,
        "coplanar overlapping faces of different elements"
    ),
    rule!(
        UV_RANGE,
        Error,
        java,
        inject_uv_range,
        "face UV outside the texture (0..16)"
    ),
    rule!(
        UV_DENSITY,
        Warning,
        java,
        inject_density,
        "texel density far from the model median"
    ),
    rule!(
        FACE_CULL,
        Warning,
        java,
        inject_cull,
        "cull direction that is not a single side"
    ),
    rule!(
        TEX_FACE_NONE,
        Error,
        java,
        inject_face_none,
        "visible face without a texture"
    ),
    rule!(
        TEX_UNRESOLVED,
        Error,
        java,
        inject_unresolved,
        "texture reference that names no entry"
    ),
    rule!(
        TEX_UNUSED,
        Warning,
        java,
        inject_unused,
        "texture entry no visible face uses"
    ),
    rule!(
        TEX_NO_PATH,
        Warning,
        java,
        inject_no_path,
        "texture entry without a Minecraft path"
    ),
    rule!(
        TEX_NOT_SQUARE,
        Warning,
        java,
        inject_not_square,
        "texture that is not square"
    ),
    rule!(
        TEX_ALPHA_CUTOUT,
        Warning,
        java,
        inject_cutout,
        "semi-transparent pixels on a cutout face"
    ),
    rule!(
        TEX_COLORS,
        Warning,
        java,
        inject_colors,
        "more distinct colours than the budget"
    ),
    rule!(
        TEX_LAYOUT,
        Error,
        has_layout,
        inject_layout,
        "texture folder names of another game version"
    ),
    rule!(
        ID_CHARS,
        Error,
        java,
        inject_id_chars,
        "namespace or path characters a resource location may not have"
    ),
    rule!(
        BUDGET_ELEMENTS,
        Warning,
        any,
        inject_budget,
        "more elements than the budget"
    ),
    rule!(
        DISPLAY_RANGE,
        Warning,
        java,
        inject_display,
        "display translation or scale the game clamps"
    ),
    rule!(
        MODEL_KIND,
        Error,
        java,
        inject_kind,
        "model kind that is not the target's"
    ),
    rule!(
        MODEL_EMPTY,
        Error,
        java,
        inject_empty,
        "no element to write"
    ),
    rule!(
        MODEL_OVERRIDES,
        Warning,
        needs_item_definition,
        inject_overrides,
        "item overrides from 1.21.4 need an item definition"
    ),
    rule!(
        VERSION_UNKNOWN,
        Warning,
        has_versioned_java,
        inject_version,
        "game version missing from the table"
    ),
    rule!(
        CUBIK_OVERRIDES,
        Warning,
        cubik,
        inject_overrides,
        ".cubik does not store item overrides"
    ),
    rule!(
        CUBIK_UV_FOLDED,
        Info,
        cubik,
        inject_uv_folded,
        ".cubik stores UV folded into 0..1"
    ),
    rule!(
        CUBIK_SCALE_ZERO,
        Info,
        cubik,
        inject_scale_zero,
        ".cubik writes a zero display scale as 0.0001"
    ),
    rule!(
        CUBIK_TEXTURE_SIZE,
        Error,
        cubik,
        inject_huge_texture,
        "texture larger than .cubik can store"
    ),
    rule!(
        PACK_MCMETA,
        Error,
        pack_target,
        inject_mcmeta,
        "pack.mcmeta missing or without a format"
    ),
    rule!(
        PACK_FORMAT,
        Error,
        has_pack_format,
        inject_pack_format,
        "declared pack format does not cover the version"
    ),
    rule!(
        PACK_FORMAT_KEYS,
        Error,
        pack_target,
        inject_format_keys,
        "min_format and max_format above format 64, no supported_formats"
    ),
    rule!(
        PACK_FORMAT_UNKNOWN,
        Info,
        pack_target,
        inject_format_unknown,
        "version without a pack format in the table"
    ),
    rule!(
        PACK_ITEMDEF,
        Error,
        pack_item_definitions,
        inject_itemdef,
        "item model without an item definition from 1.21.4"
    ),
    rule!(
        PACK_FILE_MISSING,
        Error,
        pack_target,
        inject_file_missing,
        "model or texture file missing from the pack"
    ),
];
