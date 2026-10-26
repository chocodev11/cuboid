//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::fmt::Write;

use cuboid_core::{CmdOutcome, Command, Compat, Core, Done, ElementKind, SelectOp, Target};
use cuboid_model::check_invariants;

/// A core with three cubes at x = 0, 4 and 8 (size 2), nothing selected, history cleared of
/// nothing: the set-up commands are ordinary undo steps.
pub fn scene() -> Core {
    let mut core = Core::new(Compat::port(), 42);
    for i in 0..3 {
        run(&mut core, Command::AddElement(ElementKind::Cube));
        run(
            &mut core,
            Command::SetPosition {
                targets: vec![Target::Element(i)],
                value: [Some(4.0 * i as f32), Some(0.0), Some(0.0)],
            },
        );
        run(
            &mut core,
            Command::SetSize {
                targets: vec![Target::Element(i)],
                value: [Some(2.0), Some(2.0), Some(2.0)],
            },
        );
    }
    run(&mut core, Command::SelectNone);
    core
}

/// Executes a command that must succeed and keeps the document valid.
pub fn run(core: &mut Core, cmd: Command) -> Done {
    let text = format!("{cmd:?}");
    let outcome = core.execute(cmd);
    let CmdOutcome::Done(done) = outcome else {
        panic!("{text} gave {outcome:?}");
    };
    check_invariants(core.doc(), core.selection()).expect("valid document");
    done
}

pub fn select(core: &mut Core, targets: &[Target]) {
    run(
        core,
        Command::Select {
            targets: targets.to_vec(),
            op: SelectOp::Replace,
        },
    );
}

/// Everything undo restores, as text. The selection is part of it on request: the selection
/// set comes back, the order of the selection list does not.
pub fn state(core: &Core, with_selection: bool) -> String {
    let doc = core.doc();
    let mut s = String::new();
    for el in doc.elements.items() {
        // Restore folds the UV corners, so the folded values are what stays the same.
        let faces: Vec<_> = el
            .faces
            .iter()
            .map(|f| {
                let uv = (f.uv.rot(), f.uv.tl(), f.uv.br());
                (
                    f.name as u8,
                    f.active,
                    f.tint,
                    f.cull.bits(),
                    f.texture.to_string(),
                    uv,
                )
            })
            .collect();
        writeln!(
            s,
            "{:?} {:?} {:?} {:?} {faces:?}",
            el.base, el.kind, el.color, el.flags
        )
        .unwrap();
    }
    for mesh in doc.meshes.items() {
        writeln!(s, "{mesh:?}").unwrap();
    }
    for (id, group) in doc.groups.ids().iter().zip(doc.groups.items()) {
        writeln!(s, "{id:?} {group:?}").unwrap();
    }
    for (id, animation) in doc.animations.ids().iter().zip(doc.animations.items()) {
        writeln!(s, "{id:?} {animation:?}").unwrap();
    }
    writeln!(s, "roots {:?}", doc.root_nodes()).unwrap();
    writeln!(s, "{:?}", doc.blueprint).unwrap();
    for t in &doc.textures.entries {
        writeln!(s, "{} {}x{}", t.label, t.texture.image.w, t.texture.image.h).unwrap();
    }
    writeln!(s, "{:?}", doc.props.general.origin).unwrap();
    writeln!(s, "{:?}", doc.display).unwrap();
    if with_selection {
        let mut at: Vec<_> = core
            .selection()
            .list
            .iter()
            .map(|&o| format!("{:?}", core.target_of(o)))
            .collect();
        at.sort();
        writeln!(s, "selected {at:?}").unwrap();
    }
    s
}

/// Asserts that two states are equal and shows the first difference when they are not.
pub fn assert_same(a: &str, b: &str, what: &str) {
    if a == b {
        return;
    }
    let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let show = |s: &str| {
        s.chars()
            .skip(at.saturating_sub(60))
            .take(160)
            .collect::<String>()
    };
    panic!("{what}\n  left: ...{}\n right: ...{}", show(a), show(b));
}

/// The element at a position of the list.
pub fn el(core: &Core, at: usize) -> cuboid_model::Element {
    core.doc().elements.items()[at].as_ref().clone()
}

/// Runs `cmd`, then undo and redo, and checks the document at each stage. Returns the state
/// after the command.
pub fn round_trip(core: &mut Core, cmd: Command) -> String {
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
