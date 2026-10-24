//! One scenario per command: execute, check the result, undo restores the previous state, redo
//! brings the result back.

mod common;

use common::{assert_same, run, scene, select, state};
use cuboid_core::{
    AlignEdge, Axis, CmdOutcome, Command, Compat, Core, ElementKind, FaceId, MeshShape, Refusal,
    SelectOp, Target,
};
use cuboid_model::{FaceName, ObjRef};
use glam::Vec3;

/// Runs `cmd`, then undo and redo, and checks the document at each stage. Returns the state
/// after the command.
fn round_trip(core: &mut Core, cmd: Command) -> String {
    let before = state(core, true);
    let steps = core.history().undo_len();
    run(core, cmd);
    let after = state(core, true);
    assert_ne!(before, after, "the command changed nothing");
    assert_eq!(core.history().undo_len(), steps + 1, "one undo step");
    assert!(core.is_dirty());

    assert!(core.undo().is_done());
    assert_same(
        &state(core, true),
        &before,
        "undo restores the previous state",
    );
    assert!(core.redo().is_done());
    assert_same(&state(core, true), &after, "redo brings the result back");
    after
}

fn el(core: &Core, at: usize) -> cuboid_model::Element {
    core.doc().elements.items()[at].as_ref().clone()
}

fn texture_of(e: &cuboid_model::Element, face: FaceName) -> String {
    e.faces[face as usize].texture.to_string()
}

/// The undo steps `scene()` leaves behind: three times add, position, size.
const SCENE_STEPS: usize = 9;

#[test]
fn add_element_creates_each_kind_at_the_blueprint_and_selects_it() {
    for (kind, name, faces) in [
        (ElementKind::Cube, "Box", 6),
        (ElementKind::PlaneX, "PlaneX", 2),
        (ElementKind::PlaneY, "PlaneY", 2),
        (ElementKind::PlaneZ, "PlaneZ", 2),
    ] {
        let mut core = scene();
        let done = run(&mut core, Command::AddElement(kind));
        assert_eq!(done.created, [Target::Element(3)]);
        let e = el(&core, 3);
        assert!(e.base.label.starts_with(name), "{}", e.base.label);
        assert_eq!(e.faces.iter().filter(|f| f.active).count(), faces);
        assert_eq!(core.doc().blueprint.base.p, e.base.p);
        assert_eq!(core.selection().list.len(), 1);
        let after = state(&core, true);
        assert!(core.undo().is_done());
        assert_eq!(core.doc().elements.len(), 3);
        assert!(core.redo().is_done());
        assert_same(&state(&core, true), &after, "redo of add");
    }
}

#[test]
fn new_elements_take_their_uv_from_their_position() {
    let mut core = scene();
    run(&mut core, Command::AddElement(ElementKind::Cube));
    let e = el(&core, 3);
    let default = cuboid_model::uv::default_uv_rects(e.base.p, e.base.s, false);
    assert!(e.faces.iter().zip(&default).all(|(f, r)| f.uv == *r));
}

#[test]
fn delete_removes_the_selected_objects() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(2)]);
    round_trip(&mut core, Command::Delete { targets: vec![] });
    assert_eq!(core.doc().elements.len(), 1);
    assert!(core.selection().list.is_empty());
    assert_eq!(core.history().undo_label(), Some("Delete"));
}

#[test]
fn delete_with_targets_ignores_the_selection() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    run(
        &mut core,
        Command::Delete {
            targets: vec![Target::Element(1)],
        },
    );
    assert_eq!(core.doc().elements.len(), 2);
    assert_eq!(core.selection().list.len(), 1);
}

#[test]
fn the_blueprint_cannot_be_deleted() {
    let mut core = scene();
    select(&mut core, &[Target::Blueprint]);
    let out = core.execute(Command::Delete { targets: vec![] });
    assert_eq!(out, CmdOutcome::Refused(Refusal::NothingSelected));
    let out = core.execute(Command::Delete {
        targets: vec![Target::Blueprint, Target::Element(0)],
    });
    assert!(out.is_done());
    assert_eq!(core.doc().elements.len(), 2);
}

#[test]
fn duplicate_copies_in_place_and_selects_the_copies() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    let done = run(&mut core, Command::Duplicate { targets: vec![] });
    assert_eq!(done.created, [Target::Element(3)]);
    assert_eq!(el(&core, 3).base.p, el(&core, 1).base.p);
    assert_eq!(el(&core, 3).base.label, el(&core, 1).base.label);
    assert_ne!(
        core.doc().elements.ids()[3],
        core.doc().elements.ids()[1],
        "a copy has its own identity"
    );
    assert_eq!(core.selection().list.len(), 1);
    assert!(
        core.selection()
            .contains(ObjRef::Element(core.doc().elements.ids()[3]))
    );
}

#[test]
fn duplicate_undo_and_redo() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    round_trip(&mut core, Command::Duplicate { targets: vec![] });
    assert_eq!(core.doc().elements.len(), 5);
}

#[test]
fn copy_then_paste_makes_new_selected_objects_in_place() {
    let mut core = scene();
    select(&mut core, &[Target::Element(2)]);
    assert!(core.execute(Command::Copy).is_done());
    assert_eq!(
        core.history().undo_len(),
        SCENE_STEPS,
        "copy is not an undo step"
    );
    select(&mut core, &[Target::Element(0)]);
    round_trip(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    assert_eq!(core.doc().elements.len(), 4);
    assert_eq!(el(&core, 3).base.p, el(&core, 2).base.p);
    assert!(
        core.selection()
            .contains(ObjRef::Element(core.doc().elements.ids()[3]))
    );
    assert_eq!(core.selection().list.len(), 1);
}

#[test]
fn paste_can_land_on_the_blueprint() {
    let mut core = scene();
    select(&mut core, &[Target::Element(2)]);
    core.execute(Command::Copy);
    run(&mut core, Command::Paste { at_blueprint: true });
    assert_eq!(el(&core, 3).base.p, core.doc().blueprint.base.p);
}

#[test]
fn paste_does_not_move_the_blueprint() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    core.execute(Command::Copy);
    let original = core.doc().blueprint.clone();
    run(
        &mut core,
        Command::MoveBy {
            targets: vec![Target::Blueprint],
            delta: [5.0, 0.0, 0.0],
            snap: None,
        },
    );
    let moved = core.doc().blueprint.clone();
    assert_ne!(original, moved);
    run(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    assert_eq!(core.doc().blueprint, moved);
}

#[test]
fn pasting_twice_gives_independent_copies() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    core.execute(Command::Copy);
    run(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    run(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    run(
        &mut core,
        Command::Rename {
            targets: vec![Target::Element(3)],
            name: "first".into(),
        },
    );
    assert_ne!(el(&core, 3).base.label, el(&core, 4).base.label);
}

#[test]
fn paste_with_an_empty_clipboard_is_refused() {
    let mut core = scene();
    assert_eq!(
        core.execute(Command::Paste {
            at_blueprint: false
        }),
        CmdOutcome::Refused(Refusal::ClipboardEmpty)
    );
}

#[test]
fn cut_removes_the_objects_and_keeps_them_in_the_clipboard() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    round_trip(&mut core, Command::Cut);
    assert_eq!(core.doc().elements.len(), 2);
    run(
        &mut core,
        Command::Paste {
            at_blueprint: false,
        },
    );
    assert_eq!(core.doc().elements.len(), 3);
    assert_eq!(el(&core, 2).base.p.x, 4.0);
}

#[test]
fn copy_without_a_selection_is_refused() {
    let mut core = scene();
    assert_eq!(
        core.execute(Command::Copy),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
}

#[test]
fn rename_sets_the_label_of_every_target() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    round_trip(
        &mut core,
        Command::Rename {
            targets: vec![],
            name: "arm".into(),
        },
    );
    assert_eq!(&*el(&core, 0).base.label, "arm");
    assert_eq!(&*el(&core, 1).base.label, "arm");
    assert_eq!(core.history().undo_label(), Some("Rename to arm"));
}

#[test]
fn renaming_to_the_same_name_changes_nothing() {
    let mut core = scene();
    let rename = || Command::Rename {
        targets: vec![Target::Element(0)],
        name: "x".into(),
    };
    run(&mut core, rename());
    let steps = core.history().undo_len();
    assert_eq!(core.execute(rename()), CmdOutcome::Unchanged);
    assert_eq!(core.history().undo_len(), steps);
}

#[test]
fn hide_and_show() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    round_trip(
        &mut core,
        Command::SetHidden {
            targets: vec![],
            hidden: true,
        },
    );
    assert!(el(&core, 1).base.hidden);
    round_trip(
        &mut core,
        Command::SetHidden {
            targets: vec![],
            hidden: false,
        },
    );
    assert!(!el(&core, 1).base.hidden);
}

#[test]
fn lock_and_unlock() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::SetLocked {
            targets: vec![Target::Element(0), Target::Element(2)],
            locked: true,
        },
    );
    assert!(el(&core, 2).base.locked && !el(&core, 1).base.locked);
    round_trip(&mut core, Command::UnlockAll);
    assert!(core.doc().elements.items().iter().all(|e| !e.base.locked));
}

#[test]
fn show_all_unhides_everything() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetHidden {
            targets: vec![Target::Element(0), Target::Element(1)],
            hidden: true,
        },
    );
    round_trip(&mut core, Command::ShowAll);
    assert!(core.doc().elements.items().iter().all(|e| !e.base.hidden));
}

#[test]
fn move_by_offsets_position_and_pivot() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    round_trip(
        &mut core,
        Command::MoveBy {
            targets: vec![],
            delta: [1.0, 2.0, 3.0],
            snap: None,
        },
    );
    let e = el(&core, 1);
    assert_eq!((e.base.p.x, e.base.p.y, e.base.p.z), (5.0, 2.0, 3.0));
    assert_eq!(e.base.o, e.base.p);
}

#[test]
fn move_by_snaps_the_new_pivot_to_the_grid() {
    let mut core = scene();
    run(
        &mut core,
        Command::MoveBy {
            targets: vec![Target::Element(0)],
            delta: [0.3, 0.0, 0.0],
            snap: Some(0.5),
        },
    );
    assert_eq!(el(&core, 0).base.p.x, 0.5);
}

#[test]
fn moving_the_model_origin_is_an_undo_step_too() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::MoveBy {
            targets: vec![Target::ModelOrigin],
            delta: [1.0, 0.0, 0.0],
            snap: None,
        },
    );
}

#[test]
fn set_position_replaces_the_given_components() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::SetPosition {
            targets: vec![Target::Element(2)],
            value: [None, Some(7.0), None],
        },
    );
    let e = el(&core, 2);
    assert_eq!((e.base.p.x, e.base.p.y), (8.0, 7.0));
}

#[test]
fn set_size_goes_through_the_size_setter() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::SetSize {
            targets: vec![Target::Element(0)],
            value: [Some(4.0), None, Some(0.0)],
        },
    );
    let e = el(&core, 0);
    assert_eq!(e.base.s.x, 4.0);
    assert_eq!(e.base.s.y, 2.0);
    assert!(e.base.s.z >= 0.001, "the minimum size holds");
}

#[test]
fn scale_by_scales_about_the_pivot_of_the_first_target() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    round_trip(
        &mut core,
        Command::ScaleBy {
            targets: vec![],
            factor: [2.0, 2.0, 2.0],
            snap: None,
        },
    );
    assert_eq!(el(&core, 0).base.s.x, 4.0);
    assert_eq!(el(&core, 1).base.s.x, 4.0);
    assert_eq!(
        el(&core, 1).base.p.x,
        8.0,
        "positions scale about the first pivot"
    );
}

#[test]
fn rotate_by_turns_about_the_pivot_on_the_grid() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::RotateBy {
            targets: vec![Target::Element(0)],
            delta: [0.0, 45.0, 0.0],
            snap: Some(22.5),
        },
    );
    assert_eq!(el(&core, 0).base.e.y, 45.0);
}

#[test]
fn restricted_rotation_refuses_a_second_axis() {
    let mut core = scene();
    run(
        &mut core,
        Command::RotateBy {
            targets: vec![Target::Element(0)],
            delta: [0.0, 45.0, 0.0],
            snap: None,
        },
    );
    let out = core.execute(Command::RotateBy {
        targets: vec![Target::Element(0)],
        delta: [22.5, 0.0, 0.0],
        snap: None,
    });
    assert_eq!(out, CmdOutcome::Refused(Refusal::RotationRestricted));
    assert_eq!(el(&core, 0).base.e.x, 0.0);
}

#[test]
fn set_rotation_replaces_the_given_components() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::SetRotation {
            targets: vec![Target::Element(1)],
            value: [None, None, Some(45.0)],
        },
    );
    assert_eq!(el(&core, 1).base.e.z, 45.0);
}

#[test]
fn reset_rotation_brings_the_element_back_upright() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetRotation {
            targets: vec![Target::Element(1)],
            value: [None, Some(45.0), None],
        },
    );
    round_trip(
        &mut core,
        Command::ResetRotation {
            targets: vec![Target::Element(1)],
        },
    );
    let e = el(&core, 1);
    assert_eq!(e.base.e, Vec3::ZERO);
    assert_eq!(e.base.o, e.base.p);
}

#[test]
fn reset_pivot_moves_the_pivot_to_the_centre_and_keeps_the_geometry() {
    let mut core = scene();
    let far_corner = |core: &Core| {
        let e = el(core, 1);
        let rot = cuboid_num::euler_to_quat(e.base.e);
        e.base.p + rot * Vec3::new(e.base.s.x, 0.0, 0.0)
    };
    run(
        &mut core,
        Command::SetRotation {
            targets: vec![Target::Element(1)],
            value: [None, Some(45.0), None],
        },
    );
    let before = far_corner(&core);
    round_trip(
        &mut core,
        Command::ResetPivot {
            targets: vec![Target::Element(1)],
        },
    );
    let e = el(&core, 1);
    let rot = cuboid_num::euler_to_quat(e.base.e);
    let centre = e.base.p + rot * Vec3::new(1.0, 1.0, -1.0);
    assert!((e.base.o - centre).length() < 1e-4);
    assert!(
        (far_corner(&core) - before).length() < 1e-4,
        "the box did not move"
    );
}

#[test]
fn quarter_turn_keeps_the_element_axis_aligned() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    round_trip(
        &mut core,
        Command::QuarterTurn {
            targets: vec![],
            axis: Axis::Y,
        },
    );
    assert_eq!(el(&core, 0).base.e, Vec3::ZERO);
    assert_eq!(core.history().undo_label(), Some("Rotate around Y axis"));
}

#[test]
fn mirror_reflects_the_boxes_across_the_centre_of_their_bounds() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(2)]);
    round_trip(
        &mut core,
        Command::Mirror {
            targets: vec![],
            axis: Axis::X,
            copy: false,
        },
    );
    // Bounds [0, 10] have the centre 5: [0, 2] becomes [8, 10] and [8, 10] becomes [0, 2].
    assert_eq!(el(&core, 0).base.p.x, 8.0);
    assert_eq!(el(&core, 2).base.p.x, 0.0);
    assert_eq!(el(&core, 1).base.p.x, 4.0);
}

#[test]
fn mirror_on_z_keeps_the_box_extending_toward_minus_z() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetPosition {
            targets: vec![Target::Element(0)],
            value: [None, None, Some(10.0)],
        },
    );
    // The z extent [8, 10] is its own mirror image about its centre.
    let done = run(
        &mut core,
        Command::Mirror {
            targets: vec![Target::Element(0)],
            axis: Axis::Z,
            copy: false,
        },
    );
    assert_eq!(done.affected, 1);
    assert_eq!(el(&core, 0).base.p.z, 10.0);
}

#[test]
fn mirror_of_a_rotated_element_mirrors_its_corners() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetRotation {
            targets: vec![Target::Element(0)],
            value: [None, Some(45.0), None],
        },
    );
    let corners = |e: &cuboid_model::Element| -> Vec<Vec3> {
        let rot = cuboid_num::euler_to_quat(e.base.e);
        let mut out = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [-1.0, 0.0] {
                    out.push(e.base.p + rot * (Vec3::new(x, y, z) * e.base.s));
                }
            }
        }
        out
    };
    let before = corners(&el(&core, 0));
    // The plane goes through the centre of the unrotated bounds [0, 2].
    run(
        &mut core,
        Command::Mirror {
            targets: vec![Target::Element(0)],
            axis: Axis::X,
            copy: false,
        },
    );
    let after = corners(&el(&core, 0));
    for corner in before {
        let want = Vec3::new(2.0 - corner.x, corner.y, corner.z);
        assert!(
            after.iter().any(|c| (*c - want).length() < 1e-3),
            "{want:?} is not a corner of the mirrored box"
        );
    }
}

#[test]
fn mirror_copy_leaves_the_originals_and_selects_the_copies() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    let done = run(
        &mut core,
        Command::Mirror {
            targets: vec![],
            axis: Axis::X,
            copy: true,
        },
    );
    assert_eq!(done.created, [Target::Element(3), Target::Element(4)]);
    assert_eq!(el(&core, 0).base.p.x, 0.0);
    assert_eq!(
        el(&core, 3).base.p.x,
        4.0,
        "the copy of the first box sits where the second was"
    );
    assert_eq!(core.selection().list.len(), 2);
    assert!(core.undo().is_done());
    assert_eq!(core.doc().elements.len(), 3);
}

#[test]
fn mirror_swaps_the_face_pair_of_the_axis() {
    let mut core = scene();
    run(
        &mut core,
        Command::CreateTexture {
            label: Some("side".into()),
            width: 16,
            height: 16,
            color: [255, 0, 0, 255],
        },
    );
    run(
        &mut core,
        Command::AssignTexture {
            targets: vec![Target::Element(0)],
            label: "side".into(),
            faces: vec![FaceId::East],
        },
    );
    assert_eq!(texture_of(&el(&core, 0), FaceName::East), "side");
    run(
        &mut core,
        Command::Mirror {
            targets: vec![Target::Element(0)],
            axis: Axis::X,
            copy: false,
        },
    );
    assert_eq!(texture_of(&el(&core, 0), FaceName::West), "side");
    assert_ne!(texture_of(&el(&core, 0), FaceName::East), "side");
}

#[test]
fn align_lines_the_other_targets_up_with_the_first() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::Align {
            targets: vec![Target::Element(2), Target::Element(0), Target::Element(1)],
            axis: Axis::X,
            edge: AlignEdge::Min,
        },
    );
    assert_eq!(el(&core, 0).base.p.x, 8.0);
    assert_eq!(el(&core, 1).base.p.x, 8.0);
}

#[test]
fn align_center_uses_the_middle_of_the_boxes() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetSize {
            targets: vec![Target::Element(1)],
            value: [Some(6.0), None, None],
        },
    );
    run(
        &mut core,
        Command::Align {
            targets: vec![Target::Element(1), Target::Element(0)],
            axis: Axis::X,
            edge: AlignEdge::Center,
        },
    );
    // The centre of the box [4, 10] is 7, so the 2-wide box starts at 6.
    assert_eq!(el(&core, 0).base.p.x, 6.0);
}

#[test]
fn create_texture_appends_an_entry() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::CreateTexture {
            label: Some("bark".into()),
            width: 8,
            height: 4,
            color: [10, 20, 30, 255],
        },
    );
    let t = core.doc().textures.entries.last().expect("an entry");
    assert_eq!(&*t.label, "bark");
    assert_eq!((t.texture.image.w, t.texture.image.h), (8, 4));
    assert_eq!(t.texture.image.rgba()[..4], [10, 20, 30, 255]);
}

#[test]
fn create_texture_without_a_label_picks_the_first_free_name() {
    let mut core = scene();
    run(
        &mut core,
        Command::CreateTexture {
            label: None,
            width: 2,
            height: 2,
            color: [0; 4],
        },
    );
    let last = core.doc().textures.entries.last().expect("an entry");
    assert_eq!(&*last.label, "texture1");
}

#[test]
fn create_texture_refuses_duplicates_and_bad_sizes() {
    let mut core = scene();
    let make = |label: &str, w: u32, h: u32| Command::CreateTexture {
        label: Some(label.into()),
        width: w,
        height: h,
        color: [0; 4],
    };
    for cmd in [
        make("texture", 4, 4),
        make("ok", 0, 4),
        make("ok", 100_000, 4),
        make("???", 4, 4),
    ] {
        assert!(
            matches!(
                core.execute(cmd.clone()),
                CmdOutcome::Refused(Refusal::Op(_))
            ),
            "{cmd:?}"
        );
    }
    assert_eq!(core.doc().textures.entries.len(), 2);
}

#[test]
fn remove_texture_takes_the_entry_out() {
    let mut core = scene();
    round_trip(
        &mut core,
        Command::RemoveTexture {
            label: "particle".into(),
        },
    );
    assert_eq!(core.doc().textures.entries.len(), 1);
    assert!(matches!(
        core.execute(Command::RemoveTexture {
            label: "nope".into()
        }),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
}

#[test]
fn assign_texture_points_the_chosen_faces_at_it() {
    let mut core = scene();
    run(
        &mut core,
        Command::CreateTexture {
            label: Some("wood".into()),
            width: 16,
            height: 16,
            color: [100, 60, 20, 255],
        },
    );
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    round_trip(
        &mut core,
        Command::AssignTexture {
            targets: vec![],
            label: "wood".into(),
            faces: vec![FaceId::Up, FaceId::Down],
        },
    );
    let e = el(&core, 1);
    assert_eq!(texture_of(&e, FaceName::Up), "wood");
    assert_eq!(texture_of(&e, FaceName::North), "texture");
    run(
        &mut core,
        Command::AssignTexture {
            targets: vec![Target::Element(2)],
            label: "wood".into(),
            faces: vec![],
        },
    );
    assert!(el(&core, 2).faces.iter().all(|f| &*f.texture == "wood"));
}

#[test]
fn assign_texture_needs_an_element_and_an_existing_texture() {
    let mut core = scene();
    let assign = |label: &str| Command::AssignTexture {
        targets: vec![],
        label: label.into(),
        faces: vec![],
    };
    assert_eq!(
        core.execute(assign("texture")),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
    select(&mut core, &[Target::Element(0)]);
    assert!(matches!(
        core.execute(assign("missing")),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
}

#[test]
fn selection_commands_leave_no_undo_step() {
    let mut core = scene();
    let id = |core: &Core, at: usize| ObjRef::Element(core.doc().elements.ids()[at]);
    assert!(core.execute(Command::SelectAll).is_done());
    assert_eq!(core.selection().list.len(), 3);
    assert_eq!(core.selection().primary, Some(id(&core, 0)));
    let select_op = |targets: Vec<Target>, op| Command::Select { targets, op };
    assert!(
        core.execute(select_op(vec![Target::Element(1)], SelectOp::Toggle))
            .is_done()
    );
    assert_eq!(core.selection().list.len(), 2);
    assert!(core.execute(Command::SelectInverse).is_done());
    assert_eq!(core.selection().list.len(), 1);
    assert!(core.selection().contains(id(&core, 1)));
    assert!(
        core.execute(select_op(
            vec![Target::Element(0), Target::Blueprint],
            SelectOp::Add
        ))
        .is_done()
    );
    assert!(core.selection().contains(ObjRef::Blueprint));
    assert!(
        core.execute(select_op(vec![Target::Blueprint], SelectOp::Remove))
            .is_done()
    );
    assert!(core.execute(Command::SelectNone).is_done());
    assert!(core.selection().list.is_empty());
    assert_eq!(core.execute(Command::SelectNone), CmdOutcome::Unchanged);
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

#[test]
fn select_all_skips_hidden_objects() {
    let mut core = scene();
    run(
        &mut core,
        Command::SetHidden {
            targets: vec![Target::Element(1)],
            hidden: true,
        },
    );
    core.execute(Command::SelectAll);
    assert_eq!(core.selection().list.len(), 2);
}

#[test]
fn undo_and_redo_commands_walk_the_history() {
    let mut core = scene();
    run(&mut core, Command::AddElement(ElementKind::Cube));
    assert!(core.execute(Command::Undo).is_done());
    assert_eq!(core.doc().elements.len(), 3);
    assert!(core.execute(Command::Redo).is_done());
    assert_eq!(core.doc().elements.len(), 4);
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    let mut empty = Core::new(Compat::port(), 1);
    assert_eq!(
        empty.execute(Command::Undo),
        CmdOutcome::Refused(Refusal::NothingToUndo)
    );
    assert_eq!(
        empty.execute(Command::Redo),
        CmdOutcome::Refused(Refusal::NothingToRedo)
    );
}

#[test]
fn a_new_command_after_undo_drops_the_redo_entries() {
    let mut core = scene();
    run(&mut core, Command::AddElement(ElementKind::Cube));
    core.undo();
    run(&mut core, Command::AddElement(ElementKind::PlaneX));
    assert_eq!(core.redo(), CmdOutcome::Refused(Refusal::NothingToRedo));
}

#[test]
fn commands_that_need_objects_refuse_without_a_selection() {
    let mut core = scene();
    for cmd in [
        Command::MoveBy {
            targets: vec![],
            delta: [1.0; 3],
            snap: None,
        },
        Command::Rename {
            targets: vec![],
            name: "a".into(),
        },
        Command::Duplicate { targets: vec![] },
        Command::Cut,
        Command::QuarterTurn {
            targets: vec![],
            axis: Axis::X,
        },
    ] {
        assert_eq!(
            core.execute(cmd.clone()),
            CmdOutcome::Refused(Refusal::NothingSelected),
            "{cmd:?}"
        );
    }
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

#[test]
fn bad_targets_and_numbers_are_refused_without_touching_the_document() {
    let mut core = scene();
    let before = state(&core, true);
    let first = vec![Target::Element(0)];
    assert_eq!(
        core.execute(Command::Delete {
            targets: vec![Target::Element(9)]
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    assert_eq!(
        core.execute(Command::MoveBy {
            targets: first.clone(),
            delta: [f32::NAN, 0.0, 0.0],
            snap: None
        }),
        CmdOutcome::Refused(Refusal::NotFinite)
    );
    assert_eq!(
        core.execute(Command::SetSize {
            targets: first.clone(),
            value: [Some(f32::INFINITY), None, None]
        }),
        CmdOutcome::Refused(Refusal::NotFinite)
    );
    assert_eq!(
        core.execute(Command::ScaleBy {
            targets: first,
            factor: [-1.0, 1.0, 1.0],
            snap: None
        }),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
    assert_eq!(state(&core, true), before);
}

#[test]
fn deferred_commands_report_not_yet() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    for cmd in [
        Command::AddMesh(MeshShape::Sphere),
        Command::Group {
            targets: vec![],
            name: "g".into(),
        },
        Command::Ungroup { targets: vec![] },
        Command::Reparent {
            targets: vec![],
            parent: None,
        },
    ] {
        assert!(
            matches!(core.execute(cmd.clone()), CmdOutcome::NotYet(_)),
            "{cmd:?}"
        );
    }
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

#[test]
fn merged_commands_share_one_undo_step() {
    let mut core = scene();
    let before = state(&core, false);
    let set_x = |x: f32| Command::SetPosition {
        targets: vec![Target::Element(0)],
        value: [Some(x), None, None],
    };
    for x in 1..=5_u8 {
        assert!(core.execute_merged(set_x(f32::from(x)), 77).is_done());
    }
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert_eq!(el(&core, 0).base.p.x, 5.0);
    core.undo();
    assert_eq!(state(&core, false), before);
    // A merge never reaches across an undo.
    core.redo();
    core.execute_merged(set_x(9.0), 77);
    assert_eq!(
        core.history().undo_len(),
        SCENE_STEPS + 2,
        "a new step after the undo"
    );
    core.execute(set_x(1.0));
    core.execute_merged(set_x(2.0), 77);
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 4);
}

#[test]
fn the_unsaved_flag_follows_edits_and_saves() {
    let mut core = Core::new(Compat::port(), 1);
    assert!(!core.is_dirty());
    run(&mut core, Command::AddElement(ElementKind::Cube));
    assert!(core.is_dirty());
    core.mark_saved();
    assert!(!core.is_dirty());
    core.undo();
    assert!(core.is_dirty(), "the saved file no longer matches");
}

#[test]
fn the_outcome_names_what_it_refreshes() {
    use cuboid_core::Refresh;
    let mut core = Core::new(Compat::port(), 1);
    let done = run(&mut core, Command::AddElement(ElementKind::Cube));
    assert!(
        done.effects
            .refresh
            .contains(Refresh::EDIT_MENU | Refresh::LISTS | Refresh::TITLE)
    );
    let done = run(&mut core, Command::AddElement(ElementKind::Cube));
    assert!(!done.effects.refresh.contains(Refresh::TITLE));
}

#[test]
fn query_reports_labels_selection_and_enabled_actions() {
    use cuboid_actions::ActionId;
    let mut core = scene();
    let q = core.query();
    assert_eq!(q.element_count, 3);
    assert_eq!(q.undo_label.as_deref(), Some("Set size"));
    assert_eq!(q.redo_label, None);
    assert_eq!(q.action_enabled(ActionId::Delete), Some(false));
    assert_eq!(q.action_enabled(ActionId::SelectAllElements), Some(true));
    assert_eq!(q.action_enabled(ActionId::Paste), Some(false));
    assert_eq!(q.action_enabled(ActionId::Save), None);
    select(&mut core, &[Target::Element(1)]);
    core.execute(Command::Copy);
    let q = core.query();
    assert_eq!(q.selection.elements, 1);
    assert_eq!(q.action_enabled(ActionId::Delete), Some(true));
    assert_eq!(q.action_enabled(ActionId::Paste), Some(true));
    assert_eq!(q.action_enabled(ActionId::RotateX), Some(true));
    let props = q.primary_props.expect("a primary");
    assert_eq!(props.position.x, 4.0);
    assert_eq!(props.size.x, 2.0);
    assert_eq!(props.faces.expect("an element").len(), 6);
    core.undo();
    assert_eq!(core.query().redo_label.as_deref(), Some("Set size"));
}

#[test]
fn the_outliner_lists_elements_then_meshes() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    let rows = cuboid_core::outliner(core.doc(), core.selection());
    assert_eq!(rows.len(), 3);
    assert!(rows[1].selected && !rows[0].selected);
    assert_eq!(rows[2].display_id, 3);
}

#[test]
fn export_snapshot_and_swap_document() {
    use cuboid_core::{Stale, SwapKind};
    let mut core = scene();
    let snap = core.export_snapshot();
    assert_eq!(snap.elements.len(), 3);
    let base = core.doc().revision;
    let mut other = (*snap).clone();
    other.elements.clear();
    assert_eq!(
        core.swap_document(other.clone(), base, SwapKind::Open),
        Ok(())
    );
    assert_eq!(core.doc().elements.len(), 0);
    assert_eq!(core.history().undo_len(), 0, "an open clears the history");
    assert!(!core.is_dirty());
    assert_eq!(core.swap_document(other, base, SwapKind::Open), Err(Stale));
}

#[test]
fn an_import_keeps_the_history_and_marks_the_document_changed() {
    use cuboid_core::SwapKind;
    let mut core = scene();
    core.mark_saved();
    let snap = core.export_snapshot();
    let base = core.doc().revision;
    core.swap_document((*snap).clone(), base, SwapKind::Import)
        .expect("not stale");
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
    assert!(core.is_dirty());
}
