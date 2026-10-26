//! The animation commands (animation A2 4). One scenario each: execute, undo restores, redo brings
//! it back, the same request again changes nothing and a bad one is refused. A removal of groups
//! takes their tracks in the same step.

mod common;

use common::{round_trip, run, scene, select, state};
use cuboid_core::{
    AnimationLoop, BoneChannel, CmdOutcome, Command, Core, KeyAt, KeyInterp, KeyValue, Refusal,
    Target,
};
use cuboid_model::{AnimationError, Interp, LoopMode, check_invariants};

fn group(core: &mut Core, at: usize, name: &str) {
    select(core, &[Target::Element(at)]);
    run(
        core,
        Command::Group {
            targets: vec![],
            name: name.into(),
        },
    );
}

/// The scene with a group "Arm" over the first cube and an animation "walk" of two seconds.
fn animated() -> Core {
    let mut core = scene();
    group(&mut core, 0, "Arm");
    run(
        &mut core,
        Command::AddAnimation {
            name: "walk".into(),
            length: 2.0,
        },
    );
    core
}

fn key(time: f32, x: f32) -> Command {
    Command::SetKeyframe {
        animation: 0,
        group: Target::Group(0),
        channel: BoneChannel::Rotation,
        time,
        value: KeyValue::nums([x, 0.0, 0.0]),
        interp: None,
    }
}

fn keys_of(core: &Core) -> Vec<(f32, f32)> {
    let doc = core.doc();
    let group = doc.groups.ids()[0];
    doc.animations.items()[0]
        .track(group)
        .map_or_else(Vec::new, |t| {
            t.rotation
                .iter()
                .map(|k| (k.time, k.value[0].number().unwrap()))
                .collect()
        })
}

#[test]
fn add_animation_makes_an_animation_with_a_name_and_a_length() {
    let mut core = scene();
    let after = round_trip(
        &mut core,
        Command::AddAnimation {
            name: "walk".into(),
            length: 1.5,
        },
    );
    assert!(after.contains("walk"));
    assert_eq!(core.history().undo_label(), Some("Add animation"));
    let animation = &core.doc().animations.items()[0];
    assert_eq!(animation.length, 1.5);
    assert_eq!(animation.loop_mode, LoopMode::Once);
    assert!(animation.tracks.is_empty());
    // A second one comes after it.
    run(
        &mut core,
        Command::AddAnimation {
            name: String::new(),
            length: 0.0,
        },
    );
    assert_eq!(core.doc().animations.len(), 2);
    assert_eq!(&*core.doc().animations.items()[1].name, "animation");
    for length in [-1.0, 3601.0] {
        assert_eq!(
            core.execute(Command::AddAnimation {
                name: "x".into(),
                length
            }),
            CmdOutcome::Refused(Refusal::OutOfRange)
        );
    }
    assert_eq!(
        core.execute(Command::AddAnimation {
            name: "x".into(),
            length: f32::NAN
        }),
        CmdOutcome::Refused(Refusal::NotFinite)
    );
}

#[test]
fn remove_animation_takes_it_out_and_undo_brings_back_its_id_and_keyframes() {
    let mut core = animated();
    run(&mut core, key(0.5, 10.0));
    let id = core.doc().animations.ids()[0];
    round_trip(&mut core, Command::RemoveAnimation { animation: 0 });
    assert!(core.doc().animations.is_empty());
    assert_eq!(core.history().undo_label(), Some("Delete animation"));
    core.undo();
    assert_eq!(core.doc().animations.ids(), [id]);
    assert_eq!(keys_of(&core), [(0.5, 10.0)]);
    assert_eq!(
        core.execute(Command::RemoveAnimation { animation: 3 }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn set_animation_changes_the_given_fields_only() {
    let mut core = animated();
    let after = round_trip(
        &mut core,
        Command::SetAnimation {
            animation: 0,
            name: Some("run".into()),
            length: Some(1.0),
            loop_mode: Some(AnimationLoop::Loop),
            snapping: Some(30),
        },
    );
    assert!(after.contains("run"));
    assert_eq!(core.history().undo_label(), Some("Edit animation"));
    let a = &core.doc().animations.items()[0];
    assert_eq!(
        (a.length, a.loop_mode, a.snapping, &*a.name),
        (1.0, LoopMode::Loop, 30, "run")
    );
    let same = Command::SetAnimation {
        animation: 0,
        name: Some("run".into()),
        length: None,
        loop_mode: Some(AnimationLoop::Loop),
        snapping: None,
    };
    assert_eq!(core.execute(same), CmdOutcome::Unchanged);
    for bad in [
        Command::SetAnimation {
            animation: 0,
            name: None,
            length: Some(-2.0),
            loop_mode: None,
            snapping: None,
        },
        Command::SetAnimation {
            animation: 0,
            name: None,
            length: None,
            loop_mode: None,
            snapping: Some(5),
        },
    ] {
        assert_eq!(core.execute(bad), CmdOutcome::Refused(Refusal::OutOfRange));
    }
    assert_eq!(
        core.execute(Command::SetAnimation {
            animation: 4,
            name: None,
            length: None,
            loop_mode: None,
            snapping: None
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn set_keyframe_inserts_replaces_and_is_one_undo_step() {
    let mut core = animated();
    round_trip(&mut core, key(1.0, 90.0));
    assert_eq!(core.history().undo_label(), Some("Set keyframe"));
    round_trip(&mut core, key(0.0, 0.0));
    assert_eq!(keys_of(&core), [(0.0, 0.0), (1.0, 90.0)]);
    // The same time replaces the value.
    round_trip(&mut core, key(1.0, 45.0));
    assert_eq!(keys_of(&core), [(0.0, 0.0), (1.0, 45.0)]);
    // The same value again changes nothing.
    assert_eq!(core.execute(key(1.0, 45.0)), CmdOutcome::Unchanged);
    // The interpolation of a keyframe changes without its value.
    run(
        &mut core,
        Command::SetKeyframe {
            animation: 0,
            group: Target::Group(0),
            channel: BoneChannel::Rotation,
            time: 1.0,
            value: KeyValue::nums([45.0, 0.0, 0.0]),
            interp: Some(KeyInterp::CatmullRom),
        },
    );
    let group = core.doc().groups.ids()[0];
    let track = core.doc().animations.items()[0].track(group).unwrap();
    assert_eq!(track.rotation[1].interp, Interp::CatmullRom);
}

#[test]
fn a_drag_of_a_value_is_one_undo_step() {
    let mut core = animated();
    let before = core.history().undo_len();
    for x in [10.0, 20.0, 30.0] {
        let outcome = core.execute_merged(key(0.5, x), 7);
        assert!(outcome.is_done(), "{outcome:?}");
    }
    assert_eq!(core.history().undo_len(), before + 1);
    assert_eq!(keys_of(&core), [(0.5, 30.0)]);
    assert!(core.undo().is_done());
    assert!(keys_of(&core).is_empty());
    assert!(core.redo().is_done());
    assert_eq!(keys_of(&core), [(0.5, 30.0)]);
}

#[test]
fn set_keyframe_refuses_a_missing_animation_group_and_bad_numbers() {
    let mut core = animated();
    let before = state(&core, true);
    let at = |animation, group, time: f32, x: f32| Command::SetKeyframe {
        animation,
        group,
        channel: BoneChannel::Scale,
        time,
        value: KeyValue::nums([x, 1.0, 1.0]),
        interp: None,
    };
    assert_eq!(
        core.execute(at(2, Target::Group(0), 0.0, 1.0)),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    assert_eq!(
        core.execute(at(0, Target::Group(5), 0.0, 1.0)),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    // An element is no bone.
    assert_eq!(
        core.execute(at(0, Target::Element(0), 0.0, 1.0)),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
    assert_eq!(
        core.execute(at(0, Target::Group(0), -1.0, 1.0)),
        CmdOutcome::Refused(Refusal::OutOfRange)
    );
    assert_eq!(
        core.execute(at(0, Target::Group(0), 0.0, f32::NAN)),
        CmdOutcome::Refused(Refusal::NotFinite)
    );
    assert_eq!(state(&core, true), before);
}

#[test]
fn remove_keyframes_takes_them_out_by_place() {
    let mut core = animated();
    for (t, x) in [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)] {
        run(&mut core, key(t, x));
    }
    let place = |index| KeyAt {
        group: Target::Group(0),
        channel: BoneChannel::Rotation,
        index,
    };
    round_trip(
        &mut core,
        Command::RemoveKeyframes {
            animation: 0,
            keys: vec![place(0), place(2)],
        },
    );
    assert_eq!(keys_of(&core), [(0.5, 2.0)]);
    assert_eq!(core.history().undo_label(), Some("Delete keyframes"));
    assert_eq!(
        core.execute(Command::RemoveKeyframes {
            animation: 0,
            keys: vec![place(7)]
        }),
        CmdOutcome::Unchanged
    );
    assert_eq!(
        core.execute(Command::RemoveKeyframes {
            animation: 0,
            keys: vec![KeyAt {
                group: Target::Element(0),
                ..place(0)
            }]
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn move_keyframes_shifts_them_and_the_moved_one_wins_on_equal_times() {
    let mut core = animated();
    for (t, x) in [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)] {
        run(&mut core, key(t, x));
    }
    let place = |index| KeyAt {
        group: Target::Group(0),
        channel: BoneChannel::Rotation,
        index,
    };
    round_trip(
        &mut core,
        Command::MoveKeyframes {
            animation: 0,
            keys: vec![place(1)],
            delta: 0.5,
        },
    );
    assert_eq!(keys_of(&core), [(0.0, 1.0), (1.0, 2.0)]);
    assert_eq!(core.history().undo_label(), Some("Move keyframes"));
    assert_eq!(
        core.execute(Command::MoveKeyframes {
            animation: 0,
            keys: vec![place(0)],
            delta: -1.0
        }),
        CmdOutcome::Refused(Refusal::Animation(AnimationError::OutOfRange))
    );
    assert_eq!(
        core.execute(Command::MoveKeyframes {
            animation: 0,
            keys: vec![place(0)],
            delta: f32::INFINITY
        }),
        CmdOutcome::Refused(Refusal::NotFinite)
    );
}

#[test]
fn deleting_a_group_takes_its_tracks_in_the_same_step() {
    let mut core = animated();
    run(&mut core, key(0.5, 10.0));
    // The group made by `animated` is still the selection.
    let before_steps = core.history().undo_len();
    let after = round_trip(&mut core, Command::Delete { targets: vec![] });
    assert_eq!(core.history().undo_len(), before_steps + 1);
    assert!(core.doc().animations.items()[0].tracks.is_empty());
    assert!(after.contains("tracks: []"));
    core.undo();
    assert_eq!(keys_of(&core), [(0.5, 10.0)]);
    check_invariants(core.doc(), core.selection()).unwrap();
}

#[test]
fn ungrouping_and_cutting_a_group_take_its_tracks_too() {
    let mut core = animated();
    run(&mut core, key(0.5, 10.0));
    round_trip(&mut core, Command::Ungroup { targets: vec![] });
    assert!(core.doc().animations.items()[0].tracks.is_empty());
    core.undo();
    assert_eq!(keys_of(&core), [(0.5, 10.0)]);
    assert!(
        core.selection().list.len() == 1,
        "the group is selected again"
    );

    round_trip(&mut core, Command::Cut);
    assert!(core.doc().animations.items()[0].tracks.is_empty());
}

fn place(index: usize) -> KeyAt {
    KeyAt {
        group: Target::Group(0),
        channel: BoneChannel::Rotation,
        index,
    }
}

#[test]
fn set_keyframe_keeps_an_expression_as_written_and_reads_the_old_json() {
    let mut core = animated();
    let text = |s: &str| KeyValue::Text(s.into());
    let command = Command::SetKeyframe {
        animation: 0,
        group: Target::Group(0),
        channel: BoneChannel::Position,
        time: 0.5,
        value: [
            KeyValue::Num(1.0),
            text("math.sin(q.anim_time)"),
            text("2.5"),
        ],
        interp: None,
    };
    round_trip(&mut core, command);
    let group = core.doc().groups.ids()[0];
    let key = &core.doc().animations.items()[0]
        .track(group)
        .unwrap()
        .position[0];
    assert_eq!(key.value[0].number(), Some(1.0));
    assert!(key.value[1].is_expression());
    assert_eq!(
        key.value[2],
        cuboid_model::Value::Num(2.5),
        "a number as text is a number"
    );
    for bad in [
        [KeyValue::Num(0.0), text(""), KeyValue::Num(0.0)],
        [
            KeyValue::Num(0.0),
            text(&"x".repeat(2000)),
            KeyValue::Num(0.0),
        ],
    ] {
        let command = Command::SetKeyframe {
            animation: 0,
            group: Target::Group(0),
            channel: BoneChannel::Position,
            time: 0.5,
            value: bad,
            interp: None,
        };
        assert_eq!(
            core.execute(command),
            CmdOutcome::Refused(Refusal::OutOfRange)
        );
    }
    // A command written before expressions existed, with plain numbers, still runs.
    let old = r#"{"SetKeyframe":{"animation":0,"group":{"Group":0},"channel":"Scale","time":1,"value":[1,2,3]}}"#;
    run(&mut core, serde_json::from_str(old).unwrap());
    let key = &core.doc().animations.items()[0].track(group).unwrap().scale[0];
    assert_eq!(key.value[2].number(), Some(3.0));
}

#[test]
fn set_keyframe_interp_changes_many_keyframes_in_one_undo_step() {
    let mut core = animated();
    for (t, x) in [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)] {
        run(&mut core, key(t, x));
    }
    let command = Command::SetKeyframeInterp {
        animation: 0,
        keys: vec![place(0), place(2)],
        interp: KeyInterp::Step,
    };
    round_trip(&mut core, command.clone());
    assert_eq!(core.history().undo_label(), Some("Set interpolation"));
    let group = core.doc().groups.ids()[0];
    let kinds: Vec<Interp> = core.doc().animations.items()[0]
        .track(group)
        .unwrap()
        .rotation
        .iter()
        .map(|k| k.interp)
        .collect();
    assert_eq!(kinds, [Interp::Step, Interp::Linear, Interp::Step]);
    assert_eq!(core.execute(command), CmdOutcome::Unchanged);
    assert_eq!(
        core.execute(Command::SetKeyframeInterp {
            animation: 0,
            keys: vec![place(0)],
            interp: KeyInterp::Step,
        }),
        CmdOutcome::Unchanged
    );
    assert_eq!(
        core.execute(Command::SetKeyframeInterp {
            animation: 5,
            keys: vec![place(0)],
            interp: KeyInterp::Step,
        }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn duplicate_animation_inserts_a_copy_after_it_and_undo_removes_it() {
    let mut core = animated();
    run(&mut core, key(0.5, 10.0));
    run(
        &mut core,
        Command::AddAnimation {
            name: "run".into(),
            length: 1.0,
        },
    );
    let after = round_trip(&mut core, Command::DuplicateAnimation { animation: 0 });
    assert!(after.contains("walk_copy"));
    assert_eq!(core.history().undo_label(), Some("Duplicate animation"));
    let names: Vec<String> = core
        .doc()
        .animations
        .items()
        .iter()
        .map(|a| a.name.to_string())
        .collect();
    assert_eq!(names, ["walk", "walk_copy", "run"]);
    let group = core.doc().groups.ids()[0];
    let copy = &core.doc().animations.items()[1];
    assert_eq!(copy.track(group).unwrap().rotation.len(), 1);
    assert_eq!(
        core.execute(Command::DuplicateAnimation { animation: 7 }),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn keyframes_are_copied_without_a_step_and_pasted_as_one() {
    let mut core = animated();
    for (t, x) in [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)] {
        run(&mut core, key(t, x));
    }
    let paste = |time| Command::PasteKeyframes {
        animation: 0,
        time,
        group: None,
    };
    assert_eq!(
        core.execute(paste(0.0)),
        CmdOutcome::Refused(Refusal::ClipboardEmpty)
    );
    let steps = core.history().undo_len();
    let copy = Command::CopyKeyframes {
        animation: 0,
        keys: vec![place(1), place(2)],
    };
    assert_eq!(run(&mut core, copy).affected, 2);
    assert_eq!(
        core.history().undo_len(),
        steps,
        "copying leaves no undo step"
    );
    assert!(core.clipboards().keyframes.is_some());

    round_trip(&mut core, paste(1.5));
    assert_eq!(core.history().undo_label(), Some("Paste keyframes"));
    assert_eq!(
        keys_of(&core),
        [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0), (1.5, 2.0), (2.0, 3.0)]
    );
    // The pasted end grows the animation; a paste out of range is refused whole.
    assert_eq!(core.doc().animations.items()[0].length, 2.0);
    assert_eq!(
        core.execute(paste(3600.0)),
        CmdOutcome::Refused(Refusal::Animation(AnimationError::OutOfRange))
    );
    // An unknown target bone is refused; a known one takes keys of a single bone.
    let command = Command::PasteKeyframes {
        animation: 0,
        time: 0.0,
        group: Some(Target::Element(0)),
    };
    assert_eq!(
        core.execute(command),
        CmdOutcome::Refused(Refusal::BadTarget)
    );
}

#[test]
fn a_removal_of_objects_that_are_no_group_leaves_the_tracks_alone() {
    let mut core = animated();
    run(&mut core, key(0.5, 10.0));
    select(&mut core, &[Target::Element(1)]);
    run(&mut core, Command::Delete { targets: vec![] });
    assert_eq!(keys_of(&core), [(0.5, 10.0)]);
}
