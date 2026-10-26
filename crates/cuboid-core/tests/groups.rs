//! The group commands (hierarchy A1 4). One scenario each: execute, undo restores, redo brings it
//! back, the same request again changes nothing and a bad one is refused.

mod common;

use common::{assert_same, el, round_trip, run, scene, select, state};
use cuboid_core::{CmdOutcome, Command, Core, ElementKind, Refusal, Target};
use cuboid_model::{Categories, GroupError, ObjRef};
use glam::Vec3;

/// The undo steps `scene()` leaves behind: three times add, position, size.
const SCENE_STEPS: usize = 9;

fn group_of(core: &mut Core, targets: &[Target], name: &str) -> Target {
    // The group made before may be the selection already.
    core.execute(Command::Select {
        targets: targets.to_vec(),
        op: cuboid_core::SelectOp::Replace,
    });
    let done = run(
        core,
        Command::Group {
            targets: vec![],
            name: name.into(),
        },
    );
    done.created[0]
}

fn element(core: &Core, at: usize) -> ObjRef {
    core.object_of(Target::Element(at)).expect("an element")
}

fn group_ref(core: &Core, at: usize) -> ObjRef {
    core.object_of(Target::Group(at)).expect("a group")
}

#[test]
fn group_makes_a_group_of_the_selection_where_the_first_object_was() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1), Target::Element(0)]);
    let after = round_trip(
        &mut core,
        Command::Group {
            targets: vec![],
            name: "Arm".into(),
        },
    );
    assert!(after.contains("Arm"));
    assert_eq!(core.history().undo_label(), Some("Group"));
    let doc = core.doc();
    assert_eq!(doc.groups.len(), 1);
    let id = doc.groups.ids()[0];
    let group = doc.groups.get(id).unwrap();
    // The cubes span x from 0 to 6 and z from 0 down to -2, y from 0 to 2.
    assert_eq!(group.origin, Vec3::new(3.0, 1.0, -1.0));
    assert_eq!(
        group.children(),
        [element(&core, 0), element(&core, 1)],
        "the order of the outliner, not of the selection"
    );
    assert_eq!(
        doc.root_nodes(),
        [ObjRef::Group(id), element(&core, 2)],
        "the group took the place of the first cube"
    );
    assert_eq!(core.selection().list.len(), 1);
    assert!(core.selection().contains(ObjRef::Group(id)));
}

#[test]
fn group_reports_the_new_group_and_refuses_an_empty_request() {
    let mut core = scene();
    let made = group_of(&mut core, &[Target::Element(2)], "Solo");
    assert_eq!(made, Target::Group(0));
    assert_eq!(
        core.execute(Command::Group {
            targets: vec![Target::Element(9)],
            name: "x".into()
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    select(&mut core, &[]);
    assert_eq!(
        core.execute(Command::Group {
            targets: vec![],
            name: "x".into()
        }),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
    // A group of groups.
    let second = group_of(&mut core, &[Target::Element(0)], "Other");
    assert_eq!(second, Target::Group(1));
    select(&mut core, &[Target::Group(0), Target::Group(1)]);
    run(
        &mut core,
        Command::Group {
            targets: vec![],
            name: "Both".into(),
        },
    );
    assert_eq!(core.doc().groups.len(), 3);
    let outer = core.doc().groups.ids()[2];
    assert_eq!(core.doc().groups.get(outer).unwrap().children().len(), 2);
}

#[test]
fn ungroup_puts_the_children_in_its_place_and_selects_them() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "Arm");
    round_trip(&mut core, Command::Ungroup { targets: vec![] });
    assert!(core.doc().groups.is_empty());
    assert_eq!(
        core.doc().root_nodes(),
        [element(&core, 0), element(&core, 1), element(&core, 2)]
    );
    assert_eq!(core.selection().list.len(), 2, "the freed children");
    assert_eq!(core.history().undo_label(), Some("Ungroup"));
    assert_eq!(
        core.execute(Command::Ungroup { targets: vec![] }),
        CmdOutcome::Refused(Refusal::NoGroups)
    );
    select(&mut core, &[]);
    assert_eq!(
        core.execute(Command::Ungroup { targets: vec![] }),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
}

#[test]
fn reparent_moves_objects_into_a_group_at_a_place_and_back_out() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0)], "A");
    group_of(&mut core, &[Target::Element(1)], "B");
    let into = Command::Reparent {
        targets: vec![Target::Element(2)],
        parent: Some(Target::Group(0)),
        index: Some(0),
    };
    round_trip(&mut core, into.clone());
    let a = core.doc().groups.ids()[0];
    assert_eq!(
        core.doc().child_nodes(Some(a)),
        [element(&core, 2), element(&core, 0)]
    );
    assert_eq!(core.history().undo_label(), Some("Move to group"));
    assert_eq!(core.execute(into), CmdOutcome::Unchanged, "already there");

    round_trip(
        &mut core,
        Command::Reparent {
            targets: vec![Target::Element(2)],
            parent: None,
            index: Some(0),
        },
    );
    assert_eq!(core.doc().root_nodes()[0], element(&core, 2));
    assert_eq!(core.history().undo_label(), Some("Move to top level"));

    // A group goes into another group and keeps its children.
    round_trip(
        &mut core,
        Command::Reparent {
            targets: vec![Target::Group(1)],
            parent: Some(Target::Group(0)),
            index: None,
        },
    );
    let b = core.doc().groups.ids()[1];
    assert_eq!(core.doc().groups.parent_of(ObjRef::Group(b)), Some(a));
    assert_eq!(core.doc().groups.depth(b), 2);
}

#[test]
fn reparent_refuses_a_cycle_and_a_parent_that_is_no_group() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0)], "A");
    group_of(&mut core, &[Target::Group(0)], "B");
    let before = state(&core, true);
    let steps = core.history().undo_len();
    // B holds A: B into B, B into A and A into A are all cycles.
    for (moved, parent) in [(1, 1), (1, 0), (0, 0)] {
        let cmd = Command::Reparent {
            targets: vec![Target::Group(moved)],
            parent: Some(Target::Group(parent)),
            index: None,
        };
        assert_eq!(
            core.execute(cmd),
            CmdOutcome::Refused(Refusal::Group(GroupError::Cycle))
        );
    }
    // A into B is where A is already.
    assert_eq!(
        core.execute(Command::Reparent {
            targets: vec![Target::Group(0)],
            parent: Some(Target::Group(1)),
            index: None,
        }),
        CmdOutcome::Unchanged
    );
    assert_eq!(
        core.execute(Command::Reparent {
            targets: vec![Target::Element(1)],
            parent: Some(Target::Element(2)),
            index: None,
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    assert_eq!(core.history().undo_len(), steps);
    assert_same(&state(&core, true), &before, "a refusal changes nothing");
}

#[test]
fn set_group_changes_the_given_fields_and_moves_only_the_pivot() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    let before = el(&core, 0).base;
    round_trip(
        &mut core,
        Command::SetGroup {
            group: Target::Group(0),
            name: Some("Leg".into()),
            origin: Some([5.0, 6.0, 7.0]),
            visible: Some(false),
            locked: Some(true),
            export: Some(false),
        },
    );
    let id = core.doc().groups.ids()[0];
    let group = core.doc().groups.get(id).unwrap();
    assert_eq!(&*group.name, "Leg");
    assert_eq!(group.origin, Vec3::new(5.0, 6.0, 7.0));
    assert!(!group.visible && group.locked && !group.export);
    assert_eq!(el(&core, 0).base, before, "the objects stay where they are");
    assert_eq!(core.history().undo_label(), Some("Edit group"));
    assert!(core.doc().is_hidden(element(&core, 0)));
    assert!(core.doc().is_locked(element(&core, 1)));
    assert!(!core.doc().is_hidden(element(&core, 2)));

    let again = Command::SetGroup {
        group: Target::Group(0),
        name: Some("Leg".into()),
        origin: Some([5.0, 6.0, 7.0]),
        visible: None,
        locked: None,
        export: None,
    };
    assert_eq!(core.execute(again), CmdOutcome::Unchanged);
    let nan = Command::SetGroup {
        group: Target::Group(0),
        name: None,
        origin: Some([f32::NAN, 0.0, 0.0]),
        visible: None,
        locked: None,
        export: None,
    };
    assert_eq!(core.execute(nan), CmdOutcome::Refused(Refusal::NotFinite));
    let element = Command::SetGroup {
        group: Target::Element(0),
        name: Some("x".into()),
        origin: None,
        visible: None,
        locked: None,
        export: None,
    };
    assert_eq!(
        core.execute(element),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn a_group_hides_and_locks_what_it_holds_through_the_ordinary_commands() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0)], "A");
    round_trip(
        &mut core,
        Command::SetHidden {
            targets: vec![Target::Group(0)],
            hidden: true,
        },
    );
    assert!(core.doc().is_hidden(element(&core, 0)));
    assert!(
        !core.doc().elements.items()[0].base.hidden,
        "the element keeps its own flag"
    );
    round_trip(
        &mut core,
        Command::SetLocked {
            targets: vec![Target::Group(0)],
            locked: true,
        },
    );
    assert!(core.doc().is_locked(element(&core, 0)));
    // Select all skips what a hidden group holds.
    run(&mut core, Command::SelectAll);
    assert!(!core.selection().contains(element(&core, 0)));
    assert!(core.selection().contains(element(&core, 1)));
    // Show all brings the group back.
    select(&mut core, &[]);
    round_trip(&mut core, Command::ShowAll);
    assert!(!core.doc().is_hidden(element(&core, 0)));
    round_trip(&mut core, Command::UnlockAll);
    assert!(!core.doc().is_locked(element(&core, 0)));
}

#[test]
fn rename_names_a_group_as_one_undo_step() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0)], "A");
    round_trip(
        &mut core,
        Command::Rename {
            targets: vec![Target::Group(0)],
            name: "Head".into(),
        },
    );
    let id = core.doc().groups.ids()[0];
    assert_eq!(&*core.doc().groups.get(id).unwrap().name, "Head");
}

#[test]
fn move_carries_the_subtree_and_one_undo_restores_it() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    let other = el(&core, 2).base.p;
    round_trip(
        &mut core,
        Command::MoveBy {
            targets: vec![],
            delta: [1.0, 2.0, 3.0],
            snap: None,
        },
    );
    assert_eq!(el(&core, 0).base.p, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(el(&core, 1).base.p, Vec3::new(5.0, 2.0, 3.0));
    assert_eq!(el(&core, 2).base.p, other);
    let id = core.doc().groups.ids()[0];
    assert_eq!(
        core.doc().groups.get(id).unwrap().origin,
        Vec3::new(4.0, 3.0, 2.0)
    );
    assert_eq!(core.history().undo_label(), Some("Move"));

    // The group and one of its cubes selected together move the cube once.
    select(&mut core, &[Target::Group(0), Target::Element(0)]);
    run(
        &mut core,
        Command::MoveBy {
            targets: vec![],
            delta: [1.0, 0.0, 0.0],
            snap: None,
        },
    );
    assert_eq!(el(&core, 0).base.p.x, 2.0);
}

#[test]
fn set_position_of_a_group_moves_the_pivot_and_the_subtree() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    round_trip(
        &mut core,
        Command::SetPosition {
            targets: vec![Target::Group(0)],
            value: [Some(10.0), None, None],
        },
    );
    let id = core.doc().groups.ids()[0];
    assert_eq!(core.doc().groups.get(id).unwrap().origin.x, 10.0);
    assert_eq!(
        el(&core, 0).base.p.x,
        7.0,
        "it was 3 to the left of the pivot"
    );
}

#[test]
fn rotate_turns_the_subtree_about_the_pivot_and_restricted_mode_refuses_off_grid_turns() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    let pivot = core
        .doc()
        .groups
        .get(core.doc().groups.ids()[0])
        .unwrap()
        .origin;
    round_trip(
        &mut core,
        Command::SetRotation {
            targets: vec![],
            value: [None, Some(90.0), None],
        },
    );
    let turned = el(&core, 1).base;
    assert_eq!(turned.e, Vec3::new(0.0, 90.0, 0.0));
    let id = core.doc().groups.ids()[0];
    assert_eq!(core.doc().groups.get(id).unwrap().rotation.y, 90.0);
    let turn = cuboid_num::euler_to_quat(Vec3::new(0.0, 90.0, 0.0));
    let expected = pivot + turn * (Vec3::new(4.0, 0.0, 0.0) - pivot);
    assert!((turned.p - expected).length() < 1e-3);
    assert_eq!(core.history().undo_label(), Some("Set rotation"));

    // A second axis is not allowed while rotations are restricted.
    let before = state(&core, true);
    let outcome = core.execute(Command::SetRotation {
        targets: vec![],
        value: [Some(90.0), None, None],
    });
    assert_eq!(outcome, CmdOutcome::Refused(Refusal::RotationRestricted));
    assert_same(&state(&core, true), &before, "nothing changed");

    // Rotate by adds to the local rotation.
    run(
        &mut core,
        Command::RotateBy {
            targets: vec![],
            delta: [0.0, 90.0, 0.0],
            snap: None,
        },
    );
    assert_eq!(core.doc().groups.get(id).unwrap().rotation.y, 180.0);
}

#[test]
fn scale_scales_the_subtree_about_the_pivot() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    round_trip(
        &mut core,
        Command::ScaleBy {
            targets: vec![],
            factor: [2.0, 2.0, 2.0],
            snap: None,
        },
    );
    assert_eq!(el(&core, 0).base.s, Vec3::splat(4.0));
    assert_eq!(el(&core, 2).base.s, Vec3::splat(2.0));
    assert_eq!(core.history().undo_label(), Some("Scale"));
}

#[test]
fn delete_removes_the_group_with_everything_below_it() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    round_trip(&mut core, Command::Delete { targets: vec![] });
    assert_eq!(core.doc().elements.len(), 1);
    assert!(core.doc().groups.is_empty());
    assert_eq!(core.history().undo_label(), Some("Delete"));
    // Deleting a cube in a group leaves the group, and undo puts the cube back inside.
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    select(&mut core, &[Target::Element(0)]);
    round_trip(&mut core, Command::Delete { targets: vec![] });
    let id = core.doc().groups.ids()[0];
    assert_eq!(core.doc().groups.get(id).unwrap().children().len(), 1);
}

#[test]
fn duplicate_copies_the_subtree_with_new_ids_behind_the_original() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    let first = core.doc().groups.ids()[0];
    round_trip(&mut core, Command::Duplicate { targets: vec![] });
    assert_eq!(core.doc().groups.len(), 2);
    assert_eq!(core.doc().elements.len(), 5);
    let copy = core.doc().groups.ids()[1];
    assert_ne!(copy, first);
    assert_eq!(
        core.doc().root_nodes()[..2],
        [ObjRef::Group(first), ObjRef::Group(copy)]
    );
    assert!(core.selection().contains(ObjRef::Group(copy)));
    assert_eq!(core.history().undo_label(), Some("Duplicate"));
}

#[test]
fn cut_and_paste_carry_the_objects_of_a_group_without_the_tree() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    round_trip(&mut core, Command::Cut);
    assert!(core.doc().groups.is_empty());
    assert_eq!(core.doc().elements.len(), 1);
    run(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    assert_eq!(core.doc().elements.len(), 3);
    assert!(core.doc().groups.is_empty(), "the clipboard holds no tree");
}

#[test]
fn undo_of_a_command_that_selected_a_group_restores_the_selection() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0)], "A");
    run(
        &mut core,
        Command::MoveBy {
            targets: vec![],
            delta: [1.0, 0.0, 0.0],
            snap: None,
        },
    );
    select(&mut core, &[]);
    assert!(core.undo().is_done());
    assert!(
        core.selection().contains(group_ref(&core, 0)),
        "the group was selected when the move ran"
    );
}

#[test]
fn the_group_commands_snapshot_the_tree() {
    let has = |cmd: Command| match cmd.kind() {
        cuboid_core::CommandKind::Edit(def) => def.categories.contains(Categories::HIERARCHY),
        other => panic!("{other:?}"),
    };
    let t = vec![Target::Group(0)];
    assert!(has(Command::Group {
        targets: t.clone(),
        name: "g".into()
    }));
    assert!(has(Command::Ungroup { targets: t.clone() }));
    assert!(has(Command::Reparent {
        targets: t.clone(),
        parent: None,
        index: None
    }));
    assert!(has(Command::SetGroup {
        group: Target::Group(0),
        name: None,
        origin: None,
        visible: None,
        locked: None,
        export: None
    }));
    assert!(has(Command::MoveBy {
        targets: t.clone(),
        delta: [0.0; 3],
        snap: None
    }));
    assert!(has(Command::Delete { targets: t.clone() }));
    assert!(has(Command::Duplicate { targets: t }));
    // Undo of a group edit does not touch the objects.
    let cmd = Command::Group {
        targets: vec![],
        name: "g".into(),
    };
    match cmd.kind() {
        cuboid_core::CommandKind::Edit(def) => assert_eq!(def.categories, Categories::HIERARCHY),
        other => panic!("{other:?}"),
    }
}

#[test]
fn groups_survive_a_long_session_of_undo_and_redo() {
    let mut core = scene();
    group_of(&mut core, &[Target::Element(0), Target::Element(1)], "A");
    group_of(&mut core, &[Target::Group(0), Target::Element(2)], "B");
    run(&mut core, Command::AddElement(ElementKind::Cube));
    let last = state(&core, true);
    while core.history().undo_len() > SCENE_STEPS {
        assert!(core.undo().is_done());
    }
    assert!(core.doc().groups.is_empty());
    while core.history().redo_len() > 0 {
        assert!(core.redo().is_done());
    }
    assert_same(&state(&core, true), &last, "redo replays the whole session");
}
