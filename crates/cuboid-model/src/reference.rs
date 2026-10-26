//! Literal reference twin of the apply path: capture to records, recreate every object through
//! the setters, replay every voxel through the full writer (spec: arch 9.1).
//!
//! The twin covers restore and paste. It shares nothing with the fast paths of `apply` and
//! `normal_form` except the data types and the setters of W1.5: it turns every object into a
//! plain record (packed colour, folded UV, rotation, pivot, flags), creates a new object from the
//! record, lets the rotation setter quantise the rotation, and writes each voxel with the full
//! writer into a fresh world. The proptest below compares both on random documents and histories.

use std::sync::Arc;

use cuboid_num::pack;
use cuboid_voxel::{ChunkSlot, I3, VoxelWorld, WriteKind, mapping};
use glam::{Vec2, Vec3};
use rustc_hash::FxHashSet;

use crate::apply::{ApplyError, ApplyMode, ApplyReport, DEFAULT_VOXEL_BUDGET, voxel_world_bytes};
use crate::contract::{Document, EditCtx, EditEnv, ViewProps};
use crate::ids::{ObjId, ObjRef, Rev};
use crate::list::ListSnap;
use crate::objects::{
    Element, ElementFlags, Face, FaceFlags, FaceName, Label, Mesh, MeshBounds, MeshGeometry,
    MeshKind, Submesh, Texture, TextureEntry, TextureList,
};
use crate::selection::Selection;
use crate::setters::{BlueprintMut, ElementMut, MeshMut, RotOpts, set_model_box};
use crate::snapshot::{Categories, Snapshot};
use crate::uv::UvRect;

const FORCE: RotOpts = RotOpts {
    propagate: false,
    force: true,
    about_pivot: false,
};

/// An element as the file stores it: the keys of native 4.4, with the packed colour and the
/// folded UV corners. The fields a record does not hold (permissions) come from the template.
struct ElementRecord {
    p: Vec3,
    s: Vec3,
    r: Vec3,
    o: Vec3,
    c: i32,
    hidden: bool,
    locked: bool,
    label: Label,
    faces: [FaceRecord; 6],
    flags: ElementFlags,
    template: Element,
}

struct FaceRecord {
    name: FaceName,
    active: bool,
    tint: i32,
    cull: FaceFlags,
    texture: Label,
    rot: i32,
    tl: Vec2,
    br: Vec2,
}

/// A mesh as the file stores it: the keys of native 4.6 (sub-mesh colours packed, no tint). What
/// a record does not hold (permissions, normalisation factor, import offset, the tint of an
/// imported mesh) comes from the template; the tint of a primitive is the builder's.
struct MeshRecord {
    p: Vec3,
    s: Vec3,
    r: Vec3,
    o: Vec3,
    hidden: bool,
    locked: bool,
    label: Label,
    kind: MeshKind,
    geometry: Arc<MeshGeometry>,
    texture: Option<Texture>,
    submeshes: Vec<(Vec<u32>, i32, Option<Texture>)>,
    bounds: MeshBounds,
    template: Mesh,
}

/// The white main colour of the primitive material (tools 6.5).
const BUILDER_TINT: [f32; 4] = [1.0; 4];

fn element_record(el: &Element) -> ElementRecord {
    ElementRecord {
        p: el.base.p,
        s: el.base.s,
        r: el.base.e,
        o: el.base.o,
        c: pack::element(el.color),
        hidden: el.base.hidden,
        locked: el.base.locked,
        label: el.base.label.clone(),
        faces: el.faces.each_ref().map(|face| FaceRecord {
            name: face.name,
            active: face.active,
            tint: face.tint,
            cull: face.cull,
            texture: face.texture.clone(),
            rot: face.uv.rot(),
            tl: face.uv.tl(),
            br: face.uv.br(),
        }),
        flags: el.flags,
        template: el.clone(),
    }
}

fn mesh_record(mesh: &Mesh) -> MeshRecord {
    MeshRecord {
        p: mesh.base.p,
        s: mesh.base.s,
        r: mesh.base.e,
        o: mesh.base.o,
        hidden: mesh.base.hidden,
        locked: mesh.base.locked,
        label: mesh.base.label.clone(),
        kind: mesh.kind,
        geometry: Arc::clone(&mesh.geometry),
        texture: mesh.texture.clone(),
        submeshes: mesh
            .submeshes
            .iter()
            .map(|sub| {
                (
                    sub.triangles.clone(),
                    pack::submesh(sub.color),
                    sub.texture.clone(),
                )
            })
            .collect(),
        bounds: mesh.bounds,
        template: mesh.clone(),
    }
}

/// A new element from a record, still unrotated (a new object's rotation is zero).
fn create_element(rec: &ElementRecord, offset: Vec3) -> Element {
    let mut el = rec.template.clone();
    el.color = pack::unpack_element(rec.c);
    el.base.p = rec.p + offset;
    el.base.s = rec.s;
    el.base.e = Vec3::ZERO;
    el.base.o = rec.o + offset;
    el.base.hidden = rec.hidden;
    el.base.locked = rec.locked;
    el.base.label = rec.label.clone();
    el.faces = rec.faces.each_ref().map(|face| {
        let mut uv = UvRect::default();
        uv.set_rot(face.rot);
        uv.set(face.tl, face.br);
        Face {
            name: face.name,
            active: face.active,
            tint: face.tint,
            cull: face.cull,
            texture: face.texture.clone(),
            uv,
        }
    });
    el.flags = pasted_flags(rec.flags, offset);
    el
}

fn pasted_flags(flags: ElementFlags, offset: Vec3) -> ElementFlags {
    if offset == Vec3::ZERO {
        return flags;
    }
    flags - ElementFlags::UV_SNAP
}

fn create_mesh(rec: &MeshRecord, offset: Vec3) -> Mesh {
    let mut base = rec.template.base.clone();
    base.p = rec.p + offset;
    base.s = rec.s;
    base.e = Vec3::ZERO;
    base.o = rec.o + offset;
    base.hidden = rec.hidden;
    base.locked = rec.locked;
    base.label = rec.label.clone();
    Mesh {
        base,
        kind: rec.kind,
        geometry: Arc::clone(&rec.geometry),
        texture: rec.texture.clone(),
        submeshes: rec
            .submeshes
            .iter()
            .map(|(triangles, color, texture)| Submesh {
                triangles: triangles.clone(),
                color: pack::unpack_submesh(*color),
                texture: texture.clone(),
            })
            .collect(),
        bounds: rec.bounds,
        norm_factor: rec.template.norm_factor,
        import_offset: rec.template.import_offset,
        tint: if rec.kind == MeshKind::Custom {
            rec.template.tint
        } else {
            BUILDER_TINT
        },
    }
}

fn without_quotes(text: &str) -> String {
    let closing = char::from_u32(0x201D).unwrap_or('"');
    text.chars()
        .map(|c| if c == '"' { closing } else { c })
        .collect()
}

/// Checks what the apply path checks before it changes anything, in its order.
fn check(snap: &Snapshot, cats: Categories, restore: bool) -> Result<(), ApplyError> {
    let missing = |flag: Categories, present: bool| {
        if cats.contains(flag) && !present {
            return Err(ApplyError::Invalid(format!("{flag:?} is missing")));
        }
        Ok(())
    };
    missing(Categories::MODEL_PROPERTIES, snap.props.is_some())?;
    missing(Categories::VIEW_PROPERTIES, snap.view.is_some())?;
    missing(Categories::ELEMENTS, snap.elements.is_some())?;
    missing(Categories::TEXTURES, snap.textures.is_some())?;
    missing(Categories::VOXELS, snap.voxels.is_some())?;
    missing(Categories::MESHES, snap.meshes.is_some())?;
    missing(Categories::MODEL_DISPLAY, snap.display.is_some())?;
    if let Some(textures) = snap
        .textures
        .as_ref()
        .filter(|_| cats.contains(Categories::TEXTURES))
        && textures.entries.iter().any(|e| e.label.is_empty())
    {
        return Err(ApplyError::EmptyTextureLabel);
    }
    if let Some(Some(world)) = snap
        .voxels
        .as_ref()
        .filter(|_| cats.contains(Categories::VOXELS))
    {
        let allocated = world
            .slots
            .iter()
            .filter(|slot| matches!(slot, ChunkSlot::Data(_)))
            .count();
        let bytes = voxel_world_bytes(world.res, allocated)
            .ok_or(ApplyError::VoxelResolution(world.res))?;
        if bytes > DEFAULT_VOXEL_BUDGET {
            return Err(ApplyError::VoxelBudget {
                res: world.res,
                bytes,
                budget: DEFAULT_VOXEL_BUDGET,
            });
        }
        let n = (3 * world.res).div_ceil(16) as usize;
        if world.slots.len() != n * n * n {
            return Err(ApplyError::Invalid("voxel slot count".to_owned()));
        }
    }
    let list_ok = |ids: &[ObjId], items: usize, flags: usize| {
        let distinct = ids.iter().collect::<FxHashSet<_>>().len() == ids.len();
        ids.len() == items && ids.len() == flags && (distinct || !restore)
    };
    if let Some(e) = snap
        .elements
        .as_ref()
        .filter(|_| cats.contains(Categories::ELEMENTS))
        && !list_ok(&e.list.ids, e.list.items.len(), e.selected.len())
    {
        return Err(ApplyError::Invalid("element list".to_owned()));
    }
    if let Some(m) = snap
        .meshes
        .as_ref()
        .filter(|_| cats.contains(Categories::MESHES))
        && !list_ok(&m.list.ids, m.list.items.len(), m.selected.len())
    {
        return Err(ApplyError::Invalid("mesh list".to_owned()));
    }
    Ok(())
}

/// Restore or paste, literally: the result the fast path of `apply_document` must equal.
///
/// `Open` and `Merge` are not covered; they share every rule with these two.
///
/// spec: arch 9.1, native 6.2-6.3, tools 11.3, tools 15.3
pub fn apply_reference(
    cx: &mut EditCtx<'_>,
    snap: &Snapshot,
    mode: ApplyMode,
) -> Result<ApplyReport, ApplyError> {
    let (cats, offset, restore) = match mode {
        ApplyMode::Restore(wanted) => (wanted & snap.categories, Vec3::ZERO, true),
        ApplyMode::Paste { offset } => (snap.categories & Categories::OBJECTS, offset, false),
        ApplyMode::Open | ApplyMode::Merge => {
            return Err(ApplyError::Invalid(
                "the reference twin covers restore and paste".to_owned(),
            ));
        }
    };
    check(snap, cats, restore)?;
    let has = |flag: Categories| cats.contains(flag);
    let mut report = ApplyReport::default();

    if has(Categories::ELEMENTS) || has(Categories::MESHES) {
        cx.sel.clear();
    }

    // mp
    if let Some(props) = snap
        .props
        .as_ref()
        .filter(|_| has(Categories::MODEL_PROPERTIES))
    {
        let mut copy = (**props).clone();
        copy.general.description = without_quotes(&copy.general.description);
        copy.general.name = without_quotes(&copy.general.name);
        copy.minecraft.parent = without_quotes(&copy.minecraft.parent);
        let model_box = copy.general.model_box;
        cx.doc.props = Arc::new(copy);
        if let Some(model_box) = model_box {
            set_model_box(cx.doc, model_box);
        }
    }

    // vp
    if let Some(view) = snap
        .view
        .as_ref()
        .filter(|_| has(Categories::VIEW_PROPERTIES))
    {
        let mut copy: ViewProps = (*cx.doc.view).clone();
        for (live, new) in copy.cameras.iter_mut().zip(&view.cameras) {
            if new.is_some() {
                *live = *new;
            }
        }
        copy.mouse_step = view.mouse_step;
        copy.key_step = view.key_step;
        copy.display_step = view.display_step;
        cx.doc.view = Arc::new(copy);
    }

    // The rotation setters read the restriction through the environment.
    let env = EditEnv {
        restrict_rotations: cx.doc.props.general.restrict_rotations,
        ..cx.env.clone()
    };
    let mut ctx = EditCtx {
        doc: &mut *cx.doc,
        sel: &mut *cx.sel,
        env: &env,
        rng: &mut *cx.rng,
    };

    // bp and el
    if let Some(section) = snap.elements.as_ref().filter(|_| has(Categories::ELEMENTS)) {
        let bp = &section.blueprint.base;
        ctx.doc.blueprint.base.p = bp.p;
        ctx.doc.blueprint.base.o = bp.p;
        ctx.doc.blueprint.base.hidden = bp.hidden;
        ctx.doc.blueprint.base.locked = bp.locked;
        ctx.doc.blueprint.base.label = bp.label.clone();
        let mut guard = BlueprintMut::new(&mut ctx);
        guard.set_size(bp.s);
        guard.set_rotation(bp.e, FORCE);

        let records: Vec<ElementRecord> = section
            .list
            .items
            .iter()
            .map(|el| element_record(el))
            .collect();
        let ids = if restore {
            ctx.doc.elements.clear();
            let items = records
                .iter()
                .map(|rec| Arc::new(create_element(rec, offset)));
            ctx.doc.elements.restore(&ListSnap {
                ids: section.list.ids.clone(),
                items: items.collect(),
            });
            section.list.ids.clone()
        } else {
            records
                .iter()
                .map(|rec| ctx.doc.elements.push(create_element(rec, offset)))
                .collect()
        };
        for (&id, rec) in ids.iter().zip(&records) {
            if let Some(mut guard) = ElementMut::new(&mut ctx, id) {
                guard.set_rotation(rec.r, FORCE);
            }
            // Flags are applied after the rotation, which turns UV snap off.
            let doc = &mut *ctx.doc;
            if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
                el.flags = pasted_flags(rec.flags, offset);
            }
        }
        for (&id, &flag) in ids.iter().zip(&section.selected) {
            if flag || !restore {
                ctx.sel.add(ObjRef::Element(id));
            }
        }
        report
            .created
            .extend(ids.iter().map(|&id| ObjRef::Element(id)));
    }

    // tx
    if let Some(textures) = snap.textures.as_ref().filter(|_| has(Categories::TEXTURES)) {
        let mut list = TextureList::default();
        for entry in &textures.entries {
            match list.entries.iter().position(|e| e.label == entry.label) {
                Some(at) => list.entries[at].texture = entry.texture.clone(),
                None => list.entries.push(TextureEntry {
                    label: entry.label.clone(),
                    texture: entry.texture.clone(),
                }),
            }
        }
        ctx.doc.textures = list;
    }

    // vx
    if let Some(voxels) = snap.voxels.as_ref().filter(|_| has(Categories::VOXELS)) {
        ctx.doc.voxels = voxels.as_ref().map(|world| replayed_world(ctx.doc, world));
    }

    // me
    if let Some(section) = snap.meshes.as_ref().filter(|_| has(Categories::MESHES)) {
        let records: Vec<MeshRecord> = section
            .list
            .items
            .iter()
            .map(|mesh| mesh_record(mesh))
            .collect();
        let ids = if restore {
            ctx.doc.meshes.clear();
            let items = records.iter().map(|rec| Arc::new(create_mesh(rec, offset)));
            ctx.doc.meshes.restore(&ListSnap {
                ids: section.list.ids.clone(),
                items: items.collect(),
            });
            section.list.ids.clone()
        } else {
            records
                .iter()
                .map(|rec| ctx.doc.meshes.push(create_mesh(rec, offset)))
                .collect()
        };
        for (&id, rec) in ids.iter().zip(&records) {
            if let Some(mut guard) = MeshMut::new(&mut ctx, id) {
                guard.set_rotation(rec.r, FORCE);
            }
        }
        for (&id, &flag) in ids.iter().zip(&section.selected) {
            if flag || !restore {
                ctx.sel.add(ObjRef::Mesh(id));
            }
        }
        report
            .created
            .extend(ids.iter().map(|&id| ObjRef::Mesh(id)));
    }

    // md
    if let Some(display) = snap
        .display
        .as_ref()
        .filter(|_| has(Categories::MODEL_DISPLAY))
    {
        ctx.doc.display = Arc::new((**display).clone());
    }

    let doc = &*ctx.doc;
    ctx.sel.highlighted.retain(|obj| match *obj {
        ObjRef::Element(id) => doc.elements.get(id).is_some(),
        ObjRef::Mesh(id) => doc.meshes.get(id).is_some(),
        ObjRef::Group(id) => doc.groups.get(id).is_some(),
        ObjRef::Blueprint | ObjRef::ModelOrigin => true,
    });
    ctx.doc.revision = Rev::next();
    Ok(report)
}

/// A fresh world with every stored voxel written by the full writer, in slot and scan order. The
/// settings come from the live world of the same resolution, or from the document.
fn replayed_world(doc: &Document, snap: &cuboid_voxel::VoxelSnap) -> VoxelWorld {
    let (ao, active, corners) = match &doc.voxels {
        Some(live) if live.res == snap.res => (live.ao, live.active_box, live.setup_corners),
        _ => (
            doc.props.mesh_export.ambient_occlusion,
            mapping::active_box(doc.model_box, snap.res),
            doc.model_box,
        ),
    };
    let mut world = VoxelWorld::new(snap.res, ao, active);
    world.setup_corners = corners;
    let n = (3 * snap.res).div_ceil(16) as usize;
    let mut cells: Vec<(I3, u32)> = Vec::new();
    for (slot, content) in snap.slots.iter().enumerate() {
        let ChunkSlot::Data(chunk) = content else {
            continue;
        };
        let to_i32 = |v: usize| i32::try_from(v).unwrap_or(0);
        let corner = I3::new(
            to_i32(slot / (n * n)),
            to_i32(slot / n % n),
            to_i32(slot % n),
        ) * 16;
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    let local = x + 16 * y + 256 * z;
                    let value = chunk.cell(local);
                    if value != 0 {
                        cells.push((corner + I3::new(to_i32(x), to_i32(y), to_i32(z)), value));
                    }
                }
            }
        }
    }
    world.write_many(&mut cells.into_iter(), WriteKind::Full);
    world
}

/// Where an object sits, for comparisons that ignore ids.
fn place(doc: &Document, obj: ObjRef) -> (u8, Option<usize>) {
    match obj {
        ObjRef::Element(id) => (0, doc.elements.position(id)),
        ObjRef::Mesh(id) => (1, doc.meshes.position(id)),
        ObjRef::Blueprint => (2, Some(0)),
        ObjRef::ModelOrigin => (3, Some(0)),
        ObjRef::Group(id) => (4, doc.groups.position(id)),
    }
}

fn ensure(ok: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what()) }
}

fn mesh_eq(a: &Mesh, b: &Mesh) -> bool {
    let texture_eq =
        |a: &Option<crate::objects::Texture>, b: &Option<crate::objects::Texture>| match (a, b) {
            (Some(a), Some(b)) => {
                a.name == b.name && a.source == b.source && Arc::ptr_eq(&a.image, &b.image)
            }
            (None, None) => true,
            _ => false,
        };
    a.base == b.base
        && a.kind == b.kind
        && a.geometry == b.geometry
        && texture_eq(&a.texture, &b.texture)
        && a.submeshes.len() == b.submeshes.len()
        && a.submeshes.iter().zip(b.submeshes.iter()).all(|(x, y)| {
            x.triangles == y.triangles && x.color == y.color && texture_eq(&x.texture, &y.texture)
        })
        && a.bounds == b.bounds
        && a.norm_factor == b.norm_factor
        && a.import_offset == b.import_offset
        && a.tint == b.tint
}

fn world_eq(a: &VoxelWorld, b: &VoxelWorld) -> Result<(), String> {
    ensure(a.res == b.res, || "voxel resolution".to_owned())?;
    ensure(a.active_box == b.active_box, || "active box".to_owned())?;
    ensure(a.setup_corners == b.setup_corners, || {
        "setup corners".to_owned()
    })?;
    ensure(a.ao == b.ao, || "ambient occlusion".to_owned())?;
    ensure(a.palette.values() == b.palette.values(), || {
        format!(
            "palette {:?} vs {:?}",
            a.palette.values(),
            b.palette.values()
        )
    })?;
    ensure(a.slots().len() == b.slots().len(), || {
        "slot count".to_owned()
    })?;
    for (slot, (x, y)) in a.slots().iter().zip(b.slots()).enumerate() {
        let same = match (x, y) {
            (ChunkSlot::Unallocated, ChunkSlot::Unallocated)
            | (ChunkSlot::EmptyWritten, ChunkSlot::EmptyWritten) => true,
            (ChunkSlot::Data(x), ChunkSlot::Data(y)) => {
                x.cells() == y.cells() && x.nonzero() == y.nonzero()
            }
            _ => false,
        };
        ensure(same, || format!("voxel slot {slot}"))?;
    }
    Ok(())
}

/// Compares two documents with their selections the way a user could tell them apart: every
/// section, the lists record by record (ids and display ids included when `same_ids`), the
/// voxel worlds cell by cell and the selection by position. Journals, revisions and the dirty
/// sets of the worlds are not compared.
///
/// spec: arch 9.1
pub fn documents_match(
    a: (&Document, &Selection),
    b: (&Document, &Selection),
    same_ids: bool,
) -> Result<(), String> {
    let ((da, sa), (db, sb)) = (a, b);
    ensure(*da.props == *db.props, || "model properties".to_owned())?;
    ensure(*da.view == *db.view, || "view properties".to_owned())?;
    ensure(da.blueprint == db.blueprint, || {
        format!("blueprint {:?} vs {:?}", da.blueprint, db.blueprint)
    })?;
    ensure(da.model_box == db.model_box, || "model box".to_owned())?;
    ensure(*da.display == *db.display, || "display slots".to_owned())?;
    ensure(*da.overrides == *db.overrides, || "overrides".to_owned())?;

    ensure(
        da.textures.entries.len() == db.textures.entries.len(),
        || "texture count".to_owned(),
    )?;
    for (x, y) in da.textures.entries.iter().zip(&db.textures.entries) {
        let same = x.label == y.label
            && x.texture.name == y.texture.name
            && x.texture.source == y.texture.source
            && Arc::ptr_eq(&x.texture.image, &y.texture.image);
        ensure(same, || format!("texture {}", x.label))?;
    }

    ensure(da.elements.len() == db.elements.len(), || {
        "element count".to_owned()
    })?;
    ensure(
        da.elements.display_ids() == db.elements.display_ids(),
        || "element display ids".to_owned(),
    )?;
    ensure(!same_ids || da.elements.ids() == db.elements.ids(), || {
        "element ids".to_owned()
    })?;
    for (at, (x, y)) in da
        .elements
        .items()
        .iter()
        .zip(db.elements.items())
        .enumerate()
    {
        ensure(**x == **y, || format!("element {at}: {x:?} vs {y:?}"))?;
    }
    ensure(da.meshes.len() == db.meshes.len(), || {
        "mesh count".to_owned()
    })?;
    ensure(da.meshes.display_ids() == db.meshes.display_ids(), || {
        "mesh display ids".to_owned()
    })?;
    ensure(!same_ids || da.meshes.ids() == db.meshes.ids(), || {
        "mesh ids".to_owned()
    })?;
    for (at, (x, y)) in da.meshes.items().iter().zip(db.meshes.items()).enumerate() {
        ensure(mesh_eq(x, y), || format!("mesh {at}: {x:?} vs {y:?}"))?;
    }

    match (&da.voxels, &db.voxels) {
        (None, None) => {}
        (Some(x), Some(y)) => world_eq(x, y)?,
        _ => return Err("one document has a voxel world".to_owned()),
    }

    let places = |doc: &Document, sel: &Selection| {
        (
            sel.list.iter().map(|&o| place(doc, o)).collect::<Vec<_>>(),
            sel.primary.map(|o| place(doc, o)),
            sel.highlighted
                .iter()
                .map(|&o| place(doc, o))
                .collect::<Vec<_>>(),
        )
    };
    ensure(places(da, sa) == places(db, sb), || {
        format!("selection {:?} vs {:?}", places(da, sa), places(db, sb))
    })
}

#[cfg(test)]
mod tests {
    use cuboid_image::{Image, RgbaBuf};
    use cuboid_num::EngineRng;
    use proptest::prelude::*;

    use super::*;
    use crate::apply::{ApplySource, apply_document};
    use crate::capture::{CaptureScope, capture};
    use crate::contract::{CameraState, DisplaySlots, ModelBox, ModelProps, RenderPrefs};
    use crate::objects::Blueprint;
    use crate::objects::{
        ElementType, MeshBounds, MeshGeometry, MeshKind, ObjectBase, Perms, Submesh,
    };

    const ROT: [f32; 16] = [
        0.0, 5.0, 7.0, 22.5, 30.0, 45.0, 100.0, 190.5, 359.0, 360.0, -40.0, -90.0, -400.0, 725.0,
        11.25, 33.75,
    ];
    const UVC: [f32; 13] = [
        -1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0,
    ];
    const UROT: [i32; 8] = [0, 90, 180, 270, 450, -90, -450, 45];
    const COL: [f32; 7] = [0.0, 0.1, 0.5, 0.999, 1.0, 1.5, -0.2];
    const LABELS: [&str; 5] = ["texture", "particle", "a", "b", "error"];
    const NAMES: [&str; 4] = ["Box", "q\"uoted", "", "x y"];
    const VOXEL: [u32; 5] = [
        0xFF11_2233,
        0xFF44_5566,
        0xFF77_8899,
        0x0011_2233,
        0xFFFF_FFFF,
    ];
    const RES: [u32; 7] = [1, 2, 3, 4, 6, 8, 12];

    fn pick<T: Clone + std::fmt::Debug + 'static>(
        table: &[T],
    ) -> impl Strategy<Value = T> + use<T> {
        prop::sample::select(table.to_vec())
    }

    fn quarter(v: [i32; 3]) -> Vec3 {
        Vec3::new(v[0] as f32 * 0.25, v[1] as f32 * 0.25, v[2] as f32 * 0.25)
    }

    fn rotation() -> impl Strategy<Value = Vec3> {
        prop::array::uniform3(prop_oneof![3 => Just(0.0_f32), 4 => pick(&ROT)])
            .prop_map(Vec3::from_array)
    }

    fn uv() -> impl Strategy<Value = UvRect> {
        (pick(&UROT), prop::array::uniform4(pick(&UVC)), 0u8..8).prop_map(|(r, c, wild)| {
            let (tl, br) = (Vec2::new(c[0], c[1]), Vec2::new(c[2], c[3]));
            let mut rect = UvRect::new(r, tl, br);
            if wild == 0 {
                rect.set_unclamped(tl * 2.0, br * 3.0);
            }
            rect
        })
    }

    fn element() -> impl Strategy<Value = Element> {
        (
            prop::array::uniform3(-32i32..32),
            prop::array::uniform3(0i32..16),
            rotation(),
            prop::array::uniform4(pick(&COL)),
            prop::array::uniform6((any::<bool>(), -1i32..3, 0u8..64, pick(&LABELS), uv())),
            (
                0u8..32,
                any::<bool>(),
                any::<bool>(),
                pick(&NAMES),
                prop::array::uniform3(-8i32..8),
                pick(&[
                    ElementType::Cube,
                    ElementType::PlaneX,
                    ElementType::PlaneY,
                    ElementType::PlaneZ,
                ]),
            ),
        )
            .prop_map(
                |(p, s, e, color, faces, (flags, hidden, locked, label, pivot, kind))| {
                    let mut el = Element::new(
                        kind,
                        &Blueprint::default(),
                        1,
                        color,
                        &Label::from("texture"),
                    );
                    el.base.p = quarter(p);
                    el.base.s = quarter(s);
                    el.base.e = e;
                    el.base.o = quarter(p) + quarter(pivot);
                    el.base.hidden = hidden;
                    el.base.locked = locked;
                    el.base.label = Label::from(label);
                    el.flags = ElementFlags::from_bits_truncate(flags);
                    for (face, (active, tint, cull, texture, rect)) in
                        el.faces.iter_mut().zip(faces)
                    {
                        face.active = active;
                        face.tint = tint;
                        face.cull = FaceFlags::from_bits_truncate(cull);
                        face.texture = Label::from(texture);
                        face.uv = rect;
                    }
                    el
                },
            )
    }

    fn image(size: u32) -> Arc<cuboid_image::Image> {
        Arc::new(Image::from_rgba(RgbaBuf::new(size, size)))
    }

    fn texture_entry() -> impl Strategy<Value = TextureEntry> {
        (
            pick(&LABELS),
            pick(&NAMES),
            1u32..3,
            prop::option::of(pick(&["", "C:/x.png"])),
        )
            .prop_map(|(label, name, size, path)| {
                let mut texture = crate::objects::Texture::new(image(size));
                texture.name = Label::from(name);
                texture.source.path = path.map(str::to_owned);
                TextureEntry {
                    label: Label::from(label),
                    texture,
                }
            })
    }

    fn mesh() -> impl Strategy<Value = Mesh> {
        (
            prop::array::uniform3(-32i32..32),
            prop::array::uniform3(-8i32..8),
            rotation(),
            pick(&[
                MeshKind::Custom,
                MeshKind::Cube,
                MeshKind::Sphere,
                MeshKind::Sprite,
            ]),
            (any::<bool>(), any::<bool>()),
            prop::option::of(texture_entry()),
            0usize..3,
            prop::array::uniform4(pick(&COL)),
        )
            .prop_map(|(p, s, e, kind, (hidden, locked), texture, parts, tint)| {
                let perms = if kind == MeshKind::Custom {
                    Perms::IMPORTED_MESH
                } else {
                    Perms::PRIMITIVE
                };
                let mut base = ObjectBase::new("Mesh", quarter(p), quarter(s), perms);
                base.e = e;
                base.hidden = hidden;
                base.locked = locked;
                Mesh {
                    base,
                    kind,
                    geometry: Arc::new(MeshGeometry {
                        vertices: vec![Vec3::ONE; parts],
                        ..Default::default()
                    }),
                    texture: texture.map(|t| t.texture),
                    submeshes: (0..parts)
                        .map(|i| Submesh {
                            triangles: vec![0; i],
                            color: tint,
                            texture: None,
                        })
                        .collect(),
                    bounds: MeshBounds::default(),
                    norm_factor: 1.0,
                    import_offset: Vec3::ZERO,
                    tint,
                }
            })
    }

    #[derive(Clone, Debug)]
    struct VoxelPlan {
        res: u32,
        ao: bool,
        cells: Vec<([u32; 3], u32)>,
        erase: Vec<[u32; 3]>,
    }

    fn voxel_plan() -> impl Strategy<Value = VoxelPlan> {
        (
            pick(&RES),
            any::<bool>(),
            prop::collection::vec((prop::array::uniform3(0u32..64), pick(&VOXEL)), 0..24),
            prop::collection::vec(prop::array::uniform3(0u32..64), 0..6),
        )
            .prop_map(|(res, ao, cells, erase)| VoxelPlan {
                res,
                ao,
                cells,
                erase,
            })
    }

    #[derive(Clone, Debug)]
    struct Plan {
        props: ModelProps,
        view: ViewProps,
        display: DisplaySlots,
        blueprint: Blueprint,
        elements: Vec<(Element, bool)>,
        meshes: Vec<(Mesh, bool)>,
        textures: Vec<TextureEntry>,
        model_box: Option<[i32; 6]>,
        voxel: Option<VoxelPlan>,
    }

    fn props() -> impl Strategy<Value = ModelProps> {
        (
            any::<bool>(),
            (pick(&NAMES), pick(&NAMES), pick(&NAMES)),
            prop::option::of(prop::array::uniform6(-64i32..64)),
            any::<bool>(),
            prop::array::uniform3(-8i32..8),
        )
            .prop_map(|(restrict, (name, desc, parent), mb, ao, origin)| {
                let mut props = ModelProps::default();
                props.general.restrict_rotations = restrict;
                props.general.name = name.to_owned();
                props.general.description = desc.to_owned();
                props.minecraft.parent = parent.to_owned();
                props.mesh_export.ambient_occlusion = ao;
                props.general.origin = quarter(origin);
                props.general.model_box = mb.map(box_of);
                props
            })
    }

    fn box_of(v: [i32; 6]) -> ModelBox {
        ModelBox {
            min: quarter([v[0], v[1], v[2]]),
            max: quarter([v[3], v[4], v[5]]),
        }
    }

    fn view() -> impl Strategy<Value = ViewProps> {
        (
            prop::array::uniform4(prop::option::of(0u8..3)),
            pick(&[0.125_f32, 0.5, 1.0, 2.0]),
            pick(&[0.125_f32, 0.5, 1.0]),
            any::<bool>(),
        )
            .prop_map(|(cams, step, uv_step, large)| ViewProps {
                cameras: cams.map(|c| {
                    c.map(|n| CameraState {
                        pos: Vec3::splat(f32::from(n)),
                        rot: glam::Quat::IDENTITY,
                        ortho: false,
                        ortho_size: 5.0,
                        fov_deg: 60.0,
                    })
                }),
                mouse_step: step,
                key_step: step,
                uv_step,
                display_step: step,
                large_grid: large,
            })
    }

    fn display() -> impl Strategy<Value = DisplaySlots> {
        prop::array::uniform8((
            any::<bool>(),
            prop::array::uniform3(pick(&[0.0_f32, 1.0, 2.0])),
        ))
        .prop_map(|slots| {
            let mut display = DisplaySlots::default();
            for (slot, (enabled, scale)) in display.slots.iter_mut().zip(slots) {
                slot.enabled = enabled;
                slot.scale = Vec3::from_array(scale);
            }
            display
        })
    }

    fn blueprint() -> impl Strategy<Value = Blueprint> {
        (
            prop::array::uniform3(-32i32..32),
            prop::array::uniform3(pick(&[0.5_f32, 1.0, 2.0, 4.0])),
            rotation(),
            (any::<bool>(), any::<bool>()),
        )
            .prop_map(|(p, s, e, (hidden, locked))| {
                let mut bp = Blueprint::default();
                bp.base.p = quarter(p);
                bp.base.o = bp.base.p;
                bp.base.s = Vec3::from_array(s);
                bp.base.e = e;
                bp.base.hidden = hidden;
                bp.base.locked = locked;
                bp
            })
    }

    fn plan() -> impl Strategy<Value = Plan> {
        (
            (props(), view(), display(), blueprint()),
            prop::collection::vec((element(), any::<bool>()), 0..7),
            prop::collection::vec((mesh(), any::<bool>()), 0..4),
            prop::collection::vec(texture_entry(), 0..4),
            prop::option::of(prop::array::uniform6(-64i32..64)),
            prop::option::of(voxel_plan()),
        )
            .prop_map(
                |(
                    (props, view, display, blueprint),
                    elements,
                    meshes,
                    textures,
                    model_box,
                    voxel,
                )| {
                    Plan {
                        props,
                        view,
                        display,
                        blueprint,
                        elements,
                        meshes,
                        textures,
                        model_box,
                        voxel,
                    }
                },
            )
    }

    fn make_world(doc: &Document, plan: &VoxelPlan) -> VoxelWorld {
        let mut world = VoxelWorld::new(
            plan.res,
            plan.ao,
            mapping::active_box(doc.model_box, plan.res),
        );
        world.setup_corners = doc.model_box;
        let grid = 3 * plan.res;
        let at = |c: &[u32; 3]| {
            let wrap = |v: u32| i32::try_from(v % grid).unwrap_or(0);
            I3::new(wrap(c[0]), wrap(c[1]), wrap(c[2]))
        };
        let writes: Vec<(I3, u32)> = plan.cells.iter().map(|(c, v)| (at(c), *v)).collect();
        world.write_many(&mut writes.into_iter(), WriteKind::Unchecked);
        let erased: Vec<(I3, u32)> = plan.erase.iter().map(|c| (at(c), 0)).collect();
        world.write_many(&mut erased.into_iter(), WriteKind::Unchecked);
        world
    }

    fn build(plan: &Plan) -> (Document, Selection) {
        let mut seen = FxHashSet::default();
        let entries = plan
            .textures
            .iter()
            .filter(|e| seen.insert(e.label.clone()))
            .cloned()
            .collect();
        let mut doc = Document::new(TextureList { entries });
        doc.props = Arc::new(plan.props.clone());
        doc.view = Arc::new(plan.view.clone());
        doc.display = Arc::new(plan.display.clone());
        doc.blueprint = plan.blueprint.clone();
        if let Some(b) = plan.model_box {
            set_model_box(&mut doc, box_of(b));
        }
        let mut sel = Selection::default();
        for (el, on) in &plan.elements {
            let id = doc.elements.push(el.clone());
            if *on {
                sel.add(ObjRef::Element(id));
            }
        }
        for (mesh, on) in &plan.meshes {
            let id = doc.meshes.push(mesh.clone());
            if *on {
                sel.add(ObjRef::Mesh(id));
            }
        }
        doc.voxels = plan.voxel.as_ref().map(|v| make_world(&doc, v));
        (doc, sel)
    }

    #[derive(Clone, Debug)]
    enum Op {
        RemoveElement(usize),
        RemoveMesh(usize),
        AddElement(Box<Element>),
        HideElement(usize),
        RotateElement(usize, Vec3),
        FlipRestrict,
        Rename(&'static str),
        ClearTextures,
        AddTexture(TextureEntry),
        ModelBox([i32; 6]),
        DropWorld,
        NewWorld(VoxelPlan),
        WriteCell([u32; 3], u32),
        ClearSelection,
        Highlight(usize),
        MoveBlueprint(i32),
        Rebuild(Box<Plan>),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..8).prop_map(Op::RemoveElement),
            (0usize..8).prop_map(Op::RemoveMesh),
            element().prop_map(|el| Op::AddElement(Box::new(el))),
            (0usize..8).prop_map(Op::HideElement),
            (0usize..8, rotation()).prop_map(|(i, r)| Op::RotateElement(i, r)),
            Just(Op::FlipRestrict),
            pick(&NAMES).prop_map(Op::Rename),
            Just(Op::ClearTextures),
            texture_entry().prop_map(Op::AddTexture),
            prop::array::uniform6(-64i32..64).prop_map(Op::ModelBox),
            Just(Op::DropWorld),
            voxel_plan().prop_map(Op::NewWorld),
            (prop::array::uniform3(0u32..64), pick(&VOXEL)).prop_map(|(c, v)| Op::WriteCell(c, v)),
            Just(Op::ClearSelection),
            (0usize..8).prop_map(Op::Highlight),
            (-8i32..8).prop_map(Op::MoveBlueprint),
            plan().prop_map(|p| Op::Rebuild(Box::new(p))),
        ]
    }

    fn apply_op(doc: &mut Document, sel: &mut Selection, op: &Op) {
        let nth = |len: usize, i: usize| (len > 0).then(|| i % len);
        match op {
            Op::RemoveElement(i) => {
                if let Some(at) = nth(doc.elements.len(), *i) {
                    let id = doc.elements.ids()[at];
                    doc.elements.remove_set(&FxHashSet::from_iter([id]));
                    sel.remove(ObjRef::Element(id));
                }
            }
            Op::RemoveMesh(i) => {
                if let Some(at) = nth(doc.meshes.len(), *i) {
                    let id = doc.meshes.ids()[at];
                    doc.meshes.remove_set(&FxHashSet::from_iter([id]));
                    sel.remove(ObjRef::Mesh(id));
                }
            }
            Op::AddElement(el) => {
                doc.elements.push((**el).clone());
            }
            Op::HideElement(i) => {
                if let Some(at) = nth(doc.elements.len(), *i) {
                    let id = doc.elements.ids()[at];
                    if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
                        el.base.hidden = !el.base.hidden;
                    }
                }
            }
            Op::RotateElement(i, r) => {
                if let Some(at) = nth(doc.elements.len(), *i) {
                    let id = doc.elements.ids()[at];
                    if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
                        el.base.e = *r;
                    }
                }
            }
            Op::FlipRestrict => {
                let props = Arc::make_mut(&mut doc.props);
                props.general.restrict_rotations = !props.general.restrict_rotations;
            }
            Op::Rename(name) => Arc::make_mut(&mut doc.props).general.name = (*name).to_owned(),
            Op::ClearTextures => doc.textures.entries.clear(),
            Op::AddTexture(entry) => {
                if !doc.textures.entries.iter().any(|e| e.label == entry.label) {
                    doc.textures.entries.push(entry.clone());
                }
            }
            Op::ModelBox(b) => set_model_box(doc, box_of(*b)),
            Op::DropWorld => doc.voxels = None,
            Op::NewWorld(plan) => doc.voxels = Some(make_world(doc, plan)),
            Op::WriteCell(c, v) => {
                if let Some(world) = &mut doc.voxels {
                    let grid = 3 * world.res;
                    let wrap = |v: u32| i32::try_from(v % grid).unwrap_or(0);
                    let cell = I3::new(wrap(c[0]), wrap(c[1]), wrap(c[2]));
                    world.write_many(&mut [(cell, *v)].into_iter(), WriteKind::Full);
                }
            }
            Op::ClearSelection => sel.clear(),
            Op::Highlight(i) => {
                if let Some(at) = nth(doc.elements.len(), *i) {
                    sel.highlighted
                        .insert(ObjRef::Element(doc.elements.ids()[at]));
                }
                sel.highlighted.insert(ObjRef::Element(ObjId::next()));
            }
            Op::MoveBlueprint(n) => {
                doc.blueprint.base.p.x += *n as f32;
                doc.blueprint.base.o = doc.blueprint.base.p;
            }
            Op::Rebuild(plan) => (*doc, *sel) = build(plan),
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Tweak {
        Nothing,
        EmptyLabel,
        DuplicateLabel,
        BadResolution,
        ShortSlots,
        FlagCount,
        RepeatedId,
        TinyBlueprint,
    }

    fn tweak() -> impl Strategy<Value = Tweak> {
        prop_oneof![
            24 => Just(Tweak::Nothing),
            1 => Just(Tweak::EmptyLabel),
            2 => Just(Tweak::DuplicateLabel),
            1 => Just(Tweak::BadResolution),
            1 => Just(Tweak::ShortSlots),
            1 => Just(Tweak::FlagCount),
            1 => Just(Tweak::RepeatedId),
            2 => Just(Tweak::TinyBlueprint),
        ]
    }

    fn apply_tweak(snap: &mut Snapshot, tweak: Tweak) {
        match tweak {
            Tweak::Nothing => {}
            Tweak::EmptyLabel => {
                if let Some(textures) = &mut snap.textures {
                    let mut list = (**textures).clone();
                    list.entries.push(TextureEntry {
                        label: Label::default(),
                        texture: crate::objects::Texture::new(image(1)),
                    });
                    *textures = Arc::new(list);
                }
            }
            Tweak::DuplicateLabel => {
                if let Some(textures) = &mut snap.textures {
                    let mut list = (**textures).clone();
                    if let Some(first) = list.entries.first().cloned() {
                        let mut again = first;
                        again.texture = crate::objects::Texture::new(image(3));
                        list.entries.push(again);
                    }
                    *textures = Arc::new(list);
                }
            }
            Tweak::BadResolution => {
                if let Some(Some(world)) = &mut snap.voxels {
                    // Zero, over the budget, or a grid whose address arithmetic overflows.
                    world.res = [0, 100_000, u32::MAX][world.res as usize % 3];
                }
            }
            Tweak::ShortSlots => {
                if let Some(Some(world)) = &mut snap.voxels {
                    world.slots = world.slots[..world.slots.len() / 2].into();
                }
            }
            Tweak::FlagCount => {
                if let Some(elements) = &mut snap.elements {
                    elements.selected.push(true);
                }
            }
            Tweak::RepeatedId => {
                if let Some(elements) = &mut snap.elements
                    && elements.list.ids.len() > 1
                {
                    elements.list.ids[1] = elements.list.ids[0];
                }
            }
            Tweak::TinyBlueprint => {
                if let Some(elements) = &mut snap.elements {
                    elements.blueprint.base.s = Vec3::new(0.0, 0.0005, 1.0);
                    elements.blueprint.base.o = Vec3::splat(3.0);
                }
            }
        }
    }

    #[derive(Clone, Debug)]
    struct Round {
        capture: u8,
        selection_only: bool,
        ops: Vec<Op>,
        restore: u8,
        paste: Option<[i32; 3]>,
        tweak: Tweak,
    }

    fn round() -> impl Strategy<Value = Round> {
        (
            0u8..128,
            any::<bool>(),
            prop::collection::vec(op(), 0..4),
            0u8..128,
            prop::option::of(prop::array::uniform3(-4i32..4)),
            tweak(),
        )
            .prop_map(
                |(capture, selection_only, ops, restore, paste, tweak)| Round {
                    capture,
                    selection_only,
                    ops,
                    restore,
                    paste,
                    tweak,
                },
            )
    }

    fn env_for(doc: &Document) -> EditEnv {
        EditEnv {
            restrict_rotations: doc.props.general.restrict_rotations,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        }
    }

    fn run(plan: &Plan, rounds: &[Round]) -> Result<(), TestCaseError> {
        let (mut doc, mut sel) = build(plan);
        let mut rng = EngineRng::seeded(7);
        for (n, round) in rounds.iter().enumerate() {
            let scope = CaptureScope {
                categories: Categories::from_bits_truncate(round.capture),
                selection_only: round.selection_only,
            };
            let mut snap = capture(&doc, &sel, scope);
            apply_tweak(&mut snap, round.tweak);
            for op in &round.ops {
                apply_op(&mut doc, &mut sel, op);
            }
            let (mode, same_ids) = match round.paste {
                Some(offset) => (
                    ApplyMode::Paste {
                        offset: quarter(offset),
                    },
                    false,
                ),
                None => (
                    ApplyMode::Restore(Categories::from_bits_truncate(round.restore)),
                    true,
                ),
            };
            let (mut a, mut sa) = (doc.clone(), sel.clone());
            let (mut b, mut sb) = (doc.clone(), sel.clone());
            let (env_a, env_b) = (env_for(&a), env_for(&b));
            let before_ids = (a.elements.ids().to_vec(), a.meshes.ids().to_vec());
            let ra = apply_document(
                &mut EditCtx {
                    doc: &mut a,
                    sel: &mut sa,
                    env: &env_a,
                    rng: &mut rng,
                },
                ApplySource::Snapshot(&snap),
                mode,
            );
            let rb = apply_reference(
                &mut EditCtx {
                    doc: &mut b,
                    sel: &mut sb,
                    env: &env_b,
                    rng: &mut rng,
                },
                &snap,
                mode,
            );
            match (ra, rb) {
                (Ok(ra), Ok(rb)) => {
                    let verdict = documents_match((&a, &sa), (&b, &sb), same_ids);
                    prop_assert!(verdict.is_ok(), "round {n}: {}", verdict.unwrap_err());
                    prop_assert_eq!(ra.created.len(), rb.created.len());
                    if !same_ids {
                        let old = &before_ids;
                        prop_assert!(a.elements.ids()[..old.0.len()] == old.0[..]);
                        prop_assert!(a.meshes.ids()[..old.1.len()] == old.1[..]);
                        let fresh: FxHashSet<_> = a.elements.ids().iter().collect();
                        prop_assert_eq!(fresh.len(), a.elements.len());
                    } else {
                        prop_assert_eq!(&ra.created, &rb.created);
                    }
                    if let (Some(x), Some(y)) = (&mut a.voxels, &mut b.voxels) {
                        let (dirty_a, dirty_b) = (x.take_dirty(), y.take_dirty());
                        prop_assert!(
                            dirty_b.is_subset(&dirty_a),
                            "round {n}: the fast path misses dirty chunks"
                        );
                    }
                    doc = a;
                    sel = sa;
                }
                (Err(ea), Err(eb)) => {
                    prop_assert_eq!(std::mem::discriminant(&ea), std::mem::discriminant(&eb));
                    let verdict = documents_match((&a, &sa), (&doc, &sel), true);
                    prop_assert!(
                        verdict.is_ok(),
                        "round {n}: a failed apply changed: {verdict:?}"
                    );
                    let verdict = documents_match((&b, &sb), (&doc, &sel), true);
                    prop_assert!(verdict.is_ok(), "round {n}: the twin changed: {verdict:?}");
                }
                (ra, rb) => prop_assert!(false, "round {n}: {ra:?} vs {rb:?}"),
            }
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 10_000,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        /// Acceptance of W1.6: the restore and paste fast paths equal the record-shape twin on
        /// random documents and histories.
        #[test]
        fn the_fast_paths_equal_the_reference_twin(
            plan in plan(),
            rounds in prop::collection::vec(round(), 1..4),
        ) {
            run(&plan, &rounds)?;
        }
    }

    // --- The comparison itself must not be vacuous ---------------------------------------------

    fn sample() -> (Document, Selection) {
        let mut doc = Document::new(TextureList {
            entries: vec![TextureEntry {
                label: Label::from("a"),
                texture: crate::objects::Texture::new(image(1)),
            }],
        });
        let mut sel = Selection::default();
        let el = doc.elements.push(Element::new(
            ElementType::Cube,
            &Blueprint::default(),
            1,
            [1.0; 4],
            &Label::from("a"),
        ));
        sel.add(ObjRef::Element(el));
        let mesh = Mesh {
            base: ObjectBase::new("Mesh", Vec3::ZERO, Vec3::ONE, Perms::PRIMITIVE),
            kind: MeshKind::Cube,
            geometry: Arc::default(),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        };
        doc.meshes.push(mesh);
        let mut world = VoxelWorld::new(2, true, mapping::grid_box(2));
        world.write_full(I3::new(1, 1, 1), VOXEL[0]);
        doc.voxels = Some(world);
        (doc, sel)
    }

    #[test]
    fn the_comparison_sees_every_kind_of_difference() {
        let (doc, sel) = sample();
        let same = |d: &Document, s: &Selection| documents_match((&doc, &sel), (d, s), true);
        assert!(same(&doc, &sel).is_ok());

        let mut other = doc.clone();
        let id = other.elements.ids()[0];
        other
            .elements
            .edit(id, &mut other.journal)
            .unwrap()
            .base
            .hidden = true;
        assert!(same(&other, &sel).is_err(), "element");

        let mut other = doc.clone();
        let id = other.meshes.ids()[0];
        other.meshes.edit(id, &mut other.journal).unwrap().tint = [0.0; 4];
        assert!(same(&other, &sel).is_err(), "mesh");

        let mut other = doc.clone();
        other
            .voxels
            .as_mut()
            .unwrap()
            .write_full(I3::new(2, 2, 2), VOXEL[1]);
        assert!(same(&other, &sel).is_err(), "voxel");

        let mut other = doc.clone();
        other.voxels = None;
        assert!(same(&other, &sel).is_err(), "world presence");

        let mut other = doc.clone();
        other.textures.entries[0].texture.image = image(1);
        assert!(same(&other, &sel).is_err(), "texture image");

        let mut other_sel = sel.clone();
        other_sel.clear();
        assert!(same(&doc, &other_sel).is_err(), "selection");

        let mut other = doc.clone();
        Arc::make_mut(&mut other.props).general.name = "x".to_owned();
        assert!(same(&other, &sel).is_err(), "properties");

        let mut other = doc.clone();
        other.elements.clear();
        assert!(same(&other, &sel).is_err(), "count");
    }

    #[test]
    fn the_twin_refuses_open_and_merge() {
        let (mut doc, mut sel) = sample();
        let env = env_for(&doc);
        let mut rng = EngineRng::seeded(1);
        let snap = capture(
            &doc,
            &sel,
            CaptureScope {
                categories: Categories::ALL,
                selection_only: false,
            },
        );
        let mut cx = EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        };
        assert!(apply_reference(&mut cx, &snap, ApplyMode::Open).is_err());
        assert!(apply_reference(&mut cx, &snap, ApplyMode::Merge).is_err());
    }

    #[test]
    fn a_plain_undo_round_trip_equals_the_twin() {
        let (doc, sel) = sample();
        let plan_rounds = [Round {
            capture: 127,
            selection_only: false,
            ops: vec![
                Op::DropWorld,
                Op::ClearTextures,
                Op::FlipRestrict,
                Op::HideElement(0),
            ],
            restore: 127,
            paste: None,
            tweak: Tweak::Nothing,
        }];
        let _ = (doc, sel);
        let plan = Plan {
            props: ModelProps::default(),
            view: ViewProps::default(),
            display: DisplaySlots::default(),
            blueprint: Blueprint::default(),
            elements: vec![],
            meshes: vec![],
            textures: vec![],
            model_box: None,
            voxel: None,
        };
        run(&plan, &plan_rounds).unwrap();
    }
}
