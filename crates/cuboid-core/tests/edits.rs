//! The edits of the properties panel: pivot, colour, shading, the switches of a face and the
//! pixels of a texture. One scenario each: execute, undo restores, redo brings it back, the same
//! value again changes nothing and a bad value is refused.

mod common;

use common::{assert_same, el, round_trip, run, scene, select, state};
use cuboid_core::{
    CmdOutcome, Command, Core, FaceId, FrameSpec, MeshShape, Refusal, Target, TextureFrames,
};
use cuboid_model::{ElementFlags, FaceName};
use glam::Vec3;

/// The undo steps `scene()` leaves behind: three times add, position, size.
const SCENE_STEPS: usize = 9;

fn set_pivot(at: usize, value: [Option<f32>; 3]) -> Command {
    Command::SetPivot {
        targets: vec![Target::Element(at)],
        value,
    }
}

#[test]
fn set_pivot_moves_only_the_pivot_as_one_undo_step() {
    let mut core = scene();
    let before = el(&core, 1);
    round_trip(&mut core, set_pivot(1, [Some(5.0), None, Some(-1.0)]));
    let after = el(&core, 1);
    assert_eq!(after.base.o, Vec3::new(5.0, before.base.o.y, -1.0));
    assert_eq!(
        (after.base.p, after.base.s, after.base.e),
        (before.base.p, before.base.s, before.base.e),
        "the geometry stays where it is"
    );
    assert_eq!(
        el(&core, 0).base.o,
        Vec3::ZERO,
        "other elements are untouched"
    );
    assert_eq!(core.history().undo_label(), Some("Set pivot"));

    // The same pivot again changes nothing and leaves no step.
    let steps = core.history().undo_len();
    let same = set_pivot(1, [Some(5.0), Some(after.base.o.y), Some(-1.0)]);
    assert_eq!(core.execute(same), CmdOutcome::Unchanged);
    assert_eq!(core.history().undo_len(), steps);
}

#[test]
fn a_pivot_drag_is_one_undo_step() {
    let mut core = scene();
    let before = state(&core, false);
    for x in [1.0, 2.0, 3.0] {
        let cmd = set_pivot(0, [Some(x), None, None]);
        assert!(core.execute_merged(cmd, 9).is_done());
    }
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert_eq!(el(&core, 0).base.o.x, 3.0);
    assert!(core.undo().is_done());
    assert_same(&state(&core, false), &before, "one undo ends the drag");
}

#[test]
fn a_new_pivot_is_what_later_rotations_turn_about() {
    let mut core = scene();
    let before = el(&core, 0).base.p;
    run(&mut core, set_pivot(0, [Some(10.0), Some(0.0), Some(0.0)]));
    let turn = Command::SetRotation {
        targets: vec![Target::Element(0)],
        value: [None, Some(90.0), None],
    };
    run(&mut core, turn);
    assert_ne!(
        el(&core, 0).base.p,
        before,
        "the box swings about the new pivot"
    );
}

#[test]
fn a_pivot_needs_a_listed_object_and_finite_numbers() {
    let mut core = scene();
    let before = state(&core, false);
    let nan = set_pivot(0, [Some(f32::NAN), None, None]);
    assert_eq!(core.execute(nan), CmdOutcome::Refused(Refusal::NotFinite));
    let selected = Command::SetPivot {
        targets: vec![],
        value: [Some(1.0), None, None],
    };
    assert_eq!(
        core.execute(selected.clone()),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
    select(&mut core, &[Target::Blueprint]);
    assert_eq!(
        core.execute(selected),
        CmdOutcome::Refused(Refusal::NothingSelected),
        "the pivot of the blueprint is its position"
    );
    let far = set_pivot(7, [Some(1.0), None, None]);
    assert_eq!(core.execute(far), CmdOutcome::Refused(Refusal::BadTarget));
    assert_same(&state(&core, false), &before, "nothing changed");
}

fn set_color(at: usize, color: [f32; 3]) -> Command {
    Command::SetColor {
        targets: vec![Target::Element(at)],
        color,
    }
}

#[test]
fn set_color_stores_the_colour_a_record_holds() {
    use cuboid_num::pack::from_byte;

    let mut core = scene();
    round_trip(&mut core, set_color(2, [0.4, 0.8, 1.0]));
    let colour = el(&core, 2).color;
    assert_eq!(
        colour,
        [from_byte(102), from_byte(204), from_byte(255), 1.0],
        "bytes, opaque"
    );
    assert!(
        cuboid_model::normal_form::element(&el(&core, 2), true).is_none(),
        "a save and a load give the same colour"
    );
    assert_eq!(core.history().undo_label(), Some("Set color"));
    assert_ne!(el(&core, 0).color, colour, "other elements keep theirs");

    // Each channel is at least 0.25.
    run(&mut core, set_color(2, [0.0, 0.1, 0.2]));
    let dark = el(&core, 2).color;
    assert!(
        dark[..3].iter().all(|c| (0.24..0.26).contains(c)),
        "{dark:?}"
    );

    // The same bytes again change nothing.
    let steps = core.history().undo_len();
    let again = set_color(2, [0.0, 0.1, 0.2]);
    assert_eq!(core.execute(again), CmdOutcome::Unchanged);
    assert_eq!(core.history().undo_len(), steps);
}

#[test]
fn a_colour_drag_is_one_undo_step_and_a_bad_colour_is_refused() {
    let mut core = scene();
    let before = state(&core, false);
    for c in [0.3, 0.5, 0.7] {
        assert!(core.execute_merged(set_color(0, [c; 3]), 4).is_done());
    }
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert!(core.undo().is_done());
    assert_same(&state(&core, false), &before, "one undo ends the drag");

    let nan = set_color(0, [0.5, f32::INFINITY, 0.5]);
    assert_eq!(core.execute(nan), CmdOutcome::Refused(Refusal::NotFinite));
    select(&mut core, &[Target::Blueprint]);
    let selected = Command::SetColor {
        targets: vec![],
        color: [0.5; 3],
    };
    assert_eq!(
        core.execute(selected),
        CmdOutcome::Refused(Refusal::NoElements)
    );
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

#[test]
fn set_shade_turns_the_directional_shading_on_and_off() {
    let mut core = scene();
    assert!(el(&core, 1).flags.contains(ElementFlags::SHADE));
    let off = Command::SetShade {
        targets: vec![Target::Element(1)],
        enabled: false,
    };
    round_trip(&mut core, off.clone());
    assert!(!el(&core, 1).flags.contains(ElementFlags::SHADE));
    assert!(el(&core, 0).flags.contains(ElementFlags::SHADE));
    assert_eq!(core.history().undo_label(), Some("Disable shade"));
    assert_eq!(core.execute(off), CmdOutcome::Unchanged);

    let on = Command::SetShade {
        targets: vec![Target::Element(1)],
        enabled: true,
    };
    round_trip(&mut core, on);
    assert!(el(&core, 1).flags.contains(ElementFlags::SHADE));
    assert_eq!(core.history().undo_label(), Some("Enable shade"));
}

#[test]
fn shading_needs_an_element() {
    let mut core = scene();
    let cmd = Command::SetShade {
        targets: vec![],
        enabled: false,
    };
    assert_eq!(
        core.execute(cmd.clone()),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
    run(&mut core, Command::AddMesh(MeshShape::Cube));
    assert_eq!(
        core.execute(cmd),
        CmdOutcome::Refused(Refusal::NoElements),
        "a mesh has no shade flag"
    );
}

fn enabled(face: FaceId, on: bool) -> Command {
    Command::SetFaceEnabled {
        targets: vec![Target::Element(0)],
        faces: vec![face],
        enabled: on,
    }
}

fn tint(face: FaceId, tint: i32) -> Command {
    Command::SetFaceTint {
        targets: vec![Target::Element(0)],
        faces: vec![face],
        tint,
    }
}

fn cull(face: FaceId, cull: Option<FaceId>) -> Command {
    Command::SetFaceCull {
        targets: vec![Target::Element(0)],
        faces: vec![face],
        cull,
    }
}

#[test]
fn set_face_enabled_switches_one_face() {
    let mut core = scene();
    round_trip(&mut core, enabled(FaceId::Up, false));
    let e = el(&core, 0);
    assert!(!e.faces[FaceName::Up as usize].active);
    assert_eq!(e.faces.iter().filter(|f| f.active).count(), 5);
    assert_eq!(core.history().undo_label(), Some("Disable Up face"));
    assert_eq!(
        core.execute(enabled(FaceId::Up, false)),
        CmdOutcome::Unchanged
    );
    round_trip(&mut core, enabled(FaceId::Up, true));
    assert!(el(&core, 0).faces[FaceName::Up as usize].active);
    assert_eq!(core.history().undo_label(), Some("Enable Up face"));

    // No faces named is all six.
    let all = Command::SetFaceEnabled {
        targets: vec![Target::Element(0)],
        faces: vec![],
        enabled: false,
    };
    round_trip(&mut core, all);
    assert!(el(&core, 0).faces.iter().all(|f| !f.active));
    assert_eq!(core.history().undo_label(), Some("Disable all faces"));
}

#[test]
fn set_face_tint_sets_the_index_of_the_face() {
    let mut core = scene();
    round_trip(&mut core, tint(FaceId::North, 2));
    let e = el(&core, 0);
    assert_eq!(e.faces[FaceName::North as usize].tint, 2);
    assert_eq!(e.faces.iter().filter(|f| f.tint == -1).count(), 5);
    assert_eq!(core.history().undo_label(), Some("Set tint"));
    assert_eq!(core.execute(tint(FaceId::North, 2)), CmdOutcome::Unchanged);

    // -1 takes the tint away; anything below is no index.
    round_trip(&mut core, tint(FaceId::North, -1));
    assert_eq!(
        core.execute(tint(FaceId::North, -2)),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
}

#[test]
fn set_face_cull_points_the_face_at_a_neighbour_or_at_none() {
    let mut core = scene();
    let bits =
        |core: &Core, at: usize, face: FaceName| el(core, at).faces[face as usize].cull.bits();
    round_trip(&mut core, cull(FaceId::Up, Some(FaceId::Down)));
    assert_eq!(bits(&core, 0, FaceName::Up), 1, "down is bit 1");
    assert_eq!(core.history().undo_label(), Some("Set cull face"));
    assert_eq!(
        core.execute(cull(FaceId::Up, Some(FaceId::Down))),
        CmdOutcome::Unchanged
    );
    round_trip(&mut core, cull(FaceId::Up, None));
    assert_eq!(bits(&core, 0, FaceName::Up), 0);
    assert_eq!(core.execute(cull(FaceId::Up, None)), CmdOutcome::Unchanged);

    // Every neighbour has its own bit.
    for (neighbour, expected) in [
        (FaceId::Up, 2),
        (FaceId::North, 4),
        (FaceId::South, 8),
        (FaceId::West, 16),
        (FaceId::East, 32),
    ] {
        run(&mut core, cull(FaceId::Down, Some(neighbour)));
        assert_eq!(bits(&core, 0, FaceName::Down), expected);
    }
}

#[test]
fn face_commands_need_an_element() {
    let mut core = scene();
    let before = state(&core, false);
    for cmd in [
        Command::SetFaceEnabled {
            targets: vec![],
            faces: vec![],
            enabled: false,
        },
        Command::SetFaceTint {
            targets: vec![],
            faces: vec![],
            tint: 1,
        },
        Command::SetFaceCull {
            targets: vec![],
            faces: vec![],
            cull: None,
        },
    ] {
        assert_eq!(
            core.execute(cmd.clone()),
            CmdOutcome::Refused(Refusal::NothingSelected),
            "{cmd:?}"
        );
    }
    let far = Command::SetFaceEnabled {
        targets: vec![Target::Element(9)],
        faces: vec![],
        enabled: false,
    };
    assert_eq!(core.execute(far), CmdOutcome::Refused(Refusal::BadTarget));
    assert_same(&state(&core, false), &before, "nothing changed");
}

fn pixels(w: u32, h: u32, texel: [u8; 4]) -> Vec<u8> {
    texel.repeat(w as usize * h as usize)
}

fn texture_bytes(core: &Core, label: &str) -> (u32, u32, Vec<u8>) {
    let entry = core
        .doc()
        .textures
        .entries
        .iter()
        .find(|e| &*e.label == label)
        .expect("the texture");
    let image = &entry.texture.image;
    (image.w, image.h, image.rgba().to_vec())
}

fn replace(label: &str, w: u32, h: u32, texel: [u8; 4]) -> Command {
    Command::ReplaceTexture {
        label: label.to_owned(),
        width: w,
        height: h,
        rgba: pixels(w, h, texel),
        file: None,
    }
}

#[test]
fn replace_texture_swaps_the_pixels_and_keeps_the_label() {
    let mut core = scene();
    let old = texture_bytes(&core, "texture");
    let labels = |core: &Core| -> Vec<_> {
        el(core, 0)
            .faces
            .iter()
            .map(|f| f.texture.clone())
            .collect()
    };
    let faces = labels(&core);
    round_trip(&mut core, replace("texture", 4, 2, [9, 8, 7, 255]));
    assert_eq!(
        texture_bytes(&core, "texture"),
        (4, 2, pixels(4, 2, [9, 8, 7, 255]))
    );
    assert_eq!(core.history().undo_label(), Some("Replace texture"));
    assert_eq!(labels(&core), faces, "the faces keep the label");
    assert_eq!(core.doc().textures.entries.len(), 2, "no entry is added");

    // Undo hands the earlier pixels back, redo the later ones.
    assert!(core.undo().is_done());
    assert_eq!(texture_bytes(&core, "texture"), old);
    assert!(core.redo().is_done());
    assert_eq!(texture_bytes(&core, "texture").0, 4);

    // The same pixels again leave no step behind.
    let steps = core.history().undo_len();
    let again = replace("texture", 4, 2, [9, 8, 7, 255]);
    assert_eq!(core.execute(again), CmdOutcome::Unchanged);
    assert_eq!(core.history().undo_len(), steps);
}

#[test]
fn a_reload_records_the_signature_and_the_length_of_the_file() {
    let mut core = scene();
    let with_file = |signature: &str, size: u64| Command::ReplaceTexture {
        label: "texture".to_owned(),
        width: 2,
        height: 2,
        rgba: pixels(2, 2, [1, 2, 3, 255]),
        file: Some((signature.to_owned(), size)),
    };
    let source = |core: &Core| {
        let source = &core.doc().textures.entries[1].texture.source;
        (source.signature.clone(), source.file_size)
    };
    round_trip(&mut core, with_file("abc", 70));
    assert_eq!(source(&core), (Some("abc".to_owned()), 70));
    // The pixels are the same now, the file is not: that is a change.
    let steps = core.history().undo_len();
    run(&mut core, with_file("abd", 71));
    assert_eq!(source(&core), (Some("abd".to_owned()), 71));
    assert_eq!(core.history().undo_len(), steps + 1);
    assert_eq!(core.execute(with_file("abd", 71)), CmdOutcome::Unchanged);
    assert!(core.undo().is_done());
    assert_eq!(source(&core), (Some("abc".to_owned()), 70));
    assert!(core.redo().is_done());
    assert_eq!(source(&core), (Some("abd".to_owned()), 71));
}

#[test]
fn replace_texture_refuses_what_does_not_fit() {
    let mut core = scene();
    let before = state(&core, false);
    let old = texture_bytes(&core, "texture");
    let short = Command::ReplaceTexture {
        label: "texture".to_owned(),
        width: 4,
        height: 4,
        rgba: vec![0; 10],
        file: None,
    };
    assert_eq!(
        core.execute(short),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
    let empty = replace("texture", 0, 4, [0; 4]);
    assert_eq!(
        core.execute(empty),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
    let huge = Command::ReplaceTexture {
        label: "texture".to_owned(),
        width: 1 << 20,
        height: 1 << 20,
        rgba: Vec::new(),
        file: None,
    };
    assert_eq!(core.execute(huge), CmdOutcome::Refused(Refusal::OutOfRange));
    let gone = replace("nothing", 2, 2, [0; 4]);
    assert!(matches!(
        core.execute(gone),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    assert_same(&state(&core, false), &before, "nothing changed");
    assert_eq!(texture_bytes(&core, "texture"), old);
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

fn animate(label: &str, frame_height: u32, frametime: u32) -> Command {
    Command::SetTextureAnimation {
        label: label.to_owned(),
        animation: Some(TextureFrames {
            frame_height,
            frametime,
            interpolate: false,
            frames: Vec::new(),
        }),
    }
}

fn animation(core: &Core, label: &str) -> Option<cuboid_model::TextureAnimation> {
    let entry = core
        .doc()
        .textures
        .entries
        .iter()
        .find(|e| &*e.label == label);
    entry.expect("the texture").animation.clone()
}

#[test]
fn a_texture_becomes_a_strip_in_one_undo_step_and_a_frame_grows_it_in_another() {
    let mut core = scene();
    // The 16 x 16 texture is a strip of one frame, which a frame can grow.
    let steps = core.history().undo_len();
    run(&mut core, animate("texture", 16, 4));
    assert_eq!(core.history().undo_label(), Some("Set texture animation"));
    assert_eq!(core.history().undo_len(), steps + 1);
    let set = animation(&core, "texture").expect("animated");
    assert_eq!(
        (set.frame_width, set.frame_height, set.frametime),
        (16, 16, 4)
    );

    run(
        &mut core,
        Command::AddTextureFrame {
            label: "texture".into(),
            tile: 0,
        },
    );
    assert_eq!(core.history().undo_label(), Some("Add texture frame"));
    let (w, h, _) = texture_bytes(&core, "texture");
    assert_eq!((w, h), (16, 32));
    run(
        &mut core,
        Command::AddTextureFrame {
            label: "texture".into(),
            tile: 1,
        },
    );
    assert_eq!(texture_bytes(&core, "texture").1, 48);

    // Each is one step of the texture category: undo brings back the picture and the strip.
    assert!(core.undo().is_done());
    assert_eq!(texture_bytes(&core, "texture").1, 32);
    assert!(core.undo().is_done());
    assert_eq!(texture_bytes(&core, "texture").1, 16);
    assert!(core.undo().is_done());
    assert!(animation(&core, "texture").is_none());
    assert!(core.redo().is_done());
    assert_eq!(animation(&core, "texture"), Some(set));

    // The same animation again leaves no step; none turns it into a still texture.
    assert_eq!(
        core.execute(animate("texture", 16, 4)),
        CmdOutcome::Unchanged
    );
    let none = Command::SetTextureAnimation {
        label: "texture".into(),
        animation: None,
    };
    run(&mut core, none);
    assert!(animation(&core, "texture").is_none());
}

#[test]
fn the_animation_commands_refuse_what_does_not_fit() {
    let mut core = scene();
    let before = state(&core, false);
    let refused = |core: &mut Core, cmd: Command| core.execute(cmd);
    // A frame height that does not divide the picture, one of zero, and bad times.
    assert!(matches!(
        refused(&mut core, animate("texture", 5, 1)),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    for bad in [
        animate("texture", 0, 1),
        animate("texture", 16, 0),
        animate("texture", 16, 72_001),
    ] {
        assert_eq!(
            refused(&mut core, bad),
            CmdOutcome::Refused(Refusal::OutOfRange)
        );
    }
    let timed = Command::SetTextureAnimation {
        label: "texture".into(),
        animation: Some(TextureFrames {
            frame_height: 16,
            frametime: 1,
            interpolate: false,
            frames: vec![FrameSpec {
                index: 0,
                time: Some(0),
            }],
        }),
    };
    assert_eq!(
        refused(&mut core, timed),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
    // A step that names a frame the strip does not have.
    let missing = Command::SetTextureAnimation {
        label: "texture".into(),
        animation: Some(TextureFrames {
            frame_height: 16,
            frametime: 1,
            interpolate: false,
            frames: vec![FrameSpec {
                index: 3,
                time: None,
            }],
        }),
    };
    assert!(matches!(
        refused(&mut core, missing),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    assert!(matches!(
        refused(&mut core, animate("nothing", 16, 1)),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    // A frame needs a strip, and a frame that is not in it cannot be copied.
    let frame = |label: &str, tile| Command::AddTextureFrame {
        label: label.into(),
        tile,
    };
    assert!(matches!(
        refused(&mut core, frame("texture", 0)),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    run(&mut core, animate("texture", 16, 1));
    assert!(matches!(
        refused(&mut core, frame("texture", 1)),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    assert!(matches!(
        refused(&mut core, frame("nothing", 0)),
        CmdOutcome::Refused(Refusal::Op(_))
    ));
    core.undo();
    assert_same(&state(&core, false), &before, "nothing changed");
    assert_eq!(core.history().undo_len(), SCENE_STEPS);
}

#[test]
fn a_new_picture_of_another_size_ends_the_animation_and_undo_brings_it_back() {
    let mut core = scene();
    run(&mut core, animate("texture", 16, 2));
    run(&mut core, replace("texture", 16, 32, [1, 2, 3, 255]));
    assert!(
        animation(&core, "texture").is_some(),
        "frames of the same width fit"
    );
    run(&mut core, replace("texture", 16, 20, [1, 2, 3, 255]));
    assert!(animation(&core, "texture").is_none());
    assert!(core.undo().is_done());
    assert!(animation(&core, "texture").is_some());
}
