use cuboid_model::{
    Document, EditEnv, Group, GroupsBuilder, MAX_ANIMATIONS, ObjId, RenderPrefs, Selection,
    TextureList,
};
use cuboid_num::EngineRng;
use glam::Vec3;

use super::*;

struct Rig {
    doc: Document,
    sel: Selection,
    env: EditEnv,
    rng: EngineRng,
    bone: GroupId,
}

impl Rig {
    fn new() -> Self {
        let mut doc = Document::new(TextureList::default());
        let mut b = GroupsBuilder::new();
        let bone = b.add_group(Group::new("bone", Vec3::ZERO), None).unwrap();
        doc.groups = b.finish();
        Rig {
            doc,
            sel: Selection::default(),
            env: EditEnv {
                restrict_rotations: true,
                render: RenderPrefs::default(),
                code_page: 1252,
                allow_negative_size: false,
            },
            rng: EngineRng::seeded(3),
            bone,
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

    fn animation(&mut self) -> AnimationId {
        add(&mut self.cx(), "walk", 1.0).unwrap()
    }

    fn set(&mut self, id: AnimationId, channel: Channel, time: f32, x: f32) -> bool {
        let bone = self.bone;
        let value = [x, 0.0, 0.0].map(Value::Num);
        set_keyframe(&mut self.cx(), id, bone, channel, time, value, None).unwrap()
    }

    fn times(&self, id: AnimationId, channel: Channel) -> Vec<(f32, f32)> {
        self.doc
            .animations
            .get(id)
            .and_then(|a| a.track(self.bone))
            .map_or_else(Vec::new, |t| {
                t.channel(channel)
                    .iter()
                    .map(|k| (k.time, k.value[0].number().unwrap()))
                    .collect()
            })
    }

    fn key(&self, index: usize, channel: Channel) -> KeyRef {
        KeyRef {
            group: self.bone,
            channel,
            index,
        }
    }
}

#[test]
fn an_animation_is_added_changed_and_removed() {
    let mut rig = Rig::new();
    let id = add(&mut rig.cx(), "  \"walk\" ", 2.0).unwrap();
    assert_eq!(&*rig.doc.animations.get(id).unwrap().name, "  walk ");
    let unnamed = add(&mut rig.cx(), "   ", 1.0).unwrap();
    assert_eq!(&*rig.doc.animations.get(unnamed).unwrap().name, "animation");
    assert_eq!(
        add(&mut rig.cx(), "long", MAX_LENGTH + 1.0),
        Err(AnimationError::OutOfRange)
    );

    let fields = AnimationFields {
        name: Some("run".to_owned()),
        length: Some(0.5),
        loop_mode: Some(LoopMode::Loop),
        snapping: Some(60),
    };
    assert_eq!(set_fields(&mut rig.cx(), id, &fields), Ok(true));
    let a = rig.doc.animations.get(id).unwrap();
    assert_eq!(
        (&*a.name, a.length, a.loop_mode, a.snapping),
        ("run", 0.5, LoopMode::Loop, 60)
    );
    // The same values again change nothing.
    assert_eq!(set_fields(&mut rig.cx(), id, &fields), Ok(false));
    for bad in [
        AnimationFields {
            length: Some(f32::NAN),
            ..AnimationFields::default()
        },
        AnimationFields {
            snapping: Some(9),
            ..AnimationFields::default()
        },
        AnimationFields {
            snapping: Some(501),
            ..AnimationFields::default()
        },
    ] {
        assert_eq!(
            set_fields(&mut rig.cx(), id, &bad),
            Err(AnimationError::OutOfRange)
        );
    }
    assert_eq!(
        set_fields(&mut rig.cx(), ObjId::next(), &fields),
        Err(AnimationError::Missing)
    );

    assert!(remove(&mut rig.cx(), id));
    assert!(!remove(&mut rig.cx(), id));
    assert_eq!(rig.doc.animations.ids(), [unnamed]);
}

#[test]
fn the_number_of_animations_is_bounded() {
    let mut rig = Rig::new();
    for _ in 0..MAX_ANIMATIONS {
        add(&mut rig.cx(), "a", 1.0).unwrap();
    }
    assert_eq!(
        add(&mut rig.cx(), "one more", 1.0),
        Err(AnimationError::TooMany)
    );
}

#[test]
fn a_keyframe_is_inserted_in_order_and_replaced_at_the_same_time() {
    let mut rig = Rig::new();
    let id = rig.animation();
    assert!(rig.set(id, Channel::Rotation, 0.5, 5.0));
    assert!(rig.set(id, Channel::Rotation, 0.0, 1.0));
    assert!(rig.set(id, Channel::Rotation, 1.0, 9.0));
    assert_eq!(
        rig.times(id, Channel::Rotation),
        [(0.0, 1.0), (0.5, 5.0), (1.0, 9.0)]
    );
    // The same time within a hundred-thousandth replaces the value and keeps one keyframe.
    assert!(rig.set(id, Channel::Rotation, 0.500_01, 7.0));
    assert_eq!(
        rig.times(id, Channel::Rotation),
        [(0.0, 1.0), (0.5, 7.0), (1.0, 9.0)]
    );
    assert!(
        !rig.set(id, Channel::Rotation, 0.5, 7.0),
        "nothing to change"
    );
    // Another channel has its own keyframes.
    assert!(rig.set(id, Channel::Scale, 0.5, 2.0));
    assert_eq!(rig.times(id, Channel::Scale), [(0.5, 2.0)]);
    assert_eq!(rig.times(id, Channel::Rotation).len(), 3);

    // A jump goes when the value is set again; the interpolation changes on request.
    rig.doc
        .edit_animation(id, |a| {
            a.tracks[0].rotation[1].post = Some([Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)])
        })
        .unwrap();
    let bone = rig.bone;
    set_keyframe(
        &mut rig.cx(),
        id,
        bone,
        Channel::Rotation,
        0.5,
        [7.0, 0.0, 0.0].map(Value::Num),
        Some(Interp::Step),
    )
    .unwrap();
    let key = &rig.doc.animations.get(id).unwrap().tracks[0].rotation[1];
    assert!(key.post.is_none());
    assert_eq!(key.interp, Interp::Step);
    assert!(rig.doc.check_animations().is_ok());
}

#[test]
fn a_keyframe_after_the_end_grows_the_animation() {
    let mut rig = Rig::new();
    let id = rig.animation();
    assert!(rig.set(id, Channel::Position, 2.5, 1.0));
    assert!((rig.doc.animations.get(id).unwrap().length - 2.5).abs() < 1e-6);
    // A keyframe inside the animation leaves the length alone.
    assert!(rig.set(id, Channel::Position, 1.0, 1.0));
    assert!((rig.doc.animations.get(id).unwrap().length - 2.5).abs() < 1e-6);
}

#[test]
fn set_keyframe_refuses_what_does_not_exist_or_does_not_fit() {
    let mut rig = Rig::new();
    let id = rig.animation();
    let bone = rig.bone;
    let try_set = |rig: &mut Rig, id, group, time: f32, value: [f32; 3]| {
        set_keyframe(
            &mut rig.cx(),
            id,
            group,
            Channel::Rotation,
            time,
            value.map(Value::Num),
            None,
        )
    };
    assert_eq!(
        try_set(&mut rig, ObjId::next(), bone, 0.0, [0.0; 3]),
        Err(AnimationError::Missing)
    );
    assert_eq!(
        try_set(&mut rig, id, ObjId::next(), 0.0, [0.0; 3]),
        Err(AnimationError::Missing)
    );
    for time in [-0.1, f32::NAN, MAX_LENGTH + 1.0] {
        assert_eq!(
            try_set(&mut rig, id, bone, time, [0.0; 3]),
            Err(AnimationError::OutOfRange)
        );
    }
    assert_eq!(
        try_set(&mut rig, id, bone, 0.0, [0.0, f32::INFINITY, 0.0]),
        Err(AnimationError::OutOfRange)
    );
    assert!(rig.doc.animations.get(id).unwrap().tracks.is_empty());
}

#[test]
fn the_keyframes_of_a_document_are_bounded() {
    let mut rig = Rig::new();
    let id = rig.animation();
    let bone = rig.bone;
    rig.doc
        .edit_animation(id, |a| {
            a.length = 100.0;
            a.track_mut(bone).rotation = (0..MAX_KEYFRAMES)
                .map(|i| Keyframe::new(i as f32 / 1000.0, [0.0; 3]))
                .collect();
        })
        .unwrap();
    assert_eq!(
        set_keyframe(
            &mut rig.cx(),
            id,
            bone,
            Channel::Rotation,
            99.999_5,
            [1.0, 0.0, 0.0].map(Value::Num),
            None
        ),
        Err(AnimationError::TooManyKeyframes)
    );
    // Replacing one that is there is not an addition.
    assert_eq!(
        set_keyframe(
            &mut rig.cx(),
            id,
            bone,
            Channel::Rotation,
            0.5,
            [1.0, 0.0, 0.0].map(Value::Num),
            None
        ),
        Ok(true)
    );
}

#[test]
fn keyframes_are_removed_by_place_and_empty_tracks_go() {
    let mut rig = Rig::new();
    let id = rig.animation();
    for (t, v) in [(0.0, 1.0), (0.25, 2.0), (0.5, 3.0), (1.0, 4.0)] {
        rig.set(id, Channel::Rotation, t, v);
    }
    rig.set(id, Channel::Scale, 0.0, 1.0);
    let places = [
        rig.key(1, Channel::Rotation),
        rig.key(3, Channel::Rotation),
        rig.key(3, Channel::Rotation),
        rig.key(9, Channel::Rotation),
    ];
    let removed = remove_keyframes(&mut rig.cx(), id, &places);
    assert_eq!(removed, 2);
    assert_eq!(rig.times(id, Channel::Rotation), [(0.0, 1.0), (0.5, 3.0)]);
    assert_eq!(rig.times(id, Channel::Scale).len(), 1);

    let all = [
        rig.key(0, Channel::Rotation),
        rig.key(1, Channel::Rotation),
        rig.key(0, Channel::Scale),
    ];
    assert_eq!(remove_keyframes(&mut rig.cx(), id, &all), 3);
    assert!(rig.doc.animations.get(id).unwrap().tracks.is_empty());
    assert_eq!(remove_keyframes(&mut rig.cx(), id, &all), 0);
    assert_eq!(remove_keyframes(&mut rig.cx(), ObjId::next(), &all), 0);
}

#[test]
fn a_track_with_members_of_its_project_stays_without_keyframes() {
    let mut rig = Rig::new();
    let id = rig.animation();
    rig.set(id, Channel::Rotation, 0.0, 1.0);
    rig.doc
        .edit_animation(id, |a| {
            a.tracks[0].extra = Some(std::sync::Arc::from("{\"rotation_global\":true}"))
        })
        .unwrap();
    let key = rig.key(0, Channel::Rotation);
    assert_eq!(remove_keyframes(&mut rig.cx(), id, &[key]), 1);
    assert_eq!(rig.doc.animations.get(id).unwrap().tracks.len(), 1);
}

#[test]
fn moving_keyframes_shifts_their_times_and_merges_on_equal_times() {
    let mut rig = Rig::new();
    let id = rig.animation();
    for (t, v) in [(0.0, 1.0), (0.25, 2.0), (0.5, 3.0), (1.0, 4.0)] {
        rig.set(id, Channel::Rotation, t, v);
    }
    // 0.25 and 0.5 move by 0.5: the second lands on 1.0 and replaces that keyframe.
    let places = [rig.key(1, Channel::Rotation), rig.key(2, Channel::Rotation)];
    let moved = move_keyframes(&mut rig.cx(), id, &places, 0.5);
    assert_eq!(moved, Ok(2));
    assert_eq!(
        rig.times(id, Channel::Rotation),
        [(0.0, 1.0), (0.75, 2.0), (1.0, 3.0)]
    );
    assert!(rig.doc.check_animations().is_ok());

    // A move onto the keyframes of another selected pair keeps order and uniqueness.
    let places = [rig.key(0, Channel::Rotation), rig.key(1, Channel::Rotation)];
    let moved = move_keyframes(&mut rig.cx(), id, &places, 0.25);
    assert_eq!(moved, Ok(2));
    assert_eq!(
        rig.times(id, Channel::Rotation),
        [(0.25, 1.0), (1.0, 2.0)],
        "the moved 0.75 lands on the moved 0.0's neighbour 1.0 and the keyframe there goes"
    );
    assert!(rig.doc.check_animations().is_ok());
}

#[test]
fn moving_past_the_range_is_refused_and_past_the_end_grows_the_animation() {
    let mut rig = Rig::new();
    let id = rig.animation();
    rig.set(id, Channel::Rotation, 0.5, 1.0);
    let key = rig.key(0, Channel::Rotation);
    assert_eq!(
        move_keyframes(&mut rig.cx(), id, &[key], -0.6),
        Err(AnimationError::OutOfRange)
    );
    assert_eq!(
        move_keyframes(&mut rig.cx(), id, &[key], f32::NAN),
        Err(AnimationError::OutOfRange)
    );
    assert_eq!(
        move_keyframes(&mut rig.cx(), id, &[key], MAX_LENGTH),
        Err(AnimationError::OutOfRange)
    );
    assert_eq!(move_keyframes(&mut rig.cx(), id, &[key], 0.0), Ok(0));
    assert_eq!(move_keyframes(&mut rig.cx(), id, &[], 1.0), Ok(0));
    assert_eq!(move_keyframes(&mut rig.cx(), id, &[key], 2.0), Ok(1));
    assert_eq!(rig.times(id, Channel::Rotation), [(2.5, 1.0)]);
    assert!((rig.doc.animations.get(id).unwrap().length - 2.5).abs() < 1e-6);
}

#[test]
fn a_keyframe_keeps_an_expression_as_written_and_a_number_replaces_it() {
    let mut rig = Rig::new();
    let id = rig.animation();
    let bone = rig.bone;
    let value = [
        Value::Num(1.0),
        Value::from_text("math.sin(q.anim_time)"),
        Value::Num(0.0),
    ];
    set_keyframe(
        &mut rig.cx(),
        id,
        bone,
        Channel::Position,
        0.5,
        value.clone(),
        None,
    )
    .unwrap();
    let key = &rig.doc.animations.get(id).unwrap().tracks[0].position[0];
    assert_eq!(key.value, value);
    assert!(key.value[1].is_expression());
    // The same expression again changes nothing; a number in its place does.
    let again = set_keyframe(&mut rig.cx(), id, bone, Channel::Position, 0.5, value, None);
    assert_eq!(again, Ok(false));
    assert!(rig.set(id, Channel::Position, 0.5, 1.0));
    let key = &rig.doc.animations.get(id).unwrap().tracks[0].position[0];
    assert!(key.value.iter().all(|v| !v.is_expression()));
}

#[test]
fn the_interpolation_of_many_keyframes_changes_in_one_call() {
    let mut rig = Rig::new();
    let id = rig.animation();
    for (t, v) in [(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)] {
        rig.set(id, Channel::Rotation, t, v);
    }
    rig.set(id, Channel::Scale, 0.0, 1.0);
    let places = [
        rig.key(0, Channel::Rotation),
        rig.key(2, Channel::Rotation),
        rig.key(0, Channel::Scale),
        rig.key(9, Channel::Scale),
    ];
    assert_eq!(set_interp(&mut rig.cx(), id, &places, Interp::Step), Ok(3));
    let animation = rig.doc.animations.get(id).unwrap();
    let kinds: Vec<Interp> = animation.tracks[0]
        .rotation
        .iter()
        .map(|k| k.interp)
        .collect();
    assert_eq!(kinds, [Interp::Step, Interp::Linear, Interp::Step]);
    assert_eq!(set_interp(&mut rig.cx(), id, &places, Interp::Step), Ok(0));
    assert_eq!(
        set_interp(&mut rig.cx(), ObjId::next(), &places, Interp::Step),
        Err(AnimationError::Missing)
    );
}

#[test]
fn a_duplicate_is_a_deep_copy_after_the_original_without_identities() {
    let mut rig = Rig::new();
    let id = rig.animation();
    let other = add(&mut rig.cx(), "run", 1.0).unwrap();
    rig.set(id, Channel::Rotation, 0.5, 4.0);
    rig.doc
        .edit_animation(id, |a| {
            a.uuid = Some("a".to_owned());
            a.tracks[0].rotation[0].uuid = Some("k".to_owned());
            a.loop_mode = LoopMode::Loop;
        })
        .unwrap();
    let copy = duplicate(&mut rig.cx(), id).unwrap();
    assert_eq!(rig.doc.animations.ids(), [id, copy, other]);
    let (a, b) = (
        rig.doc.animations.get(id).unwrap(),
        rig.doc.animations.get(copy).unwrap(),
    );
    assert_eq!(&*b.name, "walk_copy");
    assert_eq!((b.length, b.loop_mode), (a.length, a.loop_mode));
    assert!(b.uuid.is_none() && b.tracks[0].rotation[0].uuid.is_none());
    assert_eq!(b.tracks[0].rotation[0].value, a.tracks[0].rotation[0].value);
    assert!(a.uuid.is_some(), "the original keeps its identity");
    assert_eq!(
        duplicate(&mut rig.cx(), ObjId::next()),
        Err(AnimationError::Missing)
    );
    assert!(rig.doc.check_animations().is_ok());
}

#[test]
fn pasted_keyframes_keep_their_spacing_start_at_the_time_and_replace_what_they_hit() {
    let mut rig = Rig::new();
    let id = rig.animation();
    for (t, v) in [(0.25, 1.0), (0.5, 2.0), (0.75, 3.0)] {
        rig.set(id, Channel::Rotation, t, v);
    }
    rig.doc
        .edit_animation(id, |a| a.tracks[0].rotation[1].uuid = Some("k".to_owned()))
        .unwrap();
    let places = [rig.key(1, Channel::Rotation), rig.key(2, Channel::Rotation)];
    let clip = copy_keys(&rig.doc, id, &places);
    assert_eq!(clip.len(), 2);
    assert!(clip[0].key.time < clip[1].key.time);

    // 0.5 and 0.75 go to 0.75 and 1.0: the first replaces the keyframe at 0.75.
    assert_eq!(paste_keys(&mut rig.cx(), id, &clip, 0.75, None), Ok(2));
    assert_eq!(
        rig.times(id, Channel::Rotation),
        [(0.25, 1.0), (0.5, 2.0), (0.75, 2.0), (1.0, 3.0)]
    );
    let pasted = &rig.doc.animations.get(id).unwrap().tracks[0].rotation[2];
    assert!(
        pasted.uuid.is_none(),
        "the identity of the source stays with the source"
    );

    // A paste past the end grows the animation, one beyond the range is refused.
    assert_eq!(paste_keys(&mut rig.cx(), id, &clip, 3.0, None), Ok(2));
    assert!((rig.doc.animations.get(id).unwrap().length - 3.25).abs() < 1e-6);
    assert_eq!(
        paste_keys(&mut rig.cx(), id, &clip, MAX_LENGTH, None),
        Err(AnimationError::OutOfRange)
    );
    assert_eq!(paste_keys(&mut rig.cx(), id, &[], 1.0, None), Ok(0));
    assert!(rig.doc.check_animations().is_ok());
}

#[test]
fn a_paste_from_one_bone_can_go_to_another_and_skips_bones_that_are_gone() {
    let mut rig = Rig::new();
    let mut b = GroupsBuilder::new();
    b.add_group(Group::new("bone", Vec3::ZERO), None).unwrap();
    let second = b.add_group(Group::new("tail", Vec3::ZERO), None).unwrap();
    rig.doc.groups = b.finish();
    let first = rig.doc.groups.ids()[0];
    rig.bone = first;
    let id = rig.animation();
    rig.set(id, Channel::Position, 0.0, 5.0);
    let clip = copy_keys(&rig.doc, id, &[rig.key(0, Channel::Position)]);
    assert_eq!(
        paste_keys(&mut rig.cx(), id, &clip, 0.5, Some(second)),
        Ok(1)
    );
    let animation = rig.doc.animations.get(id).unwrap();
    assert!((animation.track(second).unwrap().position[0].time - 0.5).abs() < 1e-6);
    assert_eq!(animation.track(first).unwrap().position.len(), 1);
    // Without a target bone the keyframe goes back to its own bone.
    assert_eq!(paste_keys(&mut rig.cx(), id, &clip, 0.25, None), Ok(1));
    assert_eq!(
        rig.doc
            .animations
            .get(id)
            .unwrap()
            .track(first)
            .unwrap()
            .position
            .len(),
        2
    );
    // A bone that is not in the model is refused as a target and skipped as a source.
    assert_eq!(
        paste_keys(&mut rig.cx(), id, &clip, 0.25, Some(ObjId::next())),
        Err(AnimationError::Missing)
    );
    let ghost = [CopiedKey {
        group: ObjId::next(),
        ..clip[0].clone()
    }];
    assert_eq!(paste_keys(&mut rig.cx(), id, &ghost, 0.25, None), Ok(0));
}
