//! Random command sequences: the document stays valid after every command, and undo walks back
//! through exactly the states the sequence went through.

mod common;

use common::{assert_same, state};
use cuboid_core::{
    AlignEdge, Axis, CmdOutcome, Command, Compat, Core, ElementKind, FaceId, SelectOp, Target,
};
use cuboid_model::check_invariants;
use proptest::prelude::*;

fn target() -> impl Strategy<Value = Target> {
    prop_oneof![
        6 => (0..5_usize).prop_map(Target::Element),
        1 => (0..2_usize).prop_map(Target::Mesh),
        1 => Just(Target::Blueprint),
        1 => Just(Target::ModelOrigin),
    ]
}

fn targets() -> impl Strategy<Value = Vec<Target>> {
    prop_oneof![
        2 => Just(Vec::new()),
        3 => prop::collection::vec(target(), 1..4),
    ]
}

/// Multiples of a quarter unit between -4 and 4.
fn unit() -> impl Strategy<Value = f32> {
    (-16..=16_i8).prop_map(|n| f32::from(n) * 0.25)
}

fn optional_vec() -> impl Strategy<Value = [Option<f32>; 3]> {
    [
        prop::option::of(unit()),
        prop::option::of(unit()),
        prop::option::of(unit()),
    ]
}

fn vec3() -> impl Strategy<Value = [f32; 3]> {
    [unit(), unit(), unit()]
}

fn degrees() -> impl Strategy<Value = f32> {
    (-16..=16_i8).prop_map(|n| f32::from(n) * 22.5)
}

fn axis() -> impl Strategy<Value = Axis> {
    prop_oneof![Just(Axis::X), Just(Axis::Y), Just(Axis::Z)]
}

fn label() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("texture".to_owned()),
        Just("particle".to_owned()),
        Just("wood".to_owned()),
        Just("stone".to_owned()),
    ]
}

fn command() -> impl Strategy<Value = Command> {
    let kind = prop_oneof![
        Just(ElementKind::Cube),
        Just(ElementKind::PlaneX),
        Just(ElementKind::PlaneY),
        Just(ElementKind::PlaneZ),
    ];
    let edit = prop_oneof![
        kind.prop_map(Command::AddElement),
        targets().prop_map(|targets| Command::Delete { targets }),
        targets().prop_map(|targets| Command::Duplicate { targets }),
        Just(Command::Cut),
        Just(Command::Copy),
        any::<bool>().prop_map(|at_blueprint| Command::Paste { at_blueprint }),
        (targets(), label()).prop_map(|(targets, name)| Command::Rename { targets, name }),
        (targets(), any::<bool>())
            .prop_map(|(targets, hidden)| Command::SetHidden { targets, hidden }),
        (targets(), any::<bool>())
            .prop_map(|(targets, locked)| Command::SetLocked { targets, locked }),
        Just(Command::ShowAll),
        Just(Command::UnlockAll),
        (targets(), vec3(), prop::option::of(Just(0.5_f32))).prop_map(|(targets, delta, snap)| {
            Command::MoveBy {
                targets,
                delta,
                snap,
            }
        }),
        (targets(), optional_vec())
            .prop_map(|(targets, value)| Command::SetPosition { targets, value }),
        (targets(), optional_vec())
            .prop_map(|(targets, value)| Command::SetSize { targets, value }),
        (
            targets(),
            [
                prop_oneof![Just(0.5_f32), Just(1.0), Just(2.0)],
                prop_oneof![Just(0.5_f32), Just(1.0), Just(2.0)],
                prop_oneof![Just(0.5_f32), Just(1.0), Just(2.0)],
            ]
        )
            .prop_map(|(targets, factor)| Command::ScaleBy {
                targets,
                factor,
                snap: None
            }),
        (targets(), [degrees(), degrees(), degrees()]).prop_map(|(targets, delta)| {
            Command::RotateBy {
                targets,
                delta,
                snap: None,
            }
        }),
        (
            targets(),
            [
                prop::option::of(degrees()),
                prop::option::of(degrees()),
                prop::option::of(degrees()),
            ]
        )
            .prop_map(|(targets, value)| Command::SetRotation { targets, value }),
        (targets(), axis()).prop_map(|(targets, axis)| Command::QuarterTurn { targets, axis }),
        targets().prop_map(|targets| Command::ResetRotation { targets }),
        targets().prop_map(|targets| Command::ResetPivot { targets }),
        (targets(), axis(), any::<bool>()).prop_map(|(targets, axis, copy)| Command::Mirror {
            targets,
            axis,
            copy
        }),
        (
            targets(),
            axis(),
            prop_oneof![
                Just(AlignEdge::Min),
                Just(AlignEdge::Center),
                Just(AlignEdge::Max)
            ]
        )
            .prop_map(|(targets, axis, edge)| Command::Align {
                targets,
                axis,
                edge
            }),
        (label(), 1..8_u32).prop_map(|(label, size)| Command::CreateTexture {
            label: Some(label),
            width: size,
            height: size,
            color: [9, 9, 9, 255]
        }),
        label().prop_map(|label| Command::RemoveTexture { label }),
        (
            targets(),
            label(),
            prop::collection::vec(
                prop_oneof![Just(FaceId::Up), Just(FaceId::East), Just(FaceId::North)],
                0..3
            )
        )
            .prop_map(|(targets, label, faces)| Command::AssignTexture {
                targets,
                label,
                faces
            }),
    ];
    let selection = prop_oneof![
        (
            targets(),
            prop_oneof![
                Just(SelectOp::Replace),
                Just(SelectOp::Add),
                Just(SelectOp::Toggle),
                Just(SelectOp::Remove)
            ]
        )
            .prop_map(|(targets, op)| Command::Select { targets, op }),
        Just(Command::SelectAll),
        Just(Command::SelectNone),
        Just(Command::SelectInverse),
    ];
    prop_oneof![5 => edit, 2 => selection]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Every state the sequence made is reached again by undoing back and by redoing forward,
    /// and the document is valid in all of them.
    #[test]
    fn undo_walks_back_through_the_states(cmds in prop::collection::vec(command(), 1..30), seed in 0..1000_u64) {
        let mut core = Core::new(Compat::port(), seed);
        // Start with a few cubes so that most targets exist.
        for _ in 0..3 {
            core.execute(Command::AddElement(ElementKind::Cube));
        }
        let base = core.history().undo_len();
        let mut states = vec![state(&core, false)];
        for cmd in cmds {
            let text = format!("{cmd:?}");
            let steps = core.history().undo_len();
            let outcome = core.execute(cmd);
            prop_assert!(check_invariants(core.doc(), core.selection()).is_ok(), "{text} broke the document");
            if let CmdOutcome::Failed(why) = &outcome {
                prop_assert!(false, "{text} failed: {why}");
            }
            let now = core.history().undo_len();
            if now == steps + 1 {
                prop_assert!(outcome.is_done(), "{text} left a step without being done");
                states.push(state(&core, false));
            } else {
                prop_assert_eq!(now, steps, "{} changed the history by more than one step", text);
                prop_assert!(!outcome.is_done() || text.starts_with("Select") || text.starts_with("Copy"),
                    "{} was done without an undo step", text);
                // Nothing that did not leave a step may have changed the document.
                let last = states.last().cloned().unwrap_or_default();
                assert_same(&state(&core, false), &last, &format!("{text} changed the document without a step"));
            }
        }
        let steps = core.history().undo_len() - base;
        prop_assert_eq!(steps + 1, states.len());
        for k in (0..steps).rev() {
            prop_assert!(core.undo().is_done());
            prop_assert!(check_invariants(core.doc(), core.selection()).is_ok());
            assert_same(&state(&core, false), &states[k], &format!("undo to step {k}"));
        }
        for (k, expected) in states.iter().enumerate().skip(1) {
            prop_assert!(core.redo().is_done());
            prop_assert!(check_invariants(core.doc(), core.selection()).is_ok());
            assert_same(&state(&core, false), expected, &format!("redo to step {k}"));
        }
    }
}

#[test]
fn the_step_cap_drops_the_oldest_steps_and_undo_stops_there() {
    let mut core = Core::new(Compat::port(), 3);
    core.set_history_limits(5, usize::MAX);
    for _ in 0..20 {
        assert!(
            core.execute(Command::AddElement(ElementKind::Cube))
                .is_done()
        );
    }
    assert_eq!(core.history().undo_len(), 5);
    for _ in 0..5 {
        assert!(core.undo().is_done());
    }
    assert!(!core.undo().is_done());
    assert_eq!(core.doc().elements.len(), 15);
}

#[test]
fn the_memory_cap_counts_what_a_step_owns_alone() {
    let mut core = Core::new(Compat::port(), 3);
    core.set_history_limits(1000, 300_000);
    let texture = |label: &str, side: u32| Command::CreateTexture {
        label: Some(label.into()),
        width: side,
        height: side,
        color: [1, 2, 3, 255],
    };
    // Steps that share their images with the document and with each other cost next to nothing.
    for n in 0..20 {
        assert!(core.execute(texture(&format!("t{n}"), 2)).is_done());
    }
    assert_eq!(core.history().undo_len(), 20);
    // A big image that leaves the document is owned by the step that can bring it back.
    assert!(core.execute(texture("big", 300)).is_done());
    assert!(
        core.execute(Command::RemoveTexture {
            label: "big".into()
        })
        .is_done()
    );
    assert!(core.history().bytes() > 300_000);
    // The cap is applied with the next step.
    assert!(core.execute(texture("last", 2)).is_done());
    assert!(
        core.history().undo_len() <= 2,
        "{}",
        core.history().undo_len()
    );
    assert!(core.undo().is_done());
}
