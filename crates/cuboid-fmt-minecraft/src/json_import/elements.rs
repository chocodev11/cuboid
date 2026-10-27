//! The `elements` array of a model (interop 8.5, 31.2).

use std::collections::BTreeMap;

use cuboid_model::mc::{self, McBox, Rigid};
use cuboid_model::uv::default_uv_rects;
use cuboid_model::{
    Element, ElementFlags, ElementType, Face, FaceFlags, FaceName, Label, ObjectBase, Perms,
};
use glam::{Vec2, Vec3};
use serde_json::Value;

use super::textures::Table;
use crate::jv::{flag, int_at, num, num_at, text, vec3_at};
use crate::{McError, Notice};

/// The colours elements get, in turn: the imported elements have none of their own.
const COLORS: [[f32; 3]; 8] = [
    [1.0, 0.55, 0.55],
    [1.0, 0.78, 0.55],
    [1.0, 1.0, 0.55],
    [0.7, 1.0, 0.55],
    [0.55, 1.0, 0.8],
    [0.55, 0.8, 1.0],
    [0.7, 0.55, 1.0],
    [1.0, 0.55, 0.9],
];

/// The colour of the `index`th imported element.
pub(crate) fn marker_color(index: usize) -> [f32; 4] {
    let [r, g, b] = COLORS
        .get(index % COLORS.len())
        .copied()
        .unwrap_or([1.0; 3]);
    [r, g, b, 1.0]
}

/// The pivot of a rotation without an `origin` (the game's default).
const DEFAULT_ORIGIN: Vec3 = Vec3::splat(8.0);

/// The rotation of an element in the JSON: the angles about X, Y and Z, the pivot and `rescale`.
/// The classic form has an `axis` and an `angle`; from 1.21.11 an object with `x`, `y` and `z`
/// angles is allowed. An unknown axis gives no rotation, but the pivot stays.
fn rotation(item: &Value) -> Option<(Vec3, Vec3, bool)> {
    let spec = item.get("rotation").filter(|r| r.is_object())?;
    let pivot = vec3_at(spec, "origin").unwrap_or(DEFAULT_ORIGIN);
    let angle = num_at(spec, "angle").unwrap_or(0.0);
    let angles = match text(spec, "axis") {
        Some("x") => Vec3::new(angle, 0.0, 0.0),
        Some("y") => Vec3::new(0.0, angle, 0.0),
        Some("z") => Vec3::new(0.0, 0.0, angle),
        Some(_) => Vec3::ZERO,
        None => Vec3::new(
            num_at(spec, "x").unwrap_or(0.0),
            num_at(spec, "y").unwrap_or(0.0),
            num_at(spec, "z").unwrap_or(0.0),
        ),
    };
    Some((angles, pivot, flag(spec, "rescale") == Some(true)))
}

/// Builds the elements. `units` is the size of the UV space of the file (16 x 16 unless the file
/// has a `texture_size`). Keys the model cannot hold are counted in `skipped`.
pub(crate) fn build(
    items: &[Value],
    table: &mut Table,
    units: Vec2,
    default_label: &Label,
    skipped: &mut BTreeMap<String, usize>,
    notices: &mut Vec<Notice>,
) -> Result<Vec<Element>, McError> {
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if !item.is_object() {
            *skipped
                .entry("elements that are not objects".to_owned())
                .or_default() += 1;
            continue;
        }
        if item.get("light_emission").is_some() {
            *skipped
                .entry("light_emission values".to_owned())
                .or_default() += 1;
        }
        out.push(element(item, index, table, units, default_label, notices)?);
    }
    Ok(out)
}

fn element(
    item: &Value,
    index: usize,
    table: &mut Table,
    units: Vec2,
    default_label: &Label,
    notices: &mut Vec<Notice>,
) -> Result<Element, McError> {
    let label: String = ["__comment", "name"]
        .into_iter()
        .filter_map(|key| text(item, key))
        .find(|t| !t.is_empty())
        .unwrap_or("Element")
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();

    let from = vec3_at(item, "from").unwrap_or(Vec3::ZERO);
    let to = vec3_at(item, "to").unwrap_or(Vec3::ZERO);
    let (angles, pivot, rescale) = match rotation(item) {
        Some((angles, pivot, rescale)) => (angles, pivot, rescale),
        None => (Vec3::ZERO, from, false),
    };
    let placed = mc::place(
        &McBox {
            from,
            to,
            pivot,
            rotation: angles,
        },
        Rigid::IDENTITY,
    );

    let mut base = ObjectBase::new(label.clone(), placed.p, placed.s, Perms::ELEMENT);
    base.o = placed.o;
    base.e = placed.e;

    let mut flags = ElementFlags::empty();
    // TODO(R1): the per-element shade flag is not in cuboid-model yet; `shade` maps to
    // `ElementFlags::SHADE` until it lands.
    flags.set(ElementFlags::SHADE, flag(item, "shade").unwrap_or(true));
    flags.set(ElementFlags::RESCALE, rescale);

    // The automatic mapping of the element, for faces without a rectangle of their own.
    let auto = default_uv_rects(placed.p, placed.s.abs(), false);
    let specs = item.get("faces").filter(|f| f.is_object());
    let mut faces = Vec::with_capacity(6);
    for (slot, name) in FaceName::ALL.into_iter().enumerate() {
        let mut face = Face::new(name, true, default_label.clone());
        if let Some(rect) = auto.get(slot) {
            face.uv = *rect;
        }
        if let Some(specs) = specs {
            match specs.get(mc::face_key(name)) {
                None => face.active = false,
                Some(spec) => read_face(spec, &mut face, &label, table, units, notices)?,
            }
        }
        faces.push(face);
    }
    let faces: [Face; 6] = faces.try_into().map_err(|_| McError::Syntax)?;

    Ok(Element {
        base,
        kind: ElementType::Cube,
        color: marker_color(index),
        faces,
        flags,
        marker: None,
    })
}

/// A face that the file lists.
fn read_face(
    spec: &Value,
    face: &mut Face,
    element: &str,
    table: &mut Table,
    units: Vec2,
    notices: &mut Vec<Notice>,
) -> Result<(), McError> {
    let key = text(spec, "texture")
        .map(|t| t.strip_prefix('#').unwrap_or(t))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| McError::FaceTexture {
            element: element.to_owned(),
            face: mc::face_key(face.name),
        })?;
    face.texture = table.label_or_add(key, notices);

    if let Some(uv) = spec.get("uv").and_then(Value::as_array) {
        let at = |i: usize| {
            if uv.len() < 4 {
                return 0.0;
            }
            uv.get(i).and_then(num).unwrap_or(0.0)
        };
        let (tl, br) = mc::uv_from_units([at(0), at(1), at(2), at(3)], units);
        face.uv.set_unclamped(tl, br);
    }
    face.uv.set_rot(int_at(spec, "rotation").unwrap_or(0));
    face.tint = int_at(spec, "tintindex").unwrap_or(-1);
    face.cull = text(spec, "cullface").map_or(FaceFlags::empty(), mc::cull_from_key);
    Ok(())
}
