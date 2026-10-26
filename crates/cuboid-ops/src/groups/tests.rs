use cuboid_model::{
    Document, EditEnv, ElementType, GroupsBuilder, RenderPrefs, RotResult, Selection, TextureList,
    check_invariants,
};
use cuboid_num::{EngineRng, euler_to_quat};
use glam::Vec3;

use super::*;
use crate::transform;

struct Rig {
    doc: Document,
    sel: Selection,
    env: EditEnv,
    rng: EngineRng,
}

impl Rig {
    fn new() -> Self {
        Rig {
            doc: Document::new(TextureList::default()),
            sel: Selection::default(),
            env: EditEnv {
                restrict_rotations: true,
                render: RenderPrefs::default(),
                code_page: 1252,
                allow_negative_size: false,
            },
            rng: EngineRng::seeded(5),
        }
    }

    fn cx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            sel: &mut self.sel,
            env: &self.env,
            rng: &mut self.rng,
        }
    }

    /// A cube of size 2 at x, 0, 0 with its pivot at its position.
    fn cube(&mut self, x: f32) -> ObjRef {
        let id = crate::primitives::add_element(&mut self.cx(), ElementType::Cube);
        let doc = &mut self.doc;
        let el = doc
            .elements
            .edit(id, &mut doc.journal)
            .expect("the element");
        el.base.p = Vec3::new(x, 0.0, 0.0);
        el.base.o = el.base.p;
        el.base.s = Vec3::splat(2.0);
        ObjRef::Element(id)
    }

    fn base(&self, obj: ObjRef) -> cuboid_model::ObjectBase {
        transform::base_of(&self.doc, obj)
            .expect("an object")
            .clone()
    }

    fn valid(&self) {
        check_invariants(&self.doc, &self.sel).expect("a valid document");
    }
}

/// Two cubes in group A (pivot at the origin), A inside nothing; B holds the second cube.
fn nested(rig: &mut Rig) -> (GroupId, GroupId, [ObjRef; 3]) {
    let cubes = [rig.cube(0.0), rig.cube(4.0), rig.cube(8.0)];
    let mut b = GroupsBuilder::new();
    let a = b.add_group(Group::new("A", Vec3::ZERO), None).unwrap();
    b.add_object(cubes[0], Some(a)).unwrap();
    let inner = b
        .add_group(Group::new("B", Vec3::new(4.0, 0.0, 0.0)), Some(a))
        .unwrap();
    b.add_object(cubes[1], Some(inner)).unwrap();
    rig.doc.groups = b.finish();
    rig.valid();
    (a, inner, cubes)
}

#[test]
fn a_new_group_has_its_pivot_at_the_centre_of_the_bounds() {
    let mut rig = Rig::new();
    let cubes = [rig.cube(0.0), rig.cube(10.0)];
    let id = group(&mut rig.cx(), &cubes, "Pair").unwrap();
    let made = rig.doc.groups.get(id).unwrap();
    // The boxes span x from 0 to 12 and have their corner at z = 0 reaching to z = -2.
    assert_eq!(made.origin, Vec3::new(6.0, 1.0, -1.0));
    assert_eq!(&*made.name, "Pair");
    assert_eq!(made.children(), cubes);
    rig.valid();
    // Nothing to group.
    assert!(group(&mut rig.cx(), &[], "none").is_err());
}

#[test]
fn moving_a_group_moves_everything_below_it_and_the_pivots_of_the_groups_inside() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    let delta = Vec3::new(1.5, 0.0, -2.0);
    let moved = Pose::capture(&rig.doc, &[ObjRef::Group(a)]).translate(&mut rig.cx(), delta);
    assert_eq!(moved, 4, "two cubes and two groups");
    assert_eq!(rig.base(cubes[0]).p, Vec3::new(1.5, 0.0, -2.0));
    assert_eq!(rig.base(cubes[1]).p, Vec3::new(5.5, 0.0, -2.0));
    assert_eq!(rig.base(cubes[1]).o, Vec3::new(5.5, 0.0, -2.0));
    assert_eq!(
        rig.base(cubes[2]).p,
        Vec3::new(8.0, 0.0, 0.0),
        "outside the group"
    );
    assert_eq!(rig.doc.groups.get(a).unwrap().origin, delta);
    assert_eq!(
        rig.doc.groups.get(inner).unwrap().origin,
        Vec3::new(5.5, 0.0, -2.0)
    );
    rig.valid();
}

#[test]
fn turning_a_group_swings_its_objects_about_the_pivot_and_keeps_the_local_value() {
    let mut rig = Rig::new();
    rig.env.restrict_rotations = false;
    let (a, inner, cubes) = nested(&mut rig);
    let result = transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(0.0, 90.0, 0.0),
        cuboid_model::RotOpts {
            propagate: false,
            force: false,
            about_pivot: true,
        },
    );
    assert_eq!(result, RotResult::Applied);
    let turn = euler_to_quat(Vec3::new(0.0, 90.0, 0.0));
    let swung = turn * Vec3::new(4.0, 0.0, 0.0);
    let second = rig.base(cubes[1]);
    assert!(
        (second.p - swung).length() < 1e-3,
        "{:?} vs {swung:?}",
        second.p
    );
    assert!((second.e - Vec3::new(0.0, 90.0, 0.0)).length() < 1e-3);
    assert_eq!(
        rig.doc.groups.get(a).unwrap().rotation,
        Vec3::new(0.0, 90.0, 0.0)
    );
    // The group inside turned with it: its pivot swung, its local rotation did not change.
    let b = rig.doc.groups.get(inner).unwrap();
    assert!((b.origin - swung).length() < 1e-3);
    assert_eq!(b.rotation, Vec3::ZERO);
    assert_eq!(
        rig.base(cubes[2]).p,
        Vec3::new(8.0, 0.0, 0.0),
        "outside the group"
    );
    rig.valid();

    // The same rotation again changes nothing.
    let rev = rig.doc.revision;
    let again = transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(0.0, 90.0, 0.0),
        cuboid_model::RotOpts {
            propagate: false,
            force: false,
            about_pivot: true,
        },
    );
    assert_eq!(again, RotResult::Unchanged);
    assert_eq!(rig.doc.revision, rev);
}

#[test]
fn a_group_inside_a_turned_group_turns_by_the_world_angle() {
    let mut rig = Rig::new();
    rig.env.restrict_rotations = false;
    let (a, inner, cubes) = nested(&mut rig);
    let opts = cuboid_model::RotOpts {
        propagate: false,
        force: false,
        about_pivot: true,
    };
    transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(0.0, 90.0, 0.0),
        opts,
    );
    // Turn B by its local value: its cube follows by the same world angle, whatever A did.
    let before = rig.base(cubes[1]);
    transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(inner),
        Vec3::new(0.0, 90.0, 0.0),
        opts,
    );
    let after = rig.base(cubes[1]);
    let delta = euler_to_quat(after.e) * euler_to_quat(before.e).inverse();
    let expected = euler_to_quat(Vec3::new(0.0, 90.0, 0.0));
    assert!(delta.abs_diff_eq(expected, 1e-3) || delta.abs_diff_eq(-expected, 1e-3));
    // The pivot of B did not move, so the cube turned about it.
    let pivot = rig.doc.groups.get(inner).unwrap().origin;
    let before_off = (before.p - pivot).length();
    let after_off = (after.p - pivot).length();
    assert!((before_off - after_off).abs() < 1e-3);
    rig.valid();
}

#[test]
fn restricted_rotation_refuses_a_turn_that_leaves_the_grid() {
    let mut rig = Rig::new();
    let (a, _, cubes) = nested(&mut rig);
    let before = rig.base(cubes[1]);
    let opts = cuboid_model::RotOpts {
        propagate: false,
        force: false,
        about_pivot: true,
    };
    let turned = transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(0.0, 22.5, 0.0),
        opts,
    );
    assert_eq!(turned, RotResult::Applied, "22.5 degrees is on the grid");
    let after = rig.base(cubes[1]);
    assert_ne!(after.p, before.p);
    rig.valid();
    // A second axis on an element that already turns about Y is not.
    let refused = transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(22.5, 22.5, 0.0),
        opts,
    );
    assert_eq!(refused, RotResult::Rejected);
    assert_eq!(rig.base(cubes[1]), after, "nothing changed");
    assert_eq!(
        rig.doc.groups.get(a).unwrap().rotation,
        Vec3::new(0.0, 22.5, 0.0)
    );
    // A value off the grid snaps to it, as an element's does: here back onto the rotation it has.
    let off = transform::set_rotation(
        &mut rig.cx(),
        ObjRef::Group(a),
        Vec3::new(0.0, 30.0, 0.0),
        opts,
    );
    assert_eq!(off, RotResult::Unchanged);
    assert_eq!(
        rig.doc.groups.get(a).unwrap().rotation,
        Vec3::new(0.0, 22.5, 0.0)
    );
    rig.valid();
}

#[test]
fn scaling_a_group_scales_sizes_positions_and_pivots_about_the_pivot() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    let changed = Pose::capture(&rig.doc, &[ObjRef::Group(a)]).scale(
        &mut rig.cx(),
        Vec3::ZERO,
        Vec3::splat(2.0),
        None,
    );
    assert!(changed > 0);
    assert_eq!(rig.base(cubes[1]).p, Vec3::new(8.0, 0.0, 0.0));
    assert_eq!(rig.base(cubes[1]).s, Vec3::splat(4.0));
    assert_eq!(
        rig.doc.groups.get(inner).unwrap().origin,
        Vec3::new(8.0, 0.0, 0.0)
    );
    assert_eq!(rig.base(cubes[2]).s, Vec3::splat(2.0), "outside the group");
    rig.valid();
}

#[test]
fn hiding_and_locking_a_group_marks_everything_below_it() {
    let mut rig = Rig::new();
    let (a, _, cubes) = nested(&mut rig);
    rig.doc.journal.clear();
    let fields = GroupFields {
        visible: Some(false),
        locked: Some(true),
        ..GroupFields::default()
    };
    assert!(set_fields(&mut rig.cx(), a, &fields));
    assert!(!set_fields(&mut rig.cx(), a, &fields), "the same again");
    let ObjRef::Element(first) = cubes[0] else {
        panic!("a cube");
    };
    assert!(rig.doc.journal.objects().contains_key(&first));
    assert!(rig.doc.is_hidden(cubes[1]) && rig.doc.is_locked(cubes[1]));
    assert!(!rig.doc.is_hidden(cubes[2]));
    // Names lose their quotes; an empty name keeps the old one.
    let rename = GroupFields {
        name: Some("a\"b".to_owned()),
        ..GroupFields::default()
    };
    assert!(set_fields(&mut rig.cx(), a, &rename));
    assert_eq!(&*rig.doc.groups.get(a).unwrap().name, "ab");
    let blank = GroupFields {
        name: Some("   ".to_owned()),
        ..GroupFields::default()
    };
    assert!(!set_fields(&mut rig.cx(), a, &blank));
    rig.valid();
}

#[test]
fn moving_the_pivot_leaves_the_objects_alone() {
    let mut rig = Rig::new();
    let (a, _, cubes) = nested(&mut rig);
    let before = rig.base(cubes[0]);
    assert!(set_origin(&mut rig.cx(), a, Vec3::new(3.0, 3.0, 3.0)));
    assert_eq!(
        rig.doc.groups.get(a).unwrap().origin,
        Vec3::new(3.0, 3.0, 3.0)
    );
    assert_eq!(rig.base(cubes[0]), before);
}

#[test]
fn deleting_a_group_removes_its_subtree_and_nothing_else() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    rig.sel.add(ObjRef::Group(inner));
    rig.sel.add(cubes[0]);
    // The group and a cube inside it: the group takes the cube along, once.
    let removed = delete(&mut rig.cx(), &[ObjRef::Group(a), cubes[0]]);
    assert_eq!(removed, 4, "two groups and two cubes");
    assert_eq!(rig.doc.elements.len(), 1);
    assert!(rig.doc.groups.is_empty());
    assert!(!rig.sel.contains(ObjRef::Group(inner)) && !rig.sel.contains(cubes[0]));
    assert!(
        rig.sel.contains(cubes[2]),
        "the cube outside stays selected"
    );
    rig.valid();
}

#[test]
fn deleting_an_object_in_a_group_leaves_the_group() {
    let mut rig = Rig::new();
    let (a, _, cubes) = nested(&mut rig);
    assert_eq!(delete(&mut rig.cx(), &[cubes[0]]), 1);
    assert_eq!(rig.doc.groups.get(a).unwrap().children().len(), 1);
    rig.valid();
}

#[test]
fn a_duplicate_of_a_group_is_a_new_subtree_behind_the_original() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    rig.doc
        .groups
        .edit(a, ChangeFlags::all(), &mut rig.doc.journal)
        .unwrap()
        .uuid = Some("keep-me".to_owned());
    let copies = duplicate(&mut rig.cx(), &[ObjRef::Group(a)]).unwrap();
    assert_eq!(copies.created.len(), 2, "the two cubes of the subtree");
    assert_eq!(rig.doc.groups.len(), 4);
    assert_eq!(rig.doc.elements.len(), 5);
    let [ObjRef::Group(copy)] = copies.chosen.as_slice() else {
        panic!("one copy of the group");
    };
    assert_ne!(*copy, a);
    assert_eq!(
        rig.doc.root_nodes()[..2],
        [ObjRef::Group(a), ObjRef::Group(*copy)]
    );
    let made = rig.doc.groups.get(*copy).unwrap();
    assert_eq!(&*made.name, "A");
    assert!(
        made.uuid.is_none(),
        "a copy gets a new uuid when it is written"
    );
    // The inner group of the copy holds a copy of the second cube.
    let inner_copy = made
        .children()
        .iter()
        .find_map(|c| match c {
            ObjRef::Group(g) => Some(*g),
            _ => None,
        })
        .unwrap();
    assert_ne!(inner_copy, inner);
    let held = rig.doc.groups.get(inner_copy).unwrap().children().to_vec();
    assert_eq!(held.len(), 1);
    assert_ne!(held[0], cubes[1]);
    assert_eq!(rig.base(held[0]).p, rig.base(cubes[1]).p);
    // The copies are selected, nothing else.
    assert!(rig.sel.contains(ObjRef::Group(*copy)));
    assert_eq!(rig.sel.list.len(), 1);
    rig.valid();
}

#[test]
fn a_duplicate_of_an_object_in_a_group_stays_in_the_group() {
    let mut rig = Rig::new();
    let (a, _, cubes) = nested(&mut rig);
    let copies = duplicate(&mut rig.cx(), &[cubes[0]]).unwrap();
    let [copy] = copies.chosen.as_slice() else {
        panic!("one copy");
    };
    assert_eq!(rig.doc.groups.parent_of(*copy), Some(a));
    assert_eq!(
        rig.doc.child_nodes(Some(a))[1],
        *copy,
        "right behind the original"
    );
    rig.valid();
}

#[test]
fn ungrouping_and_regrouping_restore_the_places() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    let freed = ungroup(&mut rig.cx(), &[ObjRef::Group(a)]);
    assert_eq!(freed, [cubes[0], ObjRef::Group(inner)]);
    assert!(rig.doc.groups.get(a).is_none());
    assert_eq!(rig.doc.groups.parent_of(ObjRef::Group(inner)), None);
    rig.valid();
    let again = group(&mut rig.cx(), &[cubes[0], ObjRef::Group(inner)], "A2").unwrap();
    assert_eq!(
        rig.doc.child_nodes(Some(again)),
        [cubes[0], ObjRef::Group(inner)]
    );
    rig.valid();
}

#[test]
fn reparenting_refuses_a_cycle_and_keeps_the_relative_order() {
    let mut rig = Rig::new();
    let (a, inner, cubes) = nested(&mut rig);
    assert_eq!(
        reparent(&mut rig.cx(), &[ObjRef::Group(a)], Some(inner), None),
        Err(GroupError::Cycle)
    );
    assert_eq!(
        reparent(&mut rig.cx(), &[cubes[2], cubes[1]], Some(a), Some(0)),
        Ok(2)
    );
    assert_eq!(
        rig.doc.child_nodes(Some(a))[..2],
        [cubes[1], cubes[2]],
        "the order of the document, not of the call"
    );
    rig.valid();
}
