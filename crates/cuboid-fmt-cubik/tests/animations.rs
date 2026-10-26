//! The animations in the `cuboid.json` entry: they survive a save, the payload stays what it was
//! without them, and a tree that lost groups costs the tracks of those groups only.

mod common;

use std::io::Cursor;
use std::sync::Arc;

use common::*;
use cuboid_fmt_cubik::container;
use cuboid_fmt_cubik::{Limits, SaveOptions, load_bytes, save_bytes};
use cuboid_model::{
    Animation, Document, Group, Handles, Interp, Keyframe, LoopMode, ObjRef, Selection, Value,
};
use glam::Vec3;

fn payload_and_entry(file: Vec<u8>) -> (Vec<u8>, Option<Vec<u8>>) {
    let c = container::read(Cursor::new(file), &Limits::default()).unwrap();
    (c.payload, c.extension)
}

/// The sample document with two groups, "Arm" and "Body", and two animations on them.
fn animated() -> (Document, Selection) {
    let (mut doc, sel) = open(common::load_bytes(&full_payload()));
    let ids = doc.elements.ids().to_vec();
    let arm = doc
        .insert_group(
            Group::new("Arm", Vec3::new(1.0, 2.0, 3.0)),
            &[ObjRef::Element(ids[0]), ObjRef::Element(ids[1])],
        )
        .unwrap();
    let body = doc
        .insert_group(
            Group::new("Body", Vec3::ZERO),
            &[ObjRef::Group(arm), ObjRef::Element(ids[2])],
        )
        .unwrap();

    let mut walk = Animation::new("walk", 1.5);
    walk.loop_mode = LoopMode::Loop;
    walk.snapping = 30;
    walk.uuid = Some("walk-1".to_owned());
    walk.extra = Some(Arc::from(
        "{\"blend_weight\":\"1\",\"animators\":{\"effects\":{}}}",
    ));
    let mut curve = Keyframe::new(0.5, [1.0, 2.0, 3.0]);
    curve.interp = Interp::Bezier;
    curve.bezier = Some(Handles::default());
    curve.value[1] = Value::from_text("math.sin(query.anim_time * 90)");
    curve.post = Some([Value::Num(9.0), Value::Num(8.0), Value::from_text("7")]);
    curve.uuid = Some("k1".to_owned());
    let track = walk.track_mut(arm);
    track.rotation = vec![Keyframe::new(0.0, [0.0; 3]), curve];
    track.scale = vec![Keyframe::new(0.0, [1.0, 1.0, 1.0])];
    track.extra = Some(Arc::from("{\"rotation_global\":true}"));
    walk.track_mut(body).position = vec![Keyframe::new(1.0, [0.0, 4.0, 0.0])];
    let mut idle = Animation::new("idle", 2.0);
    idle.loop_mode = LoopMode::Hold;
    idle.track_mut(body).rotation = vec![Keyframe::new(1.0, [0.0, 90.0, 0.0])];
    doc.add_animation(walk).unwrap();
    doc.add_animation(idle).unwrap();
    (doc, sel)
}

fn save(doc: &Document, sel: &Selection, options: &SaveOptions) -> Vec<u8> {
    save_bytes(&snapshot(doc, sel), doc.retained.as_deref(), &[], options).unwrap()
}

/// An animation name, a group name and the keyframes of the three channels of a track.
type TrackRows = (String, String, Vec<Keyframe>, Vec<Keyframe>, Vec<Keyframe>);

/// The keyframes of every track of every animation, with the name of the group they animate.
fn tracks(doc: &Document) -> Vec<TrackRows> {
    doc.animations
        .items()
        .iter()
        .flat_map(|a| {
            a.tracks.iter().map(|t| {
                (
                    a.name.to_string(),
                    doc.groups.get(t.group).unwrap().name.to_string(),
                    t.rotation.clone(),
                    t.position.clone(),
                    t.scale.clone(),
                )
            })
        })
        .collect()
}

#[test]
fn the_animations_round_trip_and_the_payload_is_the_one_without_them() {
    let (doc, sel) = animated();
    let file = save(&doc, &sel, &SaveOptions::default());
    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let (back, _) = open(loaded);

    assert_eq!(back.animations.len(), 2);
    assert!(back.check_animations().is_ok());
    for (a, b) in doc.animations.items().iter().zip(back.animations.items()) {
        assert_eq!(a.name, b.name);
        assert_eq!(
            (
                a.length,
                a.loop_mode,
                a.snapping,
                a.overrides,
                &a.uuid,
                &a.extra
            ),
            (
                b.length,
                b.loop_mode,
                b.snapping,
                b.overrides,
                &b.uuid,
                &b.extra
            )
        );
    }
    assert_eq!(tracks(&doc), tracks(&back));
    let track = &back.animations.items()[0].tracks[0];
    assert_eq!(
        track.extra.as_deref(),
        Some("{\"rotation_global\":true}"),
        "the text a project kept goes along"
    );

    // The payload does not know the animations: a compatible save holds the same bytes.
    let compatible = SaveOptions {
        extension: false,
        ..SaveOptions::default()
    };
    let plain = save(&doc, &sel, &compatible);
    assert_eq!(payload_and_entry(file).0, payload_and_entry(plain).0);
}

#[test]
fn a_model_without_animations_writes_no_member_for_them() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let file = save(&doc, &sel, &SaveOptions::default());
    assert_eq!(payload_and_entry(file).1, None);

    // Groups alone: the entry has the tree and not the animations.
    let (mut grouped, sel) = animated();
    let all: Vec<_> = grouped.animations.ids().to_vec();
    grouped.remove_animations(&all.into_iter().collect());
    let entry = payload_and_entry(save(&grouped, &sel, &SaveOptions::default()))
        .1
        .unwrap();
    let text = String::from_utf8(entry).unwrap();
    assert!(text.contains("\"groups\""));
    assert!(!text.contains("\"animations\""));
}

#[test]
fn a_compatible_save_leaves_the_animations_out() {
    let (doc, sel) = animated();
    let compatible = SaveOptions {
        extension: false,
        ..SaveOptions::default()
    };
    let file = save(&doc, &sel, &compatible);
    let (back, _) = open(load_bytes(&file, &Limits::default()).unwrap());
    assert!(back.animations.is_empty());
}

#[test]
fn a_payload_changed_by_another_program_keeps_the_animations_of_the_groups_the_tree_keeps() {
    let (doc, sel) = animated();
    let file = save(&doc, &sel, &SaveOptions::default());
    let (_, entry) = payload_and_entry(file);

    // Another program moves the first two elements, which "Arm" holds, and appends a new one.
    let (mut changed, sel2) = open(common::load_bytes(&full_payload()));
    let all_ids = changed.elements.ids().to_vec();
    for &id in &all_ids[..2] {
        changed
            .elements
            .edit(id, &mut changed.journal)
            .unwrap()
            .base
            .p
            .y += 5.0;
    }
    let extra = changed.elements.items()[0].as_ref().clone();
    changed.elements.push(extra);
    let payload = save_payload(&changed, &sel2, &without_retention());
    let file = container::write(&payload, entry.as_deref()).unwrap();

    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert!(!loaded.warnings.is_empty());
    let (back, _) = open(loaded);
    // The groups themselves are in the tree whatever happened to the links of their cubes, so
    // the tracks of both stay.
    assert!(back.check_animations().is_ok());
    assert_eq!(back.animations.len(), 2);
    let names: Vec<String> = tracks(&back).into_iter().map(|t| t.1).collect();
    assert!(names.contains(&"Body".to_owned()));
}

#[test]
fn a_damaged_animation_member_is_reported_and_ignored() {
    let (doc, sel) = animated();
    let (payload, _) = payload_and_entry(save(&doc, &sel, &SaveOptions::default()));
    let entry = br#"{"version":1,"animations":{"animations":7}}"#;
    let file = container::write(&payload, Some(entry)).unwrap();
    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].contains("damaged"));
    let (back, _) = open(loaded);
    assert!(back.animations.is_empty());

    // Animations without a tree keep their settings and lose their tracks.
    let entry = br#"{"version":1,"animations":{"animations":[{"name":"walk","length":1.0,
        "snapping":24,"tracks":[{"group":"g0","rotation":[{"time":0.0,"value":[0,0,0]}]}]}]}}"#;
    let file = container::write(&payload, Some(entry)).unwrap();
    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    let (back, _) = open(loaded);
    assert_eq!(back.animations.len(), 1);
    assert!(back.animations.items()[0].tracks.is_empty());
}
