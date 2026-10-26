//! Document invariant checker for debug builds, property tests and fuzzing (spec: tools 16.2; arch
//! 8.7).

use cuboid_voxel::{ChunkSlot, VoxelWorld};
use glam::{Vec2, Vec3};
use rustc_hash::FxHashSet;
use thiserror::Error;

use crate::contract::Document;
use crate::ids::{ObjId, ObjRef};
use crate::list::ObjList;
use crate::objects::{MIN_SIZE, ObjectBase};
use crate::selection::Selection;

/// A broken invariant of the document, one variant per invariant of architecture 8.7.
///
/// spec: arch 8.7, tools 16.2
#[derive(Debug, Error)]
pub enum InvariantError {
    /// A size component produced by the size setter is below 0.001 in magnitude (editor scope
    /// only).
    #[error("size of {0:?} has a component below 0.001")]
    SizeBelowMinimum(ObjRef),
    /// A stored rotation is off its 5 or 22.5 degree grid while rotations are restricted, or not
    /// finite.
    #[error("rotation of {0:?} is off its grid")]
    RotationOffGrid(ObjRef),
    /// An element does not have six faces. The face array makes this unreachable in the model;
    /// a decoder reports it as an apply error before a document exists.
    #[error("element {0:?} does not have six faces")]
    FaceCount(ObjId),
    /// A UV rotation is not a quarter turn: 0, 90, 180 or 270, or the negative twin of a stored
    /// value below -405 (tools 4.5).
    #[error("UV rotation of element {0:?} is not a quarter turn")]
    UvRotation(ObjId),
    /// A raw UV corner is outside `[-1, 2]` (editor scope only).
    #[error("raw UV corner of element {0:?} is outside [-1, 2]")]
    UvCornerRange(ObjId),
    /// The active-face count differs from the face mask. The mask is derived from the face
    /// flags, so the model cannot violate this; the variant is kept for the contract of
    /// architecture 8.7.
    #[error("active faces of element {0:?} disagree with the face mask")]
    ActiveFaces(ObjId),
    /// A display id is zero, repeated or newer than the list's counter, so the ids do not
    /// follow creation order. Reordering keeps the ids with their items, so list order is not
    /// the order that is checked.
    #[error("display ids of the {0} list are not strictly increasing")]
    DisplayIdOrder(&'static str),
    /// The voxel palette does not start with 0, or (editor scope) holds a repeated value.
    #[error("voxel palette is malformed")]
    Palette,
    /// A chunk's non-zero count or occupancy bits disagree with its cells.
    #[error("voxel chunk {0} disagrees with its counters")]
    ChunkCounters(usize),
    /// The active box is not half-open and inside the grid.
    #[error("voxel active box is outside the grid")]
    ActiveBox,
    /// The selection refers to an object that is not in the document.
    #[error("selection refers to the missing object {0:?}")]
    DanglingSelection(ObjRef),
    /// The group tree is malformed: a node in two lists, a missing group or object, a cycle,
    /// more than 64 levels or more than 65 536 groups.
    #[error("the group tree is malformed: {0}")]
    GroupTree(String),
}

/// Which documents a check is for. Loads and imports store some values as they are in the
/// file, so the rules that only the editor's own writes keep do not apply to them.
#[derive(Copy, Clone, Eq, PartialEq)]
enum Scope {
    /// A document built through the setters and commands.
    Editor,
    /// A document a legal load or import can produce.
    Loaded,
}

/// Checks the invariants of architecture 8.7 for a document that was built through the setters
/// and commands. Run after every command in debug builds, after every property-test step and by
/// the fuzz harness. A document straight from a load or an import can break the size, raw UV
/// corner and palette rules legally; check those with [`check_loaded_invariants`].
///
/// spec: arch 8.7, tools 16.2
pub fn check_invariants(doc: &Document, sel: &Selection) -> Result<(), InvariantError> {
    check(doc, sel, Scope::Editor)
}

/// The invariants that hold for every document, including the result of a legal load or
/// import: it skips the rules that only the editor's own writes keep. Cube sizes below 0.001 and
/// mesh scales are stored as the file has them (native 6.3), the `.cub` and Minecraft JSON
/// importers keep UV corners outside `[-1, 2]` (native 4.4.3, 9.5), and import mode can force a
/// duplicate into the palette (tools 7.4).
///
/// spec: arch 8.7, arch 8.3, native 6.3
pub fn check_loaded_invariants(doc: &Document, sel: &Selection) -> Result<(), InvariantError> {
    check(doc, sel, Scope::Loaded)
}

fn check(doc: &Document, sel: &Selection, scope: Scope) -> Result<(), InvariantError> {
    let grid = doc.props.general.restrict_rotations;
    check_base(&doc.blueprint.base, ObjRef::Blueprint, true, grid, scope)?;
    for (&id, el) in doc.elements.ids().iter().zip(doc.elements.items()) {
        let obj = ObjRef::Element(id);
        check_base(&el.base, obj, true, grid, scope)?;
        for face in &el.faces {
            let rot = face.uv.rot();
            if rot % 90 != 0 || !(-360..360).contains(&rot) {
                return Err(InvariantError::UvRotation(id));
            }
            let in_range =
                |c: Vec2| c.cmpge(Vec2::splat(-1.0)).all() && c.cmple(Vec2::splat(2.0)).all();
            if scope == Scope::Editor && !(in_range(face.uv.tl_raw()) && in_range(face.uv.br_raw()))
            {
                return Err(InvariantError::UvCornerRange(id));
            }
        }
    }
    for (&id, mesh) in doc.meshes.ids().iter().zip(doc.meshes.items()) {
        check_base(&mesh.base, ObjRef::Mesh(id), false, grid, scope)?;
    }
    check_display_ids(&doc.elements, "element")?;
    check_display_ids(&doc.meshes, "mesh")?;
    if let Some(world) = &doc.voxels {
        check_voxels(world, scope)?;
    }
    doc.check_groups().map_err(InvariantError::GroupTree)?;
    for &obj in sel.list.iter().chain(sel.primary.iter()) {
        let live = match obj {
            ObjRef::Element(id) => doc.elements.get(id).is_some(),
            ObjRef::Mesh(id) => doc.meshes.get(id).is_some(),
            ObjRef::Group(id) => doc.groups.get(id).is_some(),
            ObjRef::Blueprint | ObjRef::ModelOrigin => true,
        };
        if !live {
            return Err(InvariantError::DanglingSelection(obj));
        }
    }
    Ok(())
}

/// The size and the rotation of one object. A size component is at least `MIN_SIZE` in
/// magnitude; its sign is free, because an inverted cube is a legal model. While "Restrict
/// rotations" is on (`grid`), elements and the blueprint are on the 22.5 degree grid of restricted
/// mode or the 5 degree grid and meshes are always on the 5 degree grid; while it is off,
/// rotations are free. A component lies in `[-360, 360]`: 360 results from rounding, and negative
/// values from inputs below -360 (tools 3.4).
fn check_base(
    base: &ObjectBase,
    obj: ObjRef,
    restrictable: bool,
    grid: bool,
    scope: Scope,
) -> Result<(), InvariantError> {
    let sized = base.s.abs().cmpge(Vec3::splat(MIN_SIZE)).all();
    if scope == Scope::Editor && !sized {
        return Err(InvariantError::SizeBelowMinimum(obj));
    }
    let on_grid = |c: f32, step: f32| (c / step).fract() == 0.0;
    let ok = base.e.to_array().into_iter().all(|c| {
        c.abs() <= 360.0 && (!grid || on_grid(c, 5.0) || (restrictable && on_grid(c, 22.5)))
    });
    if ok {
        Ok(())
    } else {
        Err(InvariantError::RotationOffGrid(obj))
    }
}

/// Display ids are issued from a counter, so distinct ids in `1..=counter` are exactly ids that
/// increase in creation order.
pub(crate) fn check_display_ids<T: Clone>(
    list: &ObjList<T>,
    kind: &'static str,
) -> Result<(), InvariantError> {
    let mut seen = FxHashSet::default();
    let ok = list
        .display_ids()
        .iter()
        .all(|&d| (1..=list.issued_display_ids()).contains(&d) && seen.insert(d));
    if ok {
        Ok(())
    } else {
        Err(InvariantError::DisplayIdOrder(kind))
    }
}

fn check_voxels(world: &VoxelWorld, scope: Scope) -> Result<(), InvariantError> {
    let values = world.palette.values();
    let distinct: FxHashSet<u32> = values.iter().copied().collect();
    if values.first() != Some(&0) || (scope == Scope::Editor && distinct.len() != values.len()) {
        return Err(InvariantError::Palette);
    }
    let grid = i32::try_from(3 * world.res).unwrap_or(i32::MAX);
    let b = world.active_box;
    if b.min.min_element() < 0 || b.max.max_element() > grid || b.min.cmpgt(b.max).any() {
        return Err(InvariantError::ActiveBox);
    }
    for (slot, chunk) in world.slots().iter().enumerate() {
        let ChunkSlot::Data(chunk) = chunk else {
            continue;
        };
        let counted = chunk.cells().iter().filter(|&&cell| cell != 0).count();
        let agrees = usize::from(chunk.nonzero()) == counted
            && chunk
                .cells()
                .iter()
                .enumerate()
                .all(|(local, &cell)| chunk.occupied(local) == (cell != 0));
        if !agrees {
            return Err(InvariantError::ChunkCounters(slot));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cuboid_voxel::{I3, IBox3, VoxelWorld, mapping};
    use glam::{Vec2, Vec3};

    use super::*;
    use crate::selection::Selection;
    use crate::setters::fixture::World;
    use crate::setters::{ElementMut, RotOpts};
    use crate::uv::UvRect;

    fn check(w: &World) -> Result<(), InvariantError> {
        check_invariants(&w.doc, &w.sel)
    }

    #[test]
    fn a_fresh_document_satisfies_the_invariants() {
        let w = World::new();
        assert!(check(&w).is_ok());
    }

    #[test]
    fn a_document_edited_through_the_setters_satisfies_the_invariants() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let b = w.add_element(Vec3::new(3.0, 0.0, 0.0), Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.select(&[
            ObjRef::Element(a),
            ObjRef::Element(b),
            ObjRef::Mesh(m),
            ObjRef::Blueprint,
            ObjRef::ModelOrigin,
        ]);
        let opts = RotOpts {
            propagate: true,
            force: false,
            about_pivot: true,
        };
        {
            let mut cx = w.cx();
            let mut el = ElementMut::new(&mut cx, a).unwrap();
            el.set_size(Vec3::new(0.0, 5.0, 2.0));
            el.set_rotation(Vec3::new(0.0, 100.0, 0.0), opts);
            el.bake_restricted_rotation();
            el.quarter_turn(crate::setters::Axis::Z, 270);
        }
        w.doc.elements.stable_partition(|_, _, _| false);
        assert!(check(&w).is_ok());
    }

    #[test]
    fn a_size_below_the_minimum_is_reported_with_its_object() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| el.base.s.y = 0.0005);
        assert!(matches!(
            check(&w),
            Err(InvariantError::SizeBelowMinimum(ObjRef::Element(id))) if id == a
        ));
        w.edit_element(a, |el| el.base.s.y = f32::NAN);
        assert!(check(&w).is_err());

        let mut w = World::new();
        w.doc.blueprint.base.s.x = 0.0;
        assert!(matches!(
            check(&w),
            Err(InvariantError::SizeBelowMinimum(ObjRef::Blueprint))
        ));
    }

    #[test]
    fn rotations_must_sit_on_the_five_or_the_22_5_degree_grid() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        for ok in [
            Vec3::new(0.0, 22.5, 360.0),
            Vec3::new(5.0, 337.5, 0.0),
            Vec3::new(-40.0, 0.0, 0.0),
        ] {
            w.edit_element(a, |el| el.base.e = ok);
            assert!(check(&w).is_ok(), "{ok}");
        }
        for bad in [Vec3::new(7.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 400.0)] {
            w.edit_element(a, |el| el.base.e = bad);
            assert!(matches!(
                check(&w),
                Err(InvariantError::RotationOffGrid(ObjRef::Element(_)))
            ));
        }
        w.edit_element(a, |el| el.base.e = Vec3::ZERO);

        // A mesh never uses the 22.5 degree grid.
        let doc = &mut w.doc;
        doc.meshes.edit(m, &mut doc.journal).unwrap().base.e = Vec3::new(22.5, 0.0, 0.0);
        assert!(matches!(
            check(&w),
            Err(InvariantError::RotationOffGrid(ObjRef::Mesh(_)))
        ));
    }

    #[test]
    fn a_model_that_does_not_restrict_rotations_has_free_rotations_within_a_circle() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        std::sync::Arc::make_mut(&mut w.doc.props)
            .general
            .restrict_rotations = false;
        let doc = &mut w.doc;
        doc.meshes.edit(m, &mut doc.journal).unwrap().base.e = Vec3::new(22.5, 53.0, 0.1);
        w.edit_element(a, |el| el.base.e = Vec3::new(7.0, 53.5, -12.345));
        assert!(check(&w).is_ok());
        assert!(check_loaded_invariants(&w.doc, &w.sel).is_ok());
        w.edit_element(a, |el| el.base.e = Vec3::new(0.0, 400.0, 0.0));
        assert!(check(&w).is_err());
        w.edit_element(a, |el| el.base.e = Vec3::new(0.0, f32::NAN, 0.0));
        assert!(check(&w).is_err());
    }

    #[test]
    fn a_size_may_be_negative_but_not_closer_to_zero_than_the_minimum() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| el.base.s = Vec3::new(-2.0, 1.0, -0.001));
        assert!(check(&w).is_ok());
        w.edit_element(a, |el| el.base.s = Vec3::new(-2.0, 1.0, -0.0005));
        assert!(matches!(
            check(&w),
            Err(InvariantError::SizeBelowMinimum(ObjRef::Element(_)))
        ));
        assert!(check_loaded_invariants(&w.doc, &w.sel).is_ok());
    }

    #[test]
    fn uv_rotations_and_raw_corners_are_checked() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        // A stored rotation below -405 keeps a negative quarter turn (tools 4.5).
        w.edit_element(a, |el| {
            el.faces[0].uv = UvRect::new(-450, Vec2::ZERO, Vec2::ONE)
        });
        assert!(check(&w).is_ok());
        w.edit_element(a, |el| {
            let uv = &mut el.faces[3].uv;
            uv.set_unclamped(Vec2::new(-1.5, 0.0), Vec2::ONE);
        });
        assert!(matches!(
            check(&w),
            Err(InvariantError::UvCornerRange(id)) if id == a
        ));
    }

    #[test]
    fn a_legal_load_or_import_passes_the_loaded_check_but_not_the_editor_check() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(a), ObjRef::Mesh(m)]);
        let loaded = |w: &World| check_loaded_invariants(&w.doc, &w.sel);

        // Native 6.3: a cube keeps a stored size below 0.001, a mesh its stored scale.
        w.edit_element(a, |el| el.base.s = Vec3::new(0.0005, 1.0, 1.0));
        let doc = &mut w.doc;
        doc.meshes.edit(m, &mut doc.journal).unwrap().base.s = Vec3::new(1.0, 0.0, 1.0);
        assert!(matches!(
            check(&w),
            Err(InvariantError::SizeBelowMinimum(ObjRef::Element(_)))
        ));
        assert!(loaded(&w).is_ok());

        // Native 4.4.3 and 9.5: the importers keep corners outside [-1, 2].
        w.edit_element(a, |el| {
            el.base.s = Vec3::ONE;
            el.faces[2]
                .uv
                .set_unclamped(Vec2::new(-3.0, 0.0), Vec2::new(2.5, 1.0));
        });
        let doc = &mut w.doc;
        doc.meshes.edit(m, &mut doc.journal).unwrap().base.s = Vec3::ONE;
        assert!(matches!(check(&w), Err(InvariantError::UvCornerRange(_))));
        assert!(loaded(&w).is_ok());

        // The rules of every document still hold.
        w.edit_element(a, |el| el.faces[2].uv = UvRect::default());
        w.edit_element(a, |el| el.base.e = Vec3::new(7.0, 0.0, 0.0));
        assert!(matches!(
            loaded(&w),
            Err(InvariantError::RotationOffGrid(_))
        ));
        w.edit_element(a, |el| el.base.e = Vec3::ZERO);
        w.edit_element(a, |el| {
            el.faces[0].uv = UvRect::new(-450, Vec2::ZERO, Vec2::ONE)
        });
        assert!(loaded(&w).is_ok());
        w.sel.add(ObjRef::Element(ObjId::next()));
        assert!(matches!(
            loaded(&w),
            Err(InvariantError::DanglingSelection(_))
        ));
    }

    #[test]
    fn a_forced_palette_duplicate_passes_only_the_loaded_check() {
        let mut w = World::new();
        let res = 4;
        let mut world = VoxelWorld::new(res, true, mapping::grid_box(res));
        world.write_full(I3::new(1, 2, 3), 0xFF11_2233);
        world.palette.push_forced(0xFF11_2233);
        w.doc.voxels = Some(world);
        assert!(matches!(check(&w), Err(InvariantError::Palette)));
        assert!(check_loaded_invariants(&w.doc, &w.sel).is_ok());

        let mut world = VoxelWorld::new(res, true, mapping::grid_box(res));
        world.active_box = IBox3 {
            min: I3::ZERO,
            max: I3::splat(13),
        };
        w.doc.voxels = Some(world);
        assert!(matches!(
            check_loaded_invariants(&w.doc, &w.sel),
            Err(InvariantError::ActiveBox)
        ));
    }

    #[test]
    fn the_voxel_palette_and_the_active_box_are_checked() {
        let mut w = World::new();
        let res = 4;
        let mut world = VoxelWorld::new(res, true, mapping::grid_box(res));
        world.write_full(I3::new(1, 2, 3), 0xFF11_2233);
        world.write_full(I3::new(9, 9, 9), 0xFF44_5566);
        w.doc.voxels = Some(world);
        assert!(check(&w).is_ok());

        let world = w.doc.voxels.as_mut().unwrap();
        world.palette.push_forced(0xFF11_2233);
        assert!(matches!(check(&w), Err(InvariantError::Palette)));

        let mut world = VoxelWorld::new(res, true, mapping::grid_box(res));
        world.active_box = IBox3 {
            min: I3::ZERO,
            max: I3::splat(13),
        };
        w.doc.voxels = Some(world);
        assert!(matches!(check(&w), Err(InvariantError::ActiveBox)));
    }

    #[test]
    fn the_group_tree_and_the_selected_groups_are_checked() {
        use crate::groups::{Group, GroupsBuilder};

        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let mut b = GroupsBuilder::new();
        let g = b.add_group(Group::new("g", Vec3::ZERO), None).unwrap();
        b.add_object(ObjRef::Element(a), Some(g)).unwrap();
        w.doc.groups = b.finish();
        w.select(&[ObjRef::Group(g)]);
        assert!(check(&w).is_ok());

        // An object that was deleted behind the tree's back.
        w.doc
            .elements
            .remove_set(&rustc_hash::FxHashSet::from_iter([a]));
        assert!(matches!(check(&w), Err(InvariantError::GroupTree(_))));
        w.doc.prune_groups();
        assert!(check(&w).is_ok());

        // A selected group that is gone.
        w.sel.add(ObjRef::Group(ObjId::next()));
        assert!(matches!(
            check(&w),
            Err(InvariantError::DanglingSelection(ObjRef::Group(_)))
        ));
    }

    #[test]
    fn the_selection_may_only_name_live_objects() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(a), ObjRef::ModelOrigin]);
        assert!(check(&w).is_ok());

        w.doc
            .elements
            .remove_set(&rustc_hash::FxHashSet::from_iter([a]));
        assert!(matches!(
            check(&w),
            Err(InvariantError::DanglingSelection(ObjRef::Element(id))) if id == a
        ));

        let sel = Selection {
            primary: Some(ObjRef::Mesh(ObjId::next())),
            ..Selection::default()
        };
        assert!(matches!(
            check_invariants(&w.doc, &sel),
            Err(InvariantError::DanglingSelection(ObjRef::Mesh(_)))
        ));
    }
}
