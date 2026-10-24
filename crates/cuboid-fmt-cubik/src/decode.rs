//! BSON payload to records (native 3, 4, 8.1).
//!
//! The reader is lenient like Cubik's (native 3.6): numbers convert between int and double,
//! unknown keys are ignored, a repeated key keeps its last value and missing keys keep the
//! value of a fresh record. An explicit null for a value-type key is treated as absent with a
//! warning. Limits are checked before anything is allocated for the data they bound.
//!
//! spec: native 3.6, 8.1, 4.7 (invalid voxel records are skipped with a warning)

use std::fmt;
use std::sync::Arc;

use cuboid_bson::{BsonError, ElemType, RawDoc, RawElem, Span};
use cuboid_model::apply::voxel_world_bytes;
use cuboid_model::{DisplaySlots, ModelKind, ModelProps, StaxelKind};
use cuboid_num::euler_to_quat;
use glam::{Quat, Vec2, Vec3};
use rayon::prelude::*;

use crate::Limits;
use crate::records::{
    BoundsRecord, CameraRecord, ChunkRecord, ElementRecord, FACE_KEYS, FaceRecord, FaceSetRecord,
    FaceUvRecord, MeshDataRecord, MeshRecord, QuatRecord, RawTexture, Root, SubmeshRecord,
    TextureRecord, ViewRecord, VoxelData, VoxelRecord, WorldRecord,
};

/// A malformed payload: where, and what is wrong there.
#[derive(Debug, Clone, PartialEq)]
pub struct PayloadError {
    /// The location, such as `el[3].f.d.uv`; empty at the root.
    pub path: String,
    /// What is wrong.
    pub kind: PayloadErrorKind,
}

/// What is wrong with a payload.
#[derive(Debug, Clone, PartialEq)]
pub enum PayloadErrorKind {
    /// The BSON structure or a value conversion failed.
    Bson(BsonError),
    /// A value or a record that Cubik's schema does not allow.
    Invalid(String),
}

impl PayloadError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self {
            path: String::new(),
            kind: PayloadErrorKind::Invalid(message.into()),
        }
    }

    /// The error one level up, below the key or index `segment`.
    pub(crate) fn under(mut self, segment: &str) -> Self {
        self.path = if self.path.is_empty() {
            segment.to_owned()
        } else if self.path.starts_with('[') {
            format!("{segment}{}", self.path)
        } else {
            format!("{segment}.{}", self.path)
        };
        self
    }
}

impl From<BsonError> for PayloadError {
    fn from(err: BsonError) -> Self {
        Self {
            path: String::new(),
            kind: PayloadErrorKind::Bson(err),
        }
    }
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.path.is_empty() {
            write!(f, "`{}`: ", self.path)?;
        }
        match &self.kind {
            PayloadErrorKind::Bson(err) => write!(f, "{err}"),
            PayloadErrorKind::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PayloadError {}

/// Where the sections and the records of a decoded payload sit in the buffer, for retention.
#[derive(Debug, Clone, Default)]
pub struct Spans {
    /// The whole element (type, key, value) of each root section.
    pub mp: Option<Span>,
    pub vp: Option<Span>,
    pub bp: Option<Span>,
    pub el: Option<Span>,
    pub tx: Option<Span>,
    pub vx: Option<Span>,
    pub me: Option<Span>,
    pub md: Option<Span>,
    /// The elements inside each record document (without its size field and terminator), one
    /// per record of the list, in list order.
    pub el_items: Vec<Span>,
    pub tx_items: Vec<Span>,
    pub me_items: Vec<Span>,
}

/// A decoded payload.
#[derive(Debug)]
pub struct Decoded {
    pub root: Root,
    pub spans: Spans,
    /// Records that were skipped or read in a tolerant way.
    pub warnings: Vec<String>,
}

/// The most warnings kept; the rest is counted.
pub(crate) const MAX_WARNINGS: usize = 64;

/// What a record or an array item is counted against, for the whole file.
#[derive(Clone, Copy)]
enum Budget {
    Objects,
    Textures,
    MeshItems,
}

struct Cx<'l> {
    limits: &'l Limits,
    warnings: Vec<String>,
    dropped: usize,
    /// What is left of the limits that count over the whole file (the lists of a payload may
    /// each be within a limit and still be too much together).
    objects: u32,
    textures: u32,
    mesh_items: u32,
}

impl Cx<'_> {
    /// Counts one more item against `budget` before anything is allocated for it.
    fn spend(&mut self, budget: Budget) -> Result<(), PayloadError> {
        let (left, max, what) = match budget {
            Budget::Objects => (
                &mut self.objects,
                self.limits.max_objects,
                "elements, meshes and sub-meshes",
            ),
            Budget::Textures => (&mut self.textures, self.limits.max_textures, "textures"),
            Budget::MeshItems => (
                &mut self.mesh_items,
                self.limits.max_mesh_items,
                "items in the mesh data arrays",
            ),
        };
        *left = left
            .checked_sub(1)
            .ok_or_else(|| PayloadError::invalid(format!("more than {max} {what} in the file")))?;
        Ok(())
    }

    fn warn(&mut self, message: impl FnOnce() -> String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(message());
        } else {
            self.dropped += 1;
        }
    }

    fn null(&mut self, elem: &RawElem<'_>) {
        self.warn(|| {
            format!(
                "null for the value-type key `{}`; treated as absent",
                String::from_utf8_lossy(elem.key)
            )
        });
    }
}

/// Reads a payload into records.
///
/// spec: native 8.1
pub fn decode(payload: &[u8], limits: &Limits) -> Result<Decoded, PayloadError> {
    let doc = RawDoc::parse(payload, &limits.bson())?;
    let mut cx = Cx {
        limits,
        warnings: Vec::new(),
        dropped: 0,
        objects: limits.max_objects,
        textures: limits.max_textures,
        mesh_items: limits.max_mesh_items,
    };
    let mut root = Root::default();
    let mut spans = Spans::default();
    each(&doc, |elem| {
        let whole = Some(elem.whole);
        match elem.key {
            b"v" => match elem.u8() {
                Ok(v) => {
                    root.v = i32::from(v);
                    if v != 1 {
                        cx.warn(|| {
                            format!("format version {v}; Cubik writes 1 and never checks it")
                        });
                    }
                }
                Err(_) => cx.warn(|| "`v` is outside 0..255; read as 1".to_owned()),
            },
            b"mp" => {
                spans.mp = None;
                root.mp = elem.doc()?.map(|d| props(&d, &mut cx)).transpose()?;
                spans.mp = root.mp.is_some().then_some(whole).flatten();
            }
            b"vp" => {
                spans.vp = None;
                root.vp = elem.doc()?.map(|d| view(&d, &mut cx)).transpose()?;
                spans.vp = root.vp.is_some().then_some(whole).flatten();
            }
            b"bp" => {
                spans.bp = None;
                root.bp = elem.doc()?.map(|d| element(&d, &mut cx)).transpose()?;
                spans.bp = root.bp.is_some().then_some(whole).flatten();
            }
            b"el" => {
                spans.el = None;
                spans.el_items.clear();
                root.el = list(
                    &elem,
                    &mut cx,
                    Budget::Objects,
                    &mut spans.el_items,
                    element,
                )?;
                spans.el = root.el.is_some().then_some(whole).flatten();
            }
            b"tx" => {
                spans.tx = None;
                spans.tx_items.clear();
                root.tx = list(
                    &elem,
                    &mut cx,
                    Budget::Textures,
                    &mut spans.tx_items,
                    texture,
                )?;
                spans.tx = root.tx.is_some().then_some(whole).flatten();
            }
            b"vx" => {
                spans.vx = None;
                root.vx = voxels(&elem, &mut cx)?;
                spans.vx = root.vx.is_some().then_some(whole).flatten();
            }
            b"me" => {
                spans.me = None;
                spans.me_items.clear();
                root.me = list(&elem, &mut cx, Budget::Objects, &mut spans.me_items, mesh)?;
                spans.me = root.me.is_some().then_some(whole).flatten();
            }
            b"md" => {
                spans.md = None;
                root.md = elem.doc()?.map(|d| display(&d, &mut cx)).transpose()?;
                spans.md = root.md.is_some().then_some(whole).flatten();
            }
            _ => {}
        }
        Ok(())
    })?;
    if cx.dropped > 0 {
        let dropped = cx.dropped;
        cx.warnings
            .push(format!("{dropped} more warnings left out"));
    }
    Ok(Decoded {
        root,
        spans,
        warnings: cx.warnings,
    })
}

/// Calls `f` for every element of a document and puts the key into the path of an error.
fn each<'a>(
    doc: &RawDoc<'a>,
    mut f: impl FnMut(RawElem<'a>) -> Result<(), PayloadError>,
) -> Result<(), PayloadError> {
    for elem in doc.iter() {
        let elem = elem?;
        let key = elem.key;
        f(elem).map_err(|err| err.under(&String::from_utf8_lossy(key)))?;
    }
    Ok(())
}

/// Calls `f` for every element of an array and puts the index into the path of an error.
fn each_item<'a>(
    arr: &RawDoc<'a>,
    mut f: impl FnMut(RawElem<'a>) -> Result<(), PayloadError>,
) -> Result<(), PayloadError> {
    for (index, elem) in arr.iter().enumerate() {
        let elem = elem?;
        f(elem).map_err(|err| err.under(&format!("[{index}]")))?;
    }
    Ok(())
}

/// A list of record documents. `None` for a null. Null items are skipped with a warning.
fn list<'a, T>(
    elem: &RawElem<'a>,
    cx: &mut Cx<'_>,
    budget: Budget,
    spans: &mut Vec<Span>,
    mut read: impl FnMut(&RawDoc<'a>, &mut Cx<'_>) -> Result<T, PayloadError>,
) -> Result<Option<Vec<T>>, PayloadError> {
    let Some(arr) = elem.array()? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    each_item(&arr, |item| {
        let Some(doc) = item.doc()? else {
            cx.warn(|| "a null item of a list is skipped".to_owned());
            return Ok(());
        };
        cx.spend(budget)?;
        out.push(read(&doc, cx)?);
        spans.push(inner(&doc));
        Ok(())
    })?;
    Ok(Some(out))
}

/// The elements of a document without its size field and its terminator.
pub(crate) fn inner(doc: &RawDoc<'_>) -> Span {
    let span = doc.span();
    Span {
        start: span.start + 4,
        end: span.end.saturating_sub(1).max(span.start + 4),
    }
}

// --- Scalars ------------------------------------------------------------------------------------

/// Reads `$elem` with the getter `$get` into `$dst`; a null leaves `$dst` as it is.
macro_rules! set {
    ($cx:expr, $elem:expr, $dst:expr, $get:ident) => {
        if $elem.ty == ElemType::Null {
            $cx.null(&$elem);
        } else {
            $dst = $elem.$get()?;
        }
    };
}

fn string(elem: &RawElem<'_>) -> Result<String, PayloadError> {
    Ok(elem.str_opt()?.unwrap_or_default().to_owned())
}

fn opt_string(elem: &RawElem<'_>) -> Result<Option<String>, PayloadError> {
    Ok(elem.str_opt()?.map(str::to_owned))
}

fn components<const N: usize>(
    elem: &RawElem<'_>,
    keys: [&[u8]; N],
    cx: &mut Cx<'_>,
    mut out: [f32; N],
) -> Result<[f32; N], PayloadError> {
    let Some(doc) = elem.doc()? else {
        cx.null(elem);
        return Ok(out);
    };
    each(&doc, |m| {
        if let Some(slot) = keys.iter().position(|&k| k == m.key)
            && let Some(dst) = out.get_mut(slot)
        {
            set!(cx, m, *dst, f32);
        }
        Ok(())
    })?;
    Ok(out)
}

fn vec3(elem: &RawElem<'_>, cx: &mut Cx<'_>, base: Vec3) -> Result<Vec3, PayloadError> {
    components(elem, [b"x", b"y", b"z"], cx, base.to_array()).map(Vec3::from_array)
}

fn vec2(elem: &RawElem<'_>, cx: &mut Cx<'_>, base: Vec2) -> Result<Vec2, PayloadError> {
    components(elem, [b"x", b"y"], cx, base.to_array()).map(Vec2::from_array)
}

/// Members in document order, as Cubik's reader applies them: `eulerAngles` replaces the whole
/// rotation built so far.
///
/// spec: native 3.4
fn quaternion(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<QuatRecord, PayloadError> {
    let mut rec = QuatRecord::IDENTITY;
    let Some(doc) = elem.doc()? else {
        cx.null(elem);
        return Ok(rec);
    };
    let mut rotation = Quat::IDENTITY;
    each(&doc, |m| {
        match m.key {
            b"x" => {
                set!(cx, m, rec.x, f32);
                rotation.x = rec.x;
            }
            b"y" => {
                set!(cx, m, rec.y, f32);
                rotation.y = rec.y;
            }
            b"z" => {
                set!(cx, m, rec.z, f32);
                rotation.z = rec.z;
            }
            b"w" => {
                set!(cx, m, rec.w, f32);
                rotation.w = rec.w;
            }
            b"eulerAngles" => {
                let euler = vec3(&m, cx, Vec3::ZERO)?;
                rec.euler = Some(euler);
                rotation = euler_to_quat(euler);
            }
            _ => {}
        }
        Ok(())
    })?;
    rec.rotation = rotation;
    Ok(rec)
}

// --- Model properties and view ------------------------------------------------------------------

fn props(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<ModelProps, PayloadError> {
    let mut p = ModelProps::default();
    each(doc, |m| {
        let group = m.ty != ElemType::Null;
        match m.key {
            b"sp" => p.save_path = string(&m)?,
            b"ep" => p.export_path = string(&m)?,
            b"g" if group => general(&m, cx, &mut p)?,
            b"mc" if group => minecraft(&m, cx, &mut p)?,
            b"sh" if group => stonehearth(&m, cx, &mut p)?,
            b"st" if group => staxel(&m, cx, &mut p)?,
            b"qb" if group => qubicle(&m, cx, &mut p)?,
            b"obj" if group => mesh_export(&m, cx, &mut p)?,
            b"png" if group => png(&m, cx, &mut p)?,
            b"g" | b"mc" | b"sh" | b"st" | b"qb" | b"obj" | b"png" => cx.null(&m),
            _ => {}
        }
        Ok(())
    })?;
    Ok(p)
}

fn group_doc<'a>(elem: &RawElem<'a>) -> Result<RawDoc<'a>, PayloadError> {
    elem.doc()?
        .ok_or_else(|| PayloadError::invalid("null group"))
}

fn general(elem: &RawElem<'_>, cx: &mut Cx<'_>, p: &mut ModelProps) -> Result<(), PayloadError> {
    let g = &mut p.general;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"o" => g.origin = vec3(&m, cx, g.origin)?,
            b"mb" => g.model_box = model_box(&m, cx)?,
            b"ee" => set!(cx, m, g.exclude_elements, bool),
            b"ev" => set!(cx, m, g.exclude_voxels, bool),
            b"em" => set!(cx, m, g.exclude_meshes, bool),
            b"me" => set!(cx, m, g.minify, bool),
            b"rer" => set!(cx, m, g.restrict_rotations, bool),
            b"ehe" => set!(cx, m, g.exclude_hidden, bool),
            b"pbt" => set!(cx, m, g.prevent_bleeding, bool),
            b"pzf" => set!(cx, m, g.prevent_zfighting, bool),
            b"d" => g.description = m.str_opt()?.map_or_else(String::new, str::to_owned),
            b"n" => {
                if let Some(name) = m.str_opt()? {
                    name.clone_into(&mut g.name);
                }
            }
            _ => {}
        }
        Ok(())
    })
}

fn model_box(
    elem: &RawElem<'_>,
    cx: &mut Cx<'_>,
) -> Result<Option<cuboid_model::ModelBox>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let (mut min, mut max) = (Vec3::ZERO, Vec3::ZERO);
    each(&doc, |m| {
        match m.key {
            b"min" => min = vec3(&m, cx, min)?,
            b"max" => max = vec3(&m, cx, max)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(Some(cuboid_model::ModelBox { min, max }))
}

fn minecraft(elem: &RawElem<'_>, cx: &mut Cx<'_>, p: &mut ModelProps) -> Result<(), PayloadError> {
    const KINDS: [(&str, i32); 6] = [
        ("none", 0),
        ("block", 1),
        ("item", 2),
        ("entity", 3),
        ("optifine", 4),
        ("map", 5),
    ];
    let mc = &mut p.minecraft;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"t" => {
                let kind = m.enum_i32(&KINDS)?;
                match model_kind(kind) {
                    Some(kind) => mc.kind = kind,
                    None => cx.warn(|| format!("model kind {kind} is unknown; read as block")),
                }
            }
            b"ao" => set!(cx, m, mc.ambient_occlusion, bool),
            b"uen" => set!(cx, m, mc.element_names, bool),
            b"uvds" => set!(cx, m, mc.auto_vanilla_parent, bool),
            b"uds" => set!(cx, m, mc.include_display, bool),
            b"uos" => set!(cx, m, mc.keep_overrides, bool),
            b"eab" => mc.entity_attach_bone = string(&m)?,
            b"etr" => set!(cx, m, mc.entity_resolution, i32),
            b"etx" => set!(cx, m, mc.entity_offset_x, i32),
            b"ety" => set!(cx, m, mc.entity_offset_y, i32),
            b"etw" => set!(cx, m, mc.entity_min_width, i32),
            b"eth" => set!(cx, m, mc.entity_min_height, i32),
            b"esb" => set!(cx, m, mc.entity_same_bone, bool),
            b"est" => set!(cx, m, mc.entity_reuse_texture, bool),
            b"ecn" => set!(cx, m, mc.entity_cube_names, bool),
            b"mt" => {
                if let Some(table) = m.str_opt()? {
                    table.clone_into(&mut mc.block_table);
                }
            }
            b"p" => mc.parent = string(&m)?,
            _ => {}
        }
        Ok(())
    })
}

fn model_kind(value: i32) -> Option<ModelKind> {
    Some(match value {
        0 => ModelKind::None,
        1 => ModelKind::Block,
        2 => ModelKind::Item,
        3 => ModelKind::Entity,
        4 => ModelKind::Optifine,
        5 => ModelKind::Map,
        _ => return None,
    })
}

fn stonehearth(
    elem: &RawElem<'_>,
    cx: &mut Cx<'_>,
    p: &mut ModelProps,
) -> Result<(), PayloadError> {
    let sh = &mut p.stonehearth;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"pog" => set!(cx, m, sh.on_ground, bool),
            b"pow" => set!(cx, m, sh.on_walls, bool),
            b"v" => set!(cx, m, sh.gold, i32),
            b"r" => {
                if let Some(rarity) = m.str_opt()? {
                    rarity.clone_into(&mut sh.rarity);
                }
            }
            b"b" => set!(cx, m, sh.buyable, bool),
            b"s" => set!(cx, m, sh.sellable, bool),
            _ => {}
        }
        Ok(())
    })
}

fn staxel(elem: &RawElem<'_>, cx: &mut Cx<'_>, p: &mut ModelProps) -> Result<(), PayloadError> {
    const KINDS: [(&str, i32); 5] = [
        ("none", 0),
        ("tile", 1),
        ("item", 2),
        ("accessory", 3),
        ("other", 4),
    ];
    each(&group_doc(elem)?, |m| {
        if m.key == b"t" {
            let kind = m.enum_i32(&KINDS)?;
            match kind {
                0 => p.staxel.kind = StaxelKind::None,
                1 => p.staxel.kind = StaxelKind::Tile,
                2 => p.staxel.kind = StaxelKind::Item,
                3 => p.staxel.kind = StaxelKind::Accessory,
                4 => p.staxel.kind = StaxelKind::Other,
                _ => cx.warn(|| format!("Staxel kind {kind} is unknown; read as accessory")),
            }
        }
        Ok(())
    })
}

fn qubicle(elem: &RawElem<'_>, cx: &mut Cx<'_>, p: &mut ModelProps) -> Result<(), PayloadError> {
    let qb = &mut p.qubicle;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"rgb" => set!(cx, m, qb.rgb, bool),
            b"aiz" => set!(cx, m, qb.inverted_z, bool),
            _ => {}
        }
        Ok(())
    })
}

fn mesh_export(
    elem: &RawElem<'_>,
    cx: &mut Cx<'_>,
    p: &mut ModelProps,
) -> Result<(), PayloadError> {
    let obj = &mut p.mesh_export;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"gmv" => set!(cx, m, obj.voxel_colored, bool),
            b"gmt" => set!(cx, m, obj.voxel_palette_texture, bool),
            b"omt" => set!(cx, m, obj.voxel_atlas_texture, bool),
            b"wv" => set!(cx, m, obj.weld_vertices, bool),
            b"mo" => set!(cx, m, obj.merge_objects, bool),
            b"pt" => set!(cx, m, obj.atlas_padding, bool),
            b"ao" => set!(cx, m, obj.ambient_occlusion, bool),
            b"sv" => set!(cx, m, obj.smooth_vertices, bool),
            _ => {}
        }
        Ok(())
    })
}

fn png(elem: &RawElem<'_>, cx: &mut Cx<'_>, p: &mut ModelProps) -> Result<(), PayloadError> {
    let png = &mut p.png;
    each(&group_doc(elem)?, |m| {
        match m.key {
            b"gp" => set!(cx, m, png.gui_preview, bool),
            b"mv" => set!(cx, m, png.main_view, bool),
            b"aa" => set!(cx, m, png.antialias, bool),
            _ => {}
        }
        Ok(())
    })
}

fn view(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<ViewRecord, PayloadError> {
    let mut v = ViewRecord::default();
    each(doc, |m| {
        match m.key {
            b"ctl" => v.cameras[0] = camera(&m, cx)?,
            b"ctr" => v.cameras[1] = camera(&m, cx)?,
            b"cbl" => v.cameras[2] = camera(&m, cx)?,
            b"cbr" => v.cameras[3] = camera(&m, cx)?,
            b"gm" => set!(cx, m, v.gm, f32),
            b"gk" => set!(cx, m, v.gk, f32),
            b"gu" => set!(cx, m, v.gu, f32),
            b"gd" => set!(cx, m, v.gd, f32),
            b"gl" => set!(cx, m, v.gl, bool),
            _ => {}
        }
        Ok(())
    })?;
    Ok(v)
}

fn camera(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<CameraRecord>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut cam = CameraRecord {
        p: Vec3::ZERO,
        r: QuatRecord::IDENTITY,
        o: false,
        os: 0.0,
        fv: 0.0,
    };
    each(&doc, |m| {
        match m.key {
            b"p" => cam.p = vec3(&m, cx, cam.p)?,
            b"r" => cam.r = quaternion(&m, cx)?,
            b"o" => set!(cx, m, cam.o, bool),
            b"os" => set!(cx, m, cam.os, f32),
            b"fv" => set!(cx, m, cam.fv, f32),
            _ => {}
        }
        Ok(())
    })?;
    Ok(Some(cam))
}

// --- Elements -----------------------------------------------------------------------------------

const ELEMENT_TYPES: [(&str, i32); 4] = [("cube", 0), ("planex", 1), ("planey", 2), ("planez", 3)];

const FACE_TYPES: [(&str, i32); 8] = [
    ("none", 0),
    ("down", 1),
    ("up", 2),
    ("north", 4),
    ("south", 8),
    ("west", 16),
    ("east", 32),
    ("any", 63),
];

fn element(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<ElementRecord, PayloadError> {
    let mut el = ElementRecord::default();
    each(doc, |m| {
        match m.key {
            b"p" => el.p = vec3(&m, cx, el.p)?,
            b"s" => el.s = vec3(&m, cx, el.s)?,
            b"r" => el.r = vec3(&m, cx, el.r)?,
            b"o" => el.o = vec3(&m, cx, el.o)?,
            b"c" => set!(cx, m, el.c, i32),
            b"is" => set!(cx, m, el.is, bool),
            b"ih" => set!(cx, m, el.ih, bool),
            b"il" => set!(cx, m, el.il, bool),
            b"l" => el.l = opt_string(&m)?,
            b"f" => el.f = faces(&m, cx)?,
            b"rs" => set!(cx, m, el.rs, bool),
            b"suv" => set!(cx, m, el.suv, bool),
            b"suvm" => set!(cx, m, el.suvm, bool),
            b"sh" => set!(cx, m, el.sh, bool),
            b"in" => set!(cx, m, el.inv, bool),
            b"t" => el.t = m.enum_i32(&ELEMENT_TYPES)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(el)
}

fn faces(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<FaceSetRecord>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut set = FaceSetRecord {
        faces: std::array::from_fn(FaceRecord::absent),
    };
    each(&doc, |m| {
        let Some(slot) = FACE_KEYS.iter().position(|&key| key == m.key) else {
            return Ok(());
        };
        let Some(dst) = set.faces.get_mut(slot) else {
            return Ok(());
        };
        match m.doc()? {
            Some(face_doc) => *dst = face(&face_doc, cx, slot)?,
            None => cx.null(&m),
        }
        Ok(())
    })?;
    Ok(Some(set))
}

fn face(doc: &RawDoc<'_>, cx: &mut Cx<'_>, slot: usize) -> Result<FaceRecord, PayloadError> {
    let mut f = FaceRecord::absent(slot);
    each(doc, |m| {
        match m.key {
            b"n" => f.n = opt_string(&m)?,
            b"a" => set!(cx, m, f.a, bool),
            b"ti" => set!(cx, m, f.ti, i32),
            b"c" => f.c = m.enum_i32(&FACE_TYPES)?,
            b"t" => f.t = opt_string(&m)?,
            b"uv" => match m.doc()? {
                Some(uv_doc) => f.uv = face_uv(&uv_doc, cx)?,
                None => cx.null(&m),
            },
            _ => {}
        }
        Ok(())
    })?;
    Ok(f)
}

fn face_uv(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<FaceUvRecord, PayloadError> {
    let mut uv = FaceUvRecord::default();
    each(doc, |m| {
        match m.key {
            b"r" => set!(cx, m, uv.r, i32),
            b"tl" => uv.tl = vec2(&m, cx, uv.tl)?,
            b"br" => uv.br = vec2(&m, cx, uv.br)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(uv)
}

// --- Textures and meshes ------------------------------------------------------------------------

fn texture(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<TextureRecord, PayloadError> {
    let mut rec = TextureRecord {
        lb: String::new(),
        fp: None,
        fname: None,
        fm: String::new(),
        tx: RawTexture {
            n: String::new(),
            w: 0,
            h: 0,
            f: 0,
            r: Arc::from([]),
        },
        cs: None,
    };
    let mut has_image = false;
    each(doc, |m| {
        match m.key {
            b"lb" => rec.lb = string(&m)?,
            b"fp" => rec.fp = opt_string(&m)?,
            b"fn" => rec.fname = opt_string(&m)?,
            b"fm" => rec.fm = string(&m)?,
            b"tx" => {
                if let Some(tx) = raw_texture(&m, cx)? {
                    rec.tx = tx;
                    has_image = true;
                }
            }
            b"cs" => rec.cs = opt_string(&m)?,
            _ => {}
        }
        Ok(())
    })?;
    if has_image {
        Ok(rec)
    } else {
        Err(PayloadError::invalid(format!(
            "texture `{}` has no image",
            rec.lb
        )))
    }
}

fn raw_texture(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<RawTexture>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut raw = RawTexture {
        n: String::new(),
        w: 0,
        h: 0,
        f: 0,
        r: Arc::from([]),
    };
    each(&doc, |m| {
        match m.key {
            b"n" => raw.n = string(&m)?,
            b"w" => set!(cx, m, raw.w, i32),
            b"h" => set!(cx, m, raw.h, i32),
            b"f" => set!(cx, m, raw.f, i32),
            b"r" => {
                raw.r = m
                    .bytes_opt()?
                    .map_or_else(|| Arc::from([]), |bytes| Arc::from(&*bytes));
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(Some(raw))
}

const MESH_KINDS: [(&str, i32); 11] = [
    ("custom", 0),
    ("cube", 1),
    ("sphere", 2),
    ("cylinder", 3),
    ("cone", 4),
    ("pyramid", 5),
    ("slope", 6),
    ("cornerin", 7),
    ("cornerout", 8),
    ("torus", 9),
    ("sprite", 10),
];

fn mesh(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<MeshRecord, PayloadError> {
    let mut me = MeshRecord::default();
    each(doc, |m| {
        match m.key {
            b"p" => me.p = vec3(&m, cx, me.p)?,
            b"s" => me.s = vec3(&m, cx, me.s)?,
            b"r" => me.r = vec3(&m, cx, me.r)?,
            b"o" => me.o = vec3(&m, cx, me.o)?,
            b"is" => set!(cx, m, me.is, bool),
            b"ih" => set!(cx, m, me.ih, bool),
            b"il" => set!(cx, m, me.il, bool),
            b"l" => me.l = opt_string(&m)?,
            b"tp" => me.tp = m.enum_i32(&MESH_KINDS)?,
            b"tx" => me.tx = raw_texture(&m, cx)?,
            b"mb" => me.mb = bounds(&m, cx)?,
            b"me" => me.me = mesh_data(&m, cx)?,
            b"sm" => me.sm = submeshes(&m, cx)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(me)
}

fn bounds(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<BoundsRecord>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut mb = BoundsRecord::default();
    each(&doc, |m| {
        match m.key {
            b"c" => mb.c = vec3(&m, cx, mb.c)?,
            b"s" => mb.s = vec3(&m, cx, mb.s)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(Some(mb))
}

fn mesh_data(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<MeshDataRecord>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut data = MeshDataRecord::default();
    each(&doc, |m| {
        match m.key {
            b"v" => data.v = vec3_array(&m, cx)?.unwrap_or_default(),
            b"n" => data.n = vec3_array(&m, cx)?,
            b"t" => data.t = i32_array(&m, cx)?.unwrap_or_default(),
            b"u" => {
                data.u = match m.array()? {
                    None => None,
                    Some(arr) => {
                        let mut out = Vec::with_capacity(arr.as_bytes().len() / 28);
                        each_item(&arr, |item| {
                            cx.spend(Budget::MeshItems)?;
                            out.push(vec2(&item, cx, Vec2::ZERO)?);
                            Ok(())
                        })?;
                        Some(out)
                    }
                };
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(Some(data))
}

fn vec3_array(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<Vec<Vec3>>, PayloadError> {
    let Some(arr) = elem.array()? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(arr.as_bytes().len() / 40);
    each_item(&arr, |item| {
        cx.spend(Budget::MeshItems)?;
        out.push(vec3(&item, cx, Vec3::ZERO)?);
        Ok(())
    })?;
    Ok(Some(out))
}

fn i32_array(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<Vec<i32>>, PayloadError> {
    let Some(arr) = elem.array()? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(arr.as_bytes().len() / 8);
    each_item(&arr, |item| {
        cx.spend(Budget::MeshItems)?;
        out.push(item.i32()?);
        Ok(())
    })?;
    Ok(Some(out))
}

fn submeshes(
    elem: &RawElem<'_>,
    cx: &mut Cx<'_>,
) -> Result<Option<Vec<SubmeshRecord>>, PayloadError> {
    let Some(arr) = elem.array()? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    each_item(&arr, |item| {
        let Some(doc) = item.doc()? else {
            cx.warn(|| "a null sub-mesh is skipped".to_owned());
            return Ok(());
        };
        cx.spend(Budget::Objects)?;
        let mut sm = SubmeshRecord::default();
        each(&doc, |m| {
            match m.key {
                b"t" => sm.t = i32_array(&m, cx)?.unwrap_or_default(),
                b"c" => set!(cx, m, sm.c, i32),
                b"tx" => sm.tx = raw_texture(&m, cx)?,
                _ => {}
            }
            Ok(())
        })?;
        out.push(sm);
        Ok(())
    })?;
    Ok(Some(out))
}

// --- Voxels -------------------------------------------------------------------------------------

/// Reads `vx`. Only the first world is kept. Chunk records with an index outside the grid and
/// voxel records with a local coordinate outside 0..15 are skipped with a warning (native 4.7).
fn voxels(elem: &RawElem<'_>, cx: &mut Cx<'_>) -> Result<Option<VoxelData>, PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok(None);
    };
    let mut world = None;
    each(&doc, |m| {
        if m.key != b"w" {
            return Ok(());
        }
        world = None;
        let Some(arr) = m.array()? else {
            return Ok(());
        };
        if let Some(first) = arr.iter().next() {
            let first = first?;
            if let Some(world_doc) = first.doc()? {
                world = Some(read_world(&world_doc, cx).map_err(|err| err.under("[0]"))?);
            }
        }
        Ok(())
    })?;
    Ok(world.map(|w| VoxelData { w: vec![w] }))
}

fn read_world(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<WorldRecord, PayloadError> {
    let mut res = 0;
    let mut chunks = None;
    each(doc, |m| {
        match m.key {
            b"r" => res = m.i32()?,
            b"c" => chunks = Some(m),
            _ => {}
        }
        Ok(())
    })?;
    let budget = cx.limits.voxel_budget;
    let fits = |chunks: usize| {
        u32::try_from(res)
            .ok()
            .and_then(|res| voxel_world_bytes(res, chunks))
            .is_some_and(|bytes| bytes <= budget)
    };
    if !fits(0) {
        return Err(PayloadError::invalid(format!(
            "voxel resolution {res} is below 1 or needs more than the memory budget of {budget} bytes"
        ))
        .under("r"));
    }
    let per_axis = (3 * res + 15) / 16;

    let mut items = Vec::new();
    if let Some(chunks) = chunks {
        let under_c = |err: BsonError| PayloadError::from(err).under("c");
        if let Some(arr) = chunks.array().map_err(under_c)? {
            for item in arr.iter() {
                let item = item.map_err(under_c)?;
                if !fits(items.len() + 1) {
                    return Err(PayloadError::invalid(format!(
                        "the voxel chunks need more than the memory budget of {budget} bytes"
                    ))
                    .under("c"));
                }
                items.push(item);
            }
        }
    }
    let decoded: Vec<Result<(Option<ChunkRecord>, usize), PayloadError>> = items
        .par_iter()
        .enumerate()
        .map(|(index, item)| {
            chunk(item, per_axis).map_err(|err| err.under(&format!("[{index}]")).under("c"))
        })
        .collect();
    let mut out = Vec::with_capacity(decoded.len());
    let mut skipped = 0;
    for result in decoded {
        let (rec, lost) = result?;
        skipped += lost;
        out.extend(rec);
    }
    if skipped > 0 {
        cx.warn(|| format!("{skipped} invalid voxel or chunk records were skipped"));
    }
    Ok(WorldRecord { r: res, c: out })
}

/// A chunk record and the number of records skipped inside it (the chunk itself counts one).
fn chunk(elem: &RawElem<'_>, per_axis: i32) -> Result<(Option<ChunkRecord>, usize), PayloadError> {
    let Some(doc) = elem.doc()? else {
        return Ok((None, 1));
    };
    let (mut chunk_x, mut chunk_y, mut chunk_z) = (0, 0, 0);
    let mut voxels = None;
    each(&doc, |m| {
        match m.key {
            b"cx" => chunk_x = m.i32()?,
            b"cy" => chunk_y = m.i32()?,
            b"cz" => chunk_z = m.i32()?,
            b"v" => voxels = Some(m),
            _ => {}
        }
        Ok(())
    })?;
    if [chunk_x, chunk_y, chunk_z]
        .iter()
        .any(|c| !(0..per_axis).contains(c))
    {
        return Ok((None, 1));
    }
    let mut v = Vec::new();
    let mut skipped = 0;
    if let Some(m) = voxels {
        let under_v = |err: BsonError| PayloadError::from(err).under("v");
        if let Some(arr) = m.array().map_err(under_v)? {
            v.reserve(arr.as_bytes().len() / 38);
            for (index, item) in arr.iter().enumerate() {
                let item = item.map_err(under_v)?;
                match voxel(&item).map_err(|err| err.under(&format!("[{index}]")).under("v"))? {
                    Some(rec) => v.push(rec),
                    None => skipped += 1,
                }
            }
        }
    }
    let rec = ChunkRecord {
        cx: chunk_x,
        cy: chunk_y,
        cz: chunk_z,
        v,
    };
    Ok((Some(rec), skipped))
}

/// One voxel record; `None` when a local coordinate is outside 0..15. The 33-byte layout that
/// Cubik writes is read at fixed offsets (native 3.5).
fn voxel(item: &RawElem<'_>) -> Result<Option<VoxelRecord>, PayloadError> {
    let (x, y, z, c) = match fast_voxel(item.value_bytes()) {
        Some(fields) => fields,
        None => {
            let Some(doc) = item.doc()? else {
                return Ok(None);
            };
            let (mut x, mut y, mut z, mut c) = (0, 0, 0, 0);
            each(&doc, |m| {
                match m.key {
                    b"x" => x = m.i32()?,
                    b"y" => y = m.i32()?,
                    b"z" => z = m.i32()?,
                    b"c" => c = m.i32()?,
                    _ => {}
                }
                Ok(())
            })?;
            (x, y, z, c)
        }
    };
    let local = |v: i32| u8::try_from(v).ok().filter(|&v| v < 16);
    Ok(match (local(x), local(y), local(z)) {
        (Some(x), Some(y), Some(z)) => Some(VoxelRecord { x, y, z, c }),
        _ => None,
    })
}

/// The exact bytes of a voxel record as Cubik writes it: four int32 members `x`, `y`, `z`, `c`.
fn fast_voxel(bytes: &[u8]) -> Option<(i32, i32, i32, i32)> {
    let &[
        33,
        0,
        0,
        0,
        0x10,
        b'x',
        0,
        x0,
        x1,
        x2,
        x3,
        0x10,
        b'y',
        0,
        y0,
        y1,
        y2,
        y3,
        0x10,
        b'z',
        0,
        z0,
        z1,
        z2,
        z3,
        0x10,
        b'c',
        0,
        c0,
        c1,
        c2,
        c3,
        0,
    ] = <&[u8; 33]>::try_from(bytes).ok()?
    else {
        return None;
    };
    Some((
        i32::from_le_bytes([x0, x1, x2, x3]),
        i32::from_le_bytes([y0, y1, y2, y3]),
        i32::from_le_bytes([z0, z1, z2, z3]),
        i32::from_le_bytes([c0, c1, c2, c3]),
    ))
}

// --- Display ------------------------------------------------------------------------------------

/// The keys of the display slots in file order.
pub(crate) const DISPLAY_KEYS: [&[u8]; 8] = [b"tr", b"tl", b"fr", b"fl", b"u", b"h", b"g", b"i"];

fn display(doc: &RawDoc<'_>, cx: &mut Cx<'_>) -> Result<DisplaySlots, PayloadError> {
    let mut md = DisplaySlots::default();
    each(doc, |m| {
        let Some(slot) = DISPLAY_KEYS.iter().position(|&key| key == m.key) else {
            return Ok(());
        };
        let Some(dst) = md.slots.get_mut(slot) else {
            return Ok(());
        };
        let Some(slot_doc) = m.doc()? else {
            cx.null(&m);
            return Ok(());
        };
        each(&slot_doc, |f| {
            match f.key {
                b"n" => {
                    if let Some(name) = f.str_opt()? {
                        name.clone_into(&mut dst.preset);
                    }
                }
                b"c" => set!(cx, f, dst.removable, bool),
                b"e" => set!(cx, f, dst.enabled, bool),
                b"r" => dst.rotation = vec3(&f, cx, dst.rotation)?,
                b"t" => dst.translation = vec3(&f, cx, dst.translation)?,
                b"s" => dst.scale = vec3(&f, cx, dst.scale)?,
                _ => {}
            }
            Ok(())
        })
    })?;
    Ok(md)
}
