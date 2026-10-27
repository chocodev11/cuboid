//! Transactions of the core: a batch of commands as one undo step, the dry run, the checkpoint
//! restore and the codes of the refusals.

mod common;

use common::{assert_same, run, scene, select, state};
use cuboid_core::{
    CmdOutcome, Command, Compat, Core, DisplayContext, Done, ElementKind, Refusal, Target,
};
use cuboid_model::{
    AnimationError, CameraState, Group, GroupError, GroupsBuilder, MAX_GROUPS, ObjRef,
    check_invariants,
};
use glam::{Quat, Vec3};

/// The undo steps `scene()` leaves behind: three times add, position, size.
const SCENE_STEPS: usize = 9;

fn shift(at: usize) -> Command {
    Command::MoveBy {
        targets: vec![Target::Element(at)],
        delta: [1.0, 0.0, 0.0],
        snap: None,
    }
}

fn remove_texture(label: &str) -> Command {
    Command::RemoveTexture {
        label: label.to_owned(),
    }
}

/// The labels of the undo and of the redo entries.
fn history(core: &Core) -> (Vec<String>, Vec<String>) {
    let history = core.history();
    (
        history.undo_labels().map(str::to_owned).collect(),
        history.redo_labels().map(str::to_owned).collect(),
    )
}

/// The selection as the core keeps it: the order, the primary and the highlights. `state` sorts
/// the selection and leaves the primary out.
fn selection_of(core: &Core) -> (Vec<ObjRef>, Option<ObjRef>, Vec<ObjRef>) {
    let selection = core.selection();
    (
        selection.list.iter().copied().collect(),
        selection.primary,
        selection.highlighted.iter().copied().collect(),
    )
}

/// A core whose model holds as many groups as there may be, the first with a cube in it.
fn crowded() -> Core {
    let mut doc = scene().doc().clone();
    let mut groups = GroupsBuilder::new();
    let full = groups
        .add_group(Group::new("full", Vec3::ZERO), None)
        .expect("a group");
    let cube = ObjRef::Element(doc.elements.ids()[0]);
    groups.add_object(cube, Some(full)).expect("a cube");
    for _ in 1..MAX_GROUPS {
        groups
            .add_group(Group::new("empty", Vec3::ZERO), None)
            .expect("a group");
    }
    doc.groups = groups.finish();
    Core::with_document(doc, Compat::port(), 7)
}

#[test]
fn a_move_and_a_texture_removal_undo_and_redo_as_one_step() {
    let mut core = scene();
    let before = state(&core, true);
    let batch = vec![shift(0), remove_texture("particle")];
    let outcomes = core
        .execute_batch("Fix the model", batch)
        .expect("both run");
    assert!(outcomes.iter().all(CmdOutcome::is_done), "{outcomes:?}");
    let after = state(&core, true);
    assert_ne!(after, before);
    assert_eq!(core.doc().textures.entries.len(), 1);
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert_eq!(core.history().undo_label(), Some("Fix the model"));
    assert!(core.is_dirty());

    assert!(core.undo().is_done());
    assert_same(
        &state(&core, true),
        &before,
        "one undo brings back the move and the texture",
    );
    assert_eq!(core.doc().textures.entries.len(), 2);
    assert!(core.redo().is_done());
    assert_same(&state(&core, true), &after, "one redo does both again");
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
}

#[test]
fn a_refusal_mid_batch_leaves_the_document_and_the_history_as_they_were() {
    let mut core = scene();
    // An undone step leaves a redo entry, which a push would drop.
    run(&mut core, shift(2));
    assert!(core.undo().is_done());
    core.mark_saved();
    let before = state(&core, true);
    let (revision, entries) = (core.doc().revision, history(&core));
    assert_eq!(entries.1.len(), 1);

    let batch = vec![
        shift(0),
        remove_texture("particle"),
        remove_texture("missing"),
        shift(1),
    ];
    let refused = core
        .execute_batch("Fix", batch)
        .expect_err("the third refuses");
    assert_eq!(refused.index, 2);
    assert!(
        matches!(refused.outcome, CmdOutcome::Refused(Refusal::Op(_))),
        "{refused:?}"
    );
    check_invariants(core.doc(), core.selection()).expect("valid document");
    assert_same(
        &state(&core, true),
        &before,
        "the first two commands are undone",
    );
    assert_eq!(
        core.doc().revision,
        revision,
        "the stamp of the document goes back"
    );
    assert_eq!(history(&core), entries);
    assert!(!core.is_dirty(), "the unsaved-changes flag goes back");
    assert!(core.redo().is_done(), "the redo entry is still there");

    // A refusal of the first command has nothing to roll back.
    let before = state(&core, true);
    let entries = history(&core);
    let refused = core
        .execute_batch("Fix", vec![shift(99), shift(0)])
        .expect_err("no such object");
    assert_eq!(refused.index, 0);
    assert_eq!(refused.outcome, CmdOutcome::Refused(Refusal::BadTarget));
    assert_same(&state(&core, true), &before, "nothing changed");
    assert_eq!(history(&core), entries);
}

#[test]
fn a_batch_on_the_model_origin_snapshots_the_model_properties() {
    let mut core = scene();
    let before = state(&core, true);
    let origin = Command::SetPosition {
        targets: vec![Target::ModelOrigin],
        value: [Some(3.0), Some(1.0), Some(2.0)],
    };
    core.execute_batch("Move the origin", vec![origin.clone()])
        .expect("it runs");
    assert_ne!(state(&core, true), before);
    assert!(core.undo().is_done());
    assert_same(
        &state(&core, true),
        &before,
        "one undo moves the origin back",
    );

    let refused = core
        .execute_batch("Move the origin", vec![origin, remove_texture("missing")])
        .expect_err("the second refuses");
    assert_eq!(refused.index, 1);
    assert_same(&state(&core, true), &before, "the origin is where it was");
}

#[test]
fn a_batch_that_changes_nothing_leaves_no_step() {
    let mut core = scene();
    let same = Command::SetSize {
        targets: vec![Target::Element(0)],
        value: [Some(2.0), Some(2.0), Some(2.0)],
    };
    let entries = history(&core);
    let outcomes = core.execute_batch("Nothing", vec![same]).expect("runs");
    assert_eq!(outcomes, [CmdOutcome::Unchanged]);
    assert_eq!(core.execute_batch("Nothing", Vec::new()), Ok(Vec::new()));
    assert_eq!(history(&core), entries);
}

#[test]
fn selection_and_history_commands_are_refused_in_a_batch() {
    let mut core = scene();
    let before = state(&core, true);
    let entries = history(&core);
    for instant in [Command::Undo, Command::SelectAll] {
        let refused = core
            .execute_batch("Mixed", vec![shift(0), instant])
            .expect_err("not an edit");
        assert_eq!(refused.index, 1);
        assert!(matches!(
            refused.outcome,
            CmdOutcome::Refused(Refusal::Op(_))
        ));
        assert_same(&state(&core, true), &before, "the move is undone");
        assert_eq!(history(&core), entries, "the undo did not run");
        assert_eq!(
            core.status_text(),
            "only edit commands run in a batch",
            "the status names the refusal, not the move that was undone"
        );
    }
}

#[test]
fn a_batch_can_look_at_each_result_before_the_next_command() {
    let mut core = scene();
    let before = state(&core, true);

    let made: Result<Target, CmdOutcome> = core.batch("Add a lamp", |batch| {
        let created = batch.execute(Command::AddElement(ElementKind::Cube))?;
        let target = created.done().expect("a created element").created[0];
        let name = Command::Rename {
            targets: vec![target],
            name: "lamp".to_owned(),
        };
        batch.execute(name)?;
        Ok(target)
    });
    let target = made.expect("both run");
    assert_eq!(target, Target::Element(3));
    assert_eq!(&*core.doc().elements.items()[3].base.label, "lamp");
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert_eq!(core.history().undo_label(), Some("Add a lamp"));
    assert!(core.undo().is_done());
    assert_same(&state(&core, true), &before, "one undo removes the lamp");

    let entries = history(&core);
    let broken = core.batch("Add and break", |batch| {
        batch.execute(Command::AddElement(ElementKind::Cube))?;
        batch.execute(shift(99))?;
        Ok(())
    });
    assert_eq!(broken, Err(CmdOutcome::Refused(Refusal::BadTarget)));
    assert_same(&state(&core, true), &before, "the new cube is gone");
    assert_eq!(history(&core), entries);
}

#[test]
fn a_closure_that_is_refused_leaves_the_document_the_revision_and_the_saved_state() {
    let mut core = scene();
    core.mark_saved();
    let before = state(&core, true);
    let (revision, entries) = (core.doc().revision, history(&core));

    // The batch finds out by itself that the texture list changed.
    let broken = core.batch("Remove and break", |batch| {
        batch.execute(remove_texture("particle"))?;
        batch.execute(shift(99))?;
        Ok(())
    });
    assert_eq!(broken, Err(CmdOutcome::Refused(Refusal::BadTarget)));
    assert_same(&state(&core, true), &before, "the texture is back");
    assert_eq!(core.doc().revision, revision);
    assert!(!core.is_dirty());
    assert_eq!(history(&core), entries);
}

#[test]
fn one_undo_restores_everything_a_closure_changed() {
    let mut core = scene();
    let before = state(&core, true);
    let made: Result<(), CmdOutcome> = core.batch("Mixed", |batch| {
        batch.execute(remove_texture("particle"))?;
        batch.execute(shift(0))?;
        batch.execute(Command::AddElement(ElementKind::Cube))?;
        Ok(())
    });
    made.expect("all three run");
    let after = state(&core, true);
    assert_eq!(core.doc().textures.entries.len(), 1);
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 1);
    assert_eq!(core.history().undo_label(), Some("Mixed"));

    assert!(core.undo().is_done());
    assert_same(
        &state(&core, true),
        &before,
        "one undo brings back the texture, the position and drops the cube",
    );
    assert!(core.redo().is_done());
    assert_same(&state(&core, true), &after, "one redo does all three again");
}

#[test]
fn a_closure_that_goes_on_after_a_refusal_commits_only_the_commands_that_succeeded() {
    // A copy of the group makes the cube first and is refused at the group count.
    let mut core = crowded();
    let before = state(&core, true);
    let steps = core.history().undo_len();
    let copy = Command::Duplicate {
        targets: vec![Target::Group(0)],
    };

    let made: Result<(), CmdOutcome> = core.batch("Shift twice", |batch| {
        batch.execute(shift(1))?;
        let refused = batch.execute(copy);
        assert!(
            matches!(refused, Err(CmdOutcome::Refused(Refusal::Op(_)))),
            "{refused:?}"
        );
        assert_eq!(batch.core().doc().elements.len(), 3, "the cube is gone");
        batch.execute(shift(2))?;
        Ok(())
    });
    made.expect("the closure went on");
    assert_eq!(core.doc().elements.len(), 3);
    assert_eq!(core.history().undo_len(), steps + 1);
    assert_eq!(core.history().undo_label(), Some("Shift twice"));

    assert!(core.undo().is_done());
    assert_same(&state(&core, true), &before, "one undo brings back both");
}

#[test]
fn a_closure_with_no_command_done_leaves_no_step() {
    let mut core = scene();
    core.mark_saved();
    let entries = history(&core);
    let made: Result<(), CmdOutcome> = core.batch("Nothing", |batch| {
        assert!(batch.execute(shift(99)).is_err());
        Ok(())
    });
    made.expect("the closure went on");
    assert_eq!(history(&core), entries);
    assert!(!core.is_dirty());
}

#[test]
fn a_rollback_gives_back_the_selection_with_its_order_and_primary() {
    let mut core = scene();
    let targets = [Target::ModelOrigin, Target::Element(2), Target::Element(0)];
    select(&mut core, &targets);
    let selection = selection_of(&core);
    assert_eq!(selection.1, Some(ObjRef::ModelOrigin));
    let before = state(&core, true);

    let recolor = Command::SetColor {
        targets: vec![Target::Element(0)],
        color: [0.5, 0.5, 0.5],
    };
    let refused = core
        .execute_batch("Recolor", vec![recolor, remove_texture("missing")])
        .expect_err("the second refuses");
    assert_eq!(refused.index, 1);
    assert_eq!(selection_of(&core), selection);
    assert_same(&state(&core, true), &before, "the model is as it was");
}

#[test]
fn a_rollback_gives_back_the_clipboards() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    let before = state(&core, true);

    let refused = core
        .execute_batch("Cut", vec![Command::Cut, remove_texture("missing")])
        .expect_err("the second refuses");
    assert_eq!(refused.index, 1);
    assert!(core.clipboards().objects.is_none(), "the cut left no copy");
    assert_same(&state(&core, true), &before, "the cube is back");
}

#[test]
fn a_rollback_keeps_a_display_scale_of_zero() {
    let mut core = scene();
    let head = Command::SetDisplaySlot {
        slot: DisplayContext::Head,
        enabled: Some(true),
        rotation: None,
        translation: None,
        scale: Some([0.0, 1.0, 1.0]),
        preset: None,
    };
    run(&mut core, head);
    core.mark_saved();
    let before = state(&core, true);
    let revision = core.doc().revision;

    let gui = Command::SetDisplaySlot {
        slot: DisplayContext::Gui,
        enabled: Some(true),
        rotation: None,
        translation: Some([1.0, 2.0, 3.0]),
        scale: None,
        preset: None,
    };
    let refused = core
        .execute_batch("Edit the gui slot", vec![gui, remove_texture("missing")])
        .expect_err("the second refuses");
    assert_eq!(refused.index, 1);
    assert_same(&state(&core, true), &before, "the head scale stays zero");
    assert_eq!(core.doc().revision, revision);
    assert!(!core.is_dirty());
}

#[test]
fn a_dry_run_leaves_the_real_core_alone_and_shows_what_the_real_run_does() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    let batch = || {
        let on_selection = Command::MoveBy {
            targets: Vec::new(),
            delta: [0.0, 3.0, 0.0],
            snap: None,
        };
        vec![on_selection, remove_texture("particle")]
    };
    let before = state(&core, true);
    let (revision, entries) = (core.doc().revision, history(&core));
    let dirty = core.is_dirty();

    let mut scratch = core.scratch();
    let outcomes = scratch
        .execute_batch("Dry run", batch())
        .expect("it would run");
    assert!(outcomes.iter().all(CmdOutcome::is_done), "{outcomes:?}");
    assert_ne!(state(&scratch, true), before);
    assert_same(&state(&core, true), &before, "the real model is untouched");
    assert_eq!(core.doc().revision, revision);
    assert_eq!(history(&core), entries);
    assert_eq!(core.is_dirty(), dirty);

    let refused = core
        .scratch()
        .execute_batch("Dry run", vec![shift(0), remove_texture("missing")])
        .expect_err("the second refuses");
    assert_eq!(refused.index, 1);
    assert_eq!(core.doc().revision, revision);
    assert_eq!(history(&core), entries);

    core.execute_batch("Fix", batch()).expect("it runs");
    assert_same(
        &state(&core, true),
        &state(&scratch, true),
        "the dry run showed the result",
    );
}

#[test]
fn restoring_a_checkpoint_is_one_undo_step() {
    let mut core = scene();
    let checkpoint = core.checkpoint();
    let at_checkpoint = state(&core, true);

    run(&mut core, shift(0));
    run(&mut core, remove_texture("particle"));
    select(&mut core, &[Target::Element(0), Target::Element(1)]);
    let group = Command::Group {
        targets: Vec::new(),
        name: "arm".to_owned(),
    };
    run(&mut core, group);
    let walk = Command::AddAnimation {
        name: "walk".to_owned(),
        length: 1.0,
    };
    run(&mut core, walk);
    let head = Command::SetDisplaySlot {
        slot: DisplayContext::Head,
        enabled: Some(true),
        rotation: None,
        translation: Some([0.0, 13.0, 7.0]),
        scale: None,
        preset: None,
    };
    run(&mut core, head);
    let edited = state(&core, true);
    let steps = core.history().undo_len();

    let outcome = core.restore_checkpoint("Restore checkpoint", &checkpoint);
    assert!(outcome.is_done(), "{outcome:?}");
    check_invariants(core.doc(), core.selection()).expect("valid document");
    assert_same(&state(&core, true), &at_checkpoint, "the model is back");
    assert_eq!(core.history().undo_len(), steps + 1);
    assert_eq!(core.history().undo_label(), Some("Restore checkpoint"));
    assert_eq!(core.status_text(), "Restore checkpoint");
    assert!(core.is_dirty());

    assert!(core.undo().is_done());
    assert_same(
        &state(&core, true),
        &edited,
        "one undo returns to the edits",
    );
    assert!(core.redo().is_done());
    assert_same(
        &state(&core, true),
        &at_checkpoint,
        "one redo restores again",
    );
}

#[test]
fn restoring_the_model_as_it_is_changes_nothing() {
    let mut core = scene();
    core.mark_saved();
    let checkpoint = core.checkpoint();
    let entries = history(&core);

    let outcome = core.restore_checkpoint("Restore checkpoint", &checkpoint);
    assert_eq!(outcome, CmdOutcome::Unchanged);
    assert_eq!(history(&core), entries);
    assert!(!core.is_dirty());

    run(&mut core, shift(0));
    let outcome = core.restore_checkpoint("Restore checkpoint", &checkpoint);
    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(core.history().undo_len(), SCENE_STEPS + 2);
}

#[test]
fn a_restore_leaves_the_cameras_of_the_views_alone() {
    let camera_at = |x: f32| CameraState {
        pos: Vec3::new(x, 2.0, 3.0),
        rot: Quat::IDENTITY,
        ortho: false,
        ortho_size: 4.0,
        fov_deg: 50.0,
    };
    let mut core = scene();
    core.set_cameras([Some(camera_at(1.0)), None, None, None]);
    let checkpoint = core.checkpoint();
    run(&mut core, shift(0));
    let moved = [Some(camera_at(5.0)), None, None, None];
    core.set_cameras(moved);

    let outcome = core.restore_checkpoint("Restore checkpoint", &checkpoint);
    assert!(outcome.is_done(), "{outcome:?}");
    assert_eq!(core.doc().view.cameras, moved);
    assert!(core.undo().is_done());
    assert_eq!(core.doc().view.cameras, moved);
}

/// The code each kind of refusal must have. The matches have no wildcard, so a new kind stops
/// the build here until it is given its code (and a row of `kinds` below).
fn expected_code(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::NothingSelected => "E_NOTHING_SELECTED",
        Refusal::NoElements => "E_NO_ELEMENTS",
        Refusal::BadTarget => "E_BAD_TARGET",
        Refusal::NothingToUndo => "E_NOTHING_TO_UNDO",
        Refusal::NothingToRedo => "E_NOTHING_TO_REDO",
        Refusal::ClipboardEmpty => "E_CLIPBOARD_EMPTY",
        Refusal::NotDeletable => "E_NOT_DELETABLE",
        Refusal::NotFinite => "E_NOT_FINITE",
        Refusal::OutOfRange => "E_OUT_OF_RANGE",
        Refusal::RotationRestricted => "E_ROTATION_RESTRICTED",
        Refusal::NoGroups => "E_NO_GROUPS",
        Refusal::Group(kind) => match kind {
            GroupError::TooMany => "E_GROUP_TOO_MANY",
            GroupError::TooDeep => "E_GROUP_TOO_DEEP",
            GroupError::Cycle => "E_GROUP_CYCLE",
            GroupError::Missing => "E_GROUP_MISSING",
            GroupError::Attached => "E_GROUP_ATTACHED",
        },
        Refusal::Animation(kind) => match kind {
            AnimationError::TooMany => "E_ANIMATION_TOO_MANY",
            AnimationError::TooManyKeyframes => "E_ANIMATION_TOO_MANY_KEYFRAMES",
            AnimationError::Missing => "E_ANIMATION_MISSING",
            AnimationError::OutOfRange => "E_ANIMATION_OUT_OF_RANGE",
        },
        Refusal::Op(_) => "E_OP",
    }
}

#[test]
fn every_kind_of_refusal_has_its_own_stable_code() {
    let kinds = [
        Refusal::NothingSelected,
        Refusal::NoElements,
        Refusal::BadTarget,
        Refusal::NothingToUndo,
        Refusal::NothingToRedo,
        Refusal::ClipboardEmpty,
        Refusal::NotDeletable,
        Refusal::NotFinite,
        Refusal::OutOfRange,
        Refusal::RotationRestricted,
        Refusal::NoGroups,
        Refusal::Group(GroupError::TooMany),
        Refusal::Group(GroupError::TooDeep),
        Refusal::Group(GroupError::Cycle),
        Refusal::Group(GroupError::Missing),
        Refusal::Group(GroupError::Attached),
        Refusal::Animation(AnimationError::TooMany),
        Refusal::Animation(AnimationError::TooManyKeyframes),
        Refusal::Animation(AnimationError::Missing),
        Refusal::Animation(AnimationError::OutOfRange),
        Refusal::Op("any message".to_owned()),
    ];
    for refusal in &kinds {
        assert_eq!(refusal.code(), expected_code(refusal), "{refusal}");
    }
    // The outcomes that are no refusal have a code in the same style, and nothing else has one.
    let not_yet = CmdOutcome::NotYet("later");
    let failed = CmdOutcome::Failed("it broke".to_owned());
    assert_eq!(not_yet.code(), Some("E_NOT_YET"));
    assert_eq!(failed.code(), Some("E_FAILED"));
    assert_eq!(CmdOutcome::Unchanged.code(), None);
    assert_eq!(CmdOutcome::Done(Done::default()).code(), None);
    assert_eq!(
        CmdOutcome::Refused(Refusal::BadTarget).code(),
        Some("E_BAD_TARGET")
    );
    let mut codes: Vec<&str> = kinds.iter().map(Refusal::code).collect();
    codes.extend([not_yet.code(), failed.code()].into_iter().flatten());
    let total = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), total, "no two kinds share a code");
    for code in codes {
        let name = code.strip_prefix("E_").expect("the E_ style");
        assert!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'),
            "{code}"
        );
    }

    // The code is the one of the refusal a command gets.
    let mut core = scene();
    let CmdOutcome::Refused(refusal) = core.execute(shift(99)) else {
        panic!("no such object");
    };
    assert_eq!(refusal.code(), "E_BAD_TARGET");
}
