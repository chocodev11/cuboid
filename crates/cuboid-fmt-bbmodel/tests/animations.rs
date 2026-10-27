//! Animations in `.bbmodel` projects: read, write, read again (animation A2 5). The projects are
//! made here: a hand-written one with every kind of keyframe, and random ones for the property that
//! an unchanged project writes the same `animations` as JSON values.

mod common;

use std::path::Path;

use common::{NoFiles, import_json, open, text_of};
use cuboid_fmt_bbmodel::{Notice, export, import};
use cuboid_model::invariants::check_loaded_invariants;
use cuboid_model::{
    Animation, Document, Interp, Keyframe, LoopMode, MAX_ANIMATIONS, MAX_KEYFRAMES, Value, pose,
};
use proptest::prelude::*;
use rustc_hash::FxHashSet;
use serde_json::{Value as Json, json};

fn read(project: &Json) -> Document {
    open(import_json(project))
}

fn write(doc: &Document) -> Json {
    let out = export(doc).expect("exports");
    serde_json::from_slice(&out.bytes).expect("the output is JSON")
}

/// A project with a chain of two bones, a third bone beside them and a cube in the first.
fn project(animations: Json) -> Json {
    json!({
        "meta": {"format_version": "4.10", "model_format": "free", "box_uv": false},
        "name": "rig",
        "resolution": {"width": 16, "height": 16},
        "elements": [{"name": "c", "from": [0, 0, 0], "to": [2, 2, 2], "origin": [0, 0, 0],
            "faces": {}, "type": "cube", "uuid": "e1"}],
        "outliner": [
            {"name": "root", "origin": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0],
             "uuid": "g1", "export": true, "visibility": true, "locked": false,
             "children": [
                {"name": "arm", "origin": [4.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0],
                 "uuid": "g2", "export": true, "visibility": true, "locked": false,
                 "children": ["e1"]}]},
            {"name": "tail", "origin": [0.0, 4.0, 0.0], "rotation": [0.0, 0.0, 0.0],
             "uuid": "g3", "export": true, "visibility": true, "locked": false, "children": []}
        ],
        "animations": animations,
    })
}

fn key(channel: &str, uuid: &str, time: Json, interp: &str, points: Json) -> Json {
    let mut out = json!({
        "channel": channel, "data_points": points, "uuid": uuid, "time": time,
        "color": -1, "interpolation": interp,
    });
    if channel == "scale" {
        out["uniform"] = json!(false);
    }
    out
}

fn point(x: Json, y: Json, z: Json) -> Json {
    json!({"x": x, "y": y, "z": z})
}

/// Animations that use everything the format has, in the form the writer gives them.
fn full() -> Json {
    let mut bezier = key(
        "position",
        "k5",
        json!(0.5),
        "bezier",
        json!([point(json!("0"), json!("1.5"), json!("0"))]),
    );
    bezier["bezier_linked"] = json!(false);
    bezier["bezier_left_time"] = json!([-0.25, -0.1, -0.1]);
    bezier["bezier_left_value"] = json!([0, 1.5, 0]);
    bezier["bezier_right_time"] = json!([0.25, 0.1, 0.1]);
    bezier["bezier_right_value"] = json!([0, -1.5, 0]);
    let mut jump = key(
        "rotation",
        "k3",
        json!(1),
        "linear",
        json!([
            point(json!("10"), json!("0"), json!("0")),
            point(json!("45"), json!("0"), json!("0"))
        ]),
    );
    jump["color"] = json!(3);
    let mut uniform = key(
        "scale",
        "k7",
        json!(1),
        "step",
        json!([point(json!(2), json!(2), json!(2))]),
    );
    uniform["uniform"] = json!(true);
    json!([
        {
            "uuid": "a1", "name": "walk", "loop": "loop", "override": false, "length": 1.5,
            "snapping": 24, "selected": true, "anim_time_update": "", "blend_weight": "",
            "start_delay": "", "loop_delay": "",
            "markers": [{"color": 0, "time": 0.25}],
            "animators": {
                "g1": {
                    "name": "root", "type": "bone", "rotation_global": false,
                    "keyframes": [
                        key("rotation", "k1", json!(0), "linear",
                            json!([point(json!("0"), json!("0"), json!("0"))])),
                        key("rotation", "k2", json!(0.5), "catmullrom",
                            json!([point(json!("15"), json!("math.sin(query.anim_time * 90)"), json!(2.5))])),
                        jump,
                        bezier,
                        key("position", "k6", json!(1.5), "linear",
                            json!([point(json!("0"), json!("0"), json!("1"))])),
                        key("scale", "k8", json!(0), "linear",
                            json!([point(json!("1"), json!("1"), json!("1"))])),
                        uniform,
                    ],
                },
                "g2": {
                    "name": "arm", "type": "bone",
                    "keyframes": [
                        key("rotation", "k9", json!(0.25), "linear",
                            json!([point(json!("0"), json!("0"), json!("-30"))])),
                    ],
                },
                "g3": {"name": "tail", "type": "bone", "quaternion_interpolation": true},
                "effects": {
                    "name": "Effects", "type": "effect",
                    "keyframes": [{"channel": "sound", "time": 0.5, "uuid": "s1",
                        "data_points": [{"effect": "minecraft:step", "file": ""}]}],
                },
                "11111111-2222-3333-4444-555555555555": {
                    "name": "gone", "type": "bone",
                    "keyframes": [key("rotation", "k10", json!(0), "linear",
                        json!([point(json!("1"), json!("2"), json!("3"))]))],
                },
            },
        },
        {
            "uuid": "a2", "name": "idle", "loop": "hold", "override": true, "length": 0,
            "snapping": 60, "path": "idle.animation.json",
        },
    ])
}

#[test]
fn an_unchanged_project_writes_the_same_animations() {
    let project = project(full());
    let doc = read(&project);
    assert_eq!(doc.animations.len(), 2);
    let out = write(&doc);
    assert_eq!(
        out["animations"],
        project["animations"],
        "{}",
        diff(&out["animations"], &project["animations"])
    );
    // A second round changes nothing either.
    let again = write(&open(
        import(&text_of(&out), Path::new("dir/Model.bbmodel"), &NoFiles).expect("imports"),
    ));
    assert_eq!(again["animations"], project["animations"]);
    assert_eq!(again["outliner"], out["outliner"]);
}

/// Where two values first differ, for a failure message.
fn diff(a: &Json, b: &Json) -> String {
    match (a, b) {
        (Json::Object(x), Json::Object(y)) => {
            for (key, left) in x {
                match y.get(key) {
                    Some(right) if left != right => return format!("{key}: {}", diff(left, right)),
                    None => return format!("{key} is missing on the right"),
                    _ => {}
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map_or_else(String::new, |k| format!("{k} is missing on the left"))
        }
        (Json::Array(x), Json::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .position(|(l, r)| l != r)
            .map_or_else(String::new, |at| format!("[{at}] {}", diff(&x[at], &y[at]))),
        _ => format!("{a} against {b}"),
    }
}

#[test]
fn the_model_holds_the_bones_the_keyframes_and_the_settings() {
    let doc = read(&project(full()));
    let ids = doc.groups.ids();
    let walk = &doc.animations.items()[0];
    assert_eq!(&*walk.name, "walk");
    assert_eq!(walk.loop_mode, LoopMode::Loop);
    assert_eq!((walk.length, walk.snapping), (1.5, 24));
    assert_eq!(walk.uuid.as_deref(), Some("a1"));
    // The two bones of the chain and the empty animator of the tail are tracks; the effects and
    // the animator of a uuid the model does not have are not.
    assert_eq!(walk.tracks.len(), 3);
    let root = walk.track(ids[0]).expect("the root bone");
    assert_eq!(root.rotation.len(), 3);
    assert_eq!(root.position.len(), 2);
    assert_eq!(root.scale.len(), 2);
    assert!(walk.track(ids[2]).expect("the tail").is_empty());
    assert!(walk.extra.as_deref().unwrap().contains("effects"));
    assert!(walk.extra.as_deref().unwrap().contains("anim_time_update"));

    // The project is of version 4.10: the model has the X and Y of its rotations the other way.
    let curve = &root.rotation[1];
    assert_eq!(curve.interp, Interp::CatmullRom);
    assert_eq!(curve.value[0].number(), Some(-15.0));
    assert!(curve.value[1].is_expression());
    assert!(matches!(&curve.value[1], Value::Text { text, .. } if text.contains("math.sin")));
    assert_eq!(curve.value[2], Value::Num(2.5));
    let jump = &root.rotation[2];
    assert_eq!(jump.time, 1.0);
    assert_eq!(jump.value[0].number(), Some(-10.0));
    assert_eq!(jump.post.as_ref().unwrap()[0].number(), Some(-45.0));
    assert_eq!(jump.color, 3);
    let bezier = &root.position[0];
    assert_eq!(bezier.interp, Interp::Bezier);
    let handles = bezier.bezier.as_ref().unwrap();
    assert!(!handles.linked);
    assert_eq!(handles.left_time, [-0.25, -0.1, -0.1]);
    assert_eq!(handles.right_value, [0.0, -1.5, 0.0]);
    assert_eq!(root.scale[1].interp, Interp::Step);
    assert_eq!(root.scale[1].uniform, Some(true));
    assert_eq!(root.scale[0].uniform, Some(false));

    let idle = &doc.animations.items()[1];
    assert_eq!(
        (idle.loop_mode, idle.overrides, idle.length, idle.snapping),
        (LoopMode::Hold, true, 0.0, 60)
    );
    assert!(check_loaded_invariants(&doc, &Default::default()).is_ok());
}

#[test]
fn values_are_clamped_and_bad_members_read_as_missing() {
    let doc = read(&project(json!([
        {"name": "a", "loop": "bounce", "length": -3, "snapping": 5},
        {"name": "b", "length": 99999, "snapping": 100000},
        {"length": 2.0},
    ])));
    let [a, b, c] = [0, 1, 2].map(|i| &doc.animations.items()[i]);
    assert_eq!(
        (a.loop_mode, a.length, a.snapping),
        (LoopMode::Once, 0.0, 10)
    );
    assert_eq!((b.length, b.snapping), (3600.0, 500));
    assert_eq!(&*c.name, "animation");
    assert_eq!(c.snapping, 24);
    // Things that are no animation come back as they were.
    let odd = project(json!([7, {"name": "a"}, "x"]));
    let doc = read(&odd);
    assert_eq!(doc.animations.len(), 1);
    let out = write(&doc);
    assert_eq!(out["animations"][1], json!(7));
    assert_eq!(out["animations"][2], json!("x"));
    assert_eq!(out["animations"].as_array().unwrap().len(), 3);
}

#[test]
fn keyframes_at_one_time_become_one_and_the_last_wins() {
    let animations = json!([{"name": "a", "animators": {"g1": {"name": "root", "type": "bone",
    "keyframes": [
        key("rotation", "k1", json!(1.0), "linear", json!([point(json!("1"), json!("0"), json!("0"))])),
        key("rotation", "k2", json!(0.0), "linear", json!([point(json!("5"), json!("0"), json!("0"))])),
        key("rotation", "k3", json!(1.0), "linear", json!([point(json!("9"), json!("0"), json!("0"))])),
    ]}}}]);
    let imported = import_json(&project(animations));
    assert!(imported.notices.contains(&Notice::Skipped {
        what: "keyframes at the same time as another".to_owned(),
        count: 1
    }));
    let doc = open(imported);
    let keys = &doc.animations.items()[0].tracks[0].rotation;
    assert_eq!(keys.len(), 2);
    assert_eq!((keys[0].time, keys[1].time), (0.0, 1.0));
    // Version 4.10 turns about X the other way than the model does.
    assert_eq!(keys[1].value[0].number(), Some(-9.0));
    assert_eq!(keys[1].uuid.as_deref(), Some("k3"));
}

#[test]
fn an_animator_the_model_cannot_hold_is_kept_whole() {
    let three_points = json!([
        point(json!("1"), json!("0"), json!("0")),
        point(json!("2"), json!("0"), json!("0")),
        point(json!("3"), json!("0"), json!("0")),
    ]);
    let animations = json!([{"name": "a", "animators": {
        "g1": {"name": "root", "type": "bone", "keyframes": [
            key("rotation", "k1", json!(0), "linear", three_points)]},
        "g2": {"name": "arm", "type": "bone", "keyframes": [
            {"channel": "particle", "time": 0, "uuid": "p1", "data_points": [{"effect": "x"}]}]},
        "g3": {"name": "tail", "type": "locator"},
    }}]);
    let doc = read(&project(animations.clone()));
    assert!(doc.animations.items()[0].tracks.is_empty());
    let out = write(&doc);
    assert_eq!(
        out["animations"][0]["animators"],
        animations[0]["animators"]
    );
}

#[test]
fn a_keyframe_of_an_older_project_has_its_values_on_itself() {
    let animations = json!([{"name": "a", "animators": {"g1": {"name": "root", "type": "bone",
        "keyframes": [{"channel": "position", "time": 0.5, "x": 1, "y": "2", "z": 3.5}]}}}]);
    let doc = read(&project(animations));
    let key = &doc.animations.items()[0].tracks[0].position[0];
    // The X of a position has the other sign in a project of version 4.10.
    assert_eq!(key.value[0], Value::Num(-1.0));
    assert_eq!(key.value[1].number(), Some(2.0));
    assert_eq!(key.value[2], Value::Num(3.5));
    // The writer gives the form of today.
    let out = write(&doc);
    let written = &out["animations"][0]["animators"]["g1"]["keyframes"][0];
    assert_eq!(written["data_points"][0]["z"], json!(3.5));
    assert!(written["uuid"].is_string());
}

#[test]
fn a_scale_keyframe_without_the_uniform_flag_comes_back_without_it() {
    let scale = |uniform: Option<bool>| {
        let mut k = key(
            "scale",
            "k1",
            json!(0),
            "linear",
            json!([point(json!("1"), json!("1"), json!("1"))]),
        );
        match uniform {
            Some(flag) => k["uniform"] = json!(flag),
            None => {
                k.as_object_mut().unwrap().remove("uniform");
            }
        }
        k
    };
    for flag in [None, Some(true), Some(false)] {
        let animations = json!([{"uuid": "a1", "name": "a", "loop": "once", "override": false,
            "length": 1, "snapping": 24,
            "animators": {"g1": {"name": "root", "type": "bone", "keyframes": [scale(flag)]}}}]);
        let doc = read(&project(animations.clone()));
        assert_eq!(doc.animations.items()[0].tracks[0].scale[0].uniform, flag);
        assert_eq!(write(&doc)["animations"], animations, "{flag:?}");
    }
}

#[test]
fn animations_beyond_the_limits_stay_as_they_were() {
    let many: Vec<Json> = (0..MAX_ANIMATIONS + 3)
        .map(|i| json!({"name": format!("a{i}")}))
        .collect();
    let imported = import_json(&project(Json::Array(many)));
    assert!(imported.notices.contains(&Notice::Kept {
        what: "animations".to_owned(),
        count: 3
    }));
    let doc = open(imported);
    assert_eq!(doc.animations.len(), MAX_ANIMATIONS);
    let out = write(&doc);
    assert_eq!(
        out["animations"].as_array().unwrap().len(),
        MAX_ANIMATIONS + 3
    );
    assert_eq!(
        out["animations"][MAX_ANIMATIONS + 2]["name"],
        json!(format!("a{}", MAX_ANIMATIONS + 2))
    );

    // The keyframes of all animations are bounded together.
    let keys: Vec<Json> = (0..MAX_KEYFRAMES / 2 + 1)
        .map(|i| {
            key(
                "rotation",
                &format!("k{i}"),
                json!(i as f64 / 100.0),
                "linear",
                json!([point(json!("0"), json!("0"), json!("0"))]),
            )
        })
        .collect();
    let big = |name: &str| json!({"name": name, "animators": {"g1": {"name": "root", "type": "bone", "keyframes": keys}}});
    let doc = read(&project(json!([big("one"), big("two")])));
    assert_eq!(
        doc.animations.len(),
        1,
        "the second would pass 100000 keyframes"
    );
    assert_eq!(write(&doc)["animations"].as_array().unwrap().len(), 2);
}

#[test]
fn an_edited_model_writes_its_own_keyframes_with_fresh_uuids() {
    let mut doc = read(&project(full()));
    let g = doc.groups.ids()[1];
    let walk = doc.animations.ids()[0];
    doc.edit_animation(walk, |a| {
        let mut added = Keyframe::new(0.75, [0.0, 0.0, 12.5]);
        added.interp = Interp::CatmullRom;
        a.track_mut(g).rotation.push(added);
        a.length = 2.0;
    })
    .unwrap();
    let id = doc.add_animation(Animation::new("new", 1.0)).unwrap();
    doc.edit_animation(id, |a| {
        a.track_mut(g).scale = vec![Keyframe::new(0.0, [1.0, 2.0, 1.0])];
    })
    .unwrap();
    let out = write(&doc);
    let walk_out = &out["animations"][0];
    assert_eq!(walk_out["length"], json!(2));
    let arm = &walk_out["animators"]["g2"]["keyframes"];
    assert_eq!(arm.as_array().unwrap().len(), 2);
    assert_eq!(arm[1]["time"], json!(0.75));
    assert_eq!(arm[1]["interpolation"], json!("catmullrom"));
    assert_eq!(arm[1]["data_points"][0]["z"], json!(12.5));
    let uuids: Vec<&str> = [&arm[1]["uuid"], &out["animations"][2]["uuid"]]
        .iter()
        .map(|u| u.as_str().expect("a uuid"))
        .collect();
    assert_ne!(uuids[0], uuids[1]);
    assert!(!["k1", "k2", "k3", "k5", "k9", "a1", "a2", "g1", "g2", "e1"].contains(&uuids[0]));
    // Reading it again gives the model back.
    let back = read(&out);
    assert_eq!(back.animations.len(), 3);
    assert_eq!(
        back.animations.items()[2].tracks[0].scale[0].value[1],
        Value::Num(2.0)
    );
    assert_eq!(back.animations.items()[0].tracks[1].rotation.len(), 2);
}

#[test]
fn an_animation_keeps_a_project_a_free_one_and_a_model_without_any_stays_a_java_block() {
    let out = write(&read(&project(full())));
    assert_eq!(out["meta"]["model_format"], json!("free"));
    let mut plain = project(json!([]));
    plain["meta"]["model_format"] = json!("java_block");
    plain["outliner"] = json!(["e1"]);
    let out = write(&read(&plain));
    assert_eq!(out["meta"]["model_format"], json!("java_block"));
    assert!(out.get("animations").is_none());
}

#[test]
fn a_group_deleted_in_the_model_takes_its_animator_along() {
    let mut doc = read(&project(full()));
    let g = doc.groups.ids()[1];
    doc.remove_groups(&FxHashSet::from_iter([g]));
    let out = write(&doc);
    assert!(out["animations"][0]["animators"].get("g2").is_none());
    assert!(out["animations"][0]["animators"].get("g1").is_some());
    assert!(out["animations"][0]["animators"].get("effects").is_some());
}

// --- The signs of version 4.10 and 5.0 --------------------------------------------------------

/// A project of a version with the root bone moving from rest at 0 s to `rotation` and `position`
/// at 1 s, linearly.
fn moving(version: &str, rotation: [Json; 3], position: [Json; 3]) -> Json {
    let [rx, ry, rz] = rotation;
    let [px, py, pz] = position;
    let zero = || point(json!("0"), json!("0"), json!("0"));
    let mut out = project(json!([{
        "uuid": "a1", "name": "move", "loop": "once", "override": false, "length": 1,
        "snapping": 24,
        "animators": {"g1": {"name": "root", "type": "bone", "keyframes": [
            key("rotation", "r0", json!(0), "linear", json!([zero()])),
            key("rotation", "r1", json!(1), "linear", json!([point(rx, ry, rz)])),
            key("position", "p0", json!(0), "linear", json!([zero()])),
            key("position", "p1", json!(1), "linear", json!([point(px, py, pz)])),
        ]}},
    }]));
    out["meta"]["format_version"] = json!(version);
    out
}

#[test]
fn a_4_10_and_a_5_0_project_with_the_same_motion_pose_the_same() {
    // Version 4.10 has the X of position and rotation and the Y of rotation the other way.
    let old = read(&moving(
        "4.10",
        [json!("30"), json!(20), json!("10")],
        [json!("2"), json!("1"), json!(3)],
    ));
    let new = read(&moving(
        "5.0",
        [json!("-30"), json!(-20), json!("10")],
        [json!("-2"), json!("1"), json!(3)],
    ));
    // Read as if it were 5.0, the motion of the old project turns and moves the other way.
    let wrong = read(&moving(
        "5.0",
        [json!("30"), json!(20), json!("10")],
        [json!("2"), json!("1"), json!(3)],
    ));
    let delta = |doc: &Document, t: f32| {
        let animation = &doc.animations.items()[0];
        *pose(doc, animation, t)
            .delta_of(doc.groups.ids()[0])
            .expect("the root bone moves")
    };
    for t in [0.25, 0.5, 1.0] {
        assert!(delta(&old, t).abs_diff_eq(delta(&new, t), 1e-5), "{t}");
        assert!(!delta(&old, t).abs_diff_eq(delta(&wrong, t), 1e-2), "{t}");
    }
    let end = |doc: &Document| {
        doc.animations.items()[0].tracks[0].rotation[1]
            .value
            .clone()
    };
    assert_eq!(end(&old)[0].number(), Some(-30.0));
    assert_eq!(end(&new)[0].number(), Some(-30.0));
    assert_eq!(end(&old)[2].number(), Some(10.0));

    // A project without a version is read as 4.0, as the other rules of the reader do.
    let mut bare = moving(
        "4.10",
        [json!("30"), json!(0), json!(0)],
        [json!(0), json!(0), json!(0)],
    );
    bare["meta"]
        .as_object_mut()
        .unwrap()
        .remove("format_version");
    assert_eq!(end(&read(&bare))[0].number(), Some(-30.0));
}

/// Animations of a 4.10 project with a value of every kind on the axes whose sign differs from
/// 5.0: numbers, numbers written as text in several forms, expressions and Bezier handles.
fn signed() -> Json {
    let mut rotation = key(
        "rotation",
        "k3",
        json!(1),
        "bezier",
        json!([
            point(json!("-7.50"), json!(3), json!("+2")),
            point(json!(" 4 "), json!("v.a + 1"), json!(-0.5))
        ]),
    );
    rotation["bezier_linked"] = json!(true);
    rotation["bezier_left_time"] = json!([-0.5, -0.5, -0.5]);
    rotation["bezier_left_value"] = json!([2, -3.5, 4]);
    rotation["bezier_right_time"] = json!([0.5, 0.5, 0.5]);
    rotation["bezier_right_value"] = json!([-2, 3.5, -4]);
    let mut position = key(
        "position",
        "k4",
        json!(2),
        "bezier",
        json!([point(
            json!(1.25),
            json!("-0"),
            json!("-math.sin(query.anim_time) * 4")
        )]),
    );
    position["bezier_linked"] = json!(false);
    position["bezier_left_time"] = json!([-0.25, -0.25, -0.25]);
    position["bezier_left_value"] = json!([0.5, 6, -1]);
    position["bezier_right_time"] = json!([0.25, 0.25, 0.25]);
    position["bezier_right_value"] = json!([-0.5, 6, 1]);
    json!([{
        "uuid": "a1", "name": "signs", "loop": "once", "override": false, "length": 2,
        "snapping": 24,
        "animators": {"g1": {"name": "root", "type": "bone", "keyframes": [
            key("rotation", "k1", json!(0), "linear",
                json!([point(json!("0.50"), json!("1e3"), json!("math.sin(query.anim_time * 90)"))])),
            key("rotation", "k2", json!(0.5), "catmullrom",
                json!([point(json!(4), json!(2.5), json!(1))])),
            rotation,
            key("position", "k5", json!(0), "linear",
                json!([point(json!("math.cos(query.anim_time) + 1"), json!(2), json!("3"))])),
            position,
            key("scale", "k6", json!(0), "linear",
                json!([point(json!("1.50"), json!("2"), json!(3))])),
        ]}},
    }])
}

#[test]
fn a_4_10_project_writes_back_the_numbers_text_and_handles_it_had() {
    let project = project(signed());
    let doc = read(&project);
    let walk = &doc.animations.items()[0].tracks[0];

    // The model has the signs of 5.0: text keeps what the file wrote, and says what it means.
    let first = &walk.rotation[0].value;
    assert_eq!(first[0].number(), Some(-0.5));
    assert_eq!(first[1].number(), Some(-1000.0));
    assert!(
        matches!(&first[2], Value::Text { text, .. } if &**text == "math.sin(query.anim_time * 90)"),
        "Z does not change"
    );
    assert_eq!(walk.rotation[1].value[0], Value::Num(-4.0));
    assert_eq!(walk.rotation[1].value[1], Value::Num(-2.5));
    assert_eq!(
        walk.rotation[1].value[2],
        Value::Num(1.0),
        "Z does not change"
    );
    let bezier = &walk.rotation[2];
    let handles = bezier.bezier.as_ref().unwrap();
    assert_eq!(handles.left_value, [-2.0, 3.5, 4.0]);
    assert_eq!(handles.right_value, [2.0, -3.5, -4.0]);
    assert_eq!(handles.left_time, [-0.5; 3], "times do not change");
    let post = bezier.post.as_ref().unwrap();
    assert_eq!(post[0].number(), Some(-4.0));
    assert!(post[1].is_expression());
    let moved = &walk.position[1].bezier.as_ref().unwrap();
    assert_eq!(
        moved.left_value,
        [-0.5, 6.0, -1.0],
        "only the X of a position changes"
    );
    assert_eq!(walk.position[1].value[0], Value::Num(-1.25));
    assert_eq!(
        walk.scale[0].value[0].number(),
        Some(1.5),
        "the scale does not change"
    );

    // Nothing changed, so everything is written as it was.
    let out = write(&doc);
    assert_eq!(
        out["animations"],
        project["animations"],
        "{}",
        diff(&out["animations"], &project["animations"])
    );
    assert_eq!(out["meta"]["format_version"], json!("4.10"));

    // A value that was changed is written for 4.10: inverted, as a number or as text.
    let mut doc = doc;
    let id = doc.animations.ids()[0];
    doc.edit_animation(id, |a| {
        let keys = &mut a.tracks[0].rotation;
        keys[0].value[0] = Value::Num(5.0);
        keys[0].value[1] = Value::from_text("2.50");
        keys[0].value[2] = Value::from_text("q.x * 2");
        keys[1].value[1] = Value::from_text("q.a + q.b");
    })
    .unwrap();
    let written = &write(&doc)["animations"][0]["animators"]["g1"]["keyframes"];
    let first = &written[0]["data_points"][0];
    assert_eq!(first["x"], json!(-5));
    assert_eq!(first["y"], json!("-2.5"));
    assert_eq!(first["z"], json!("q.x * 2"), "Z does not change");
    assert_eq!(written[1]["data_points"][0]["y"], json!("-(q.a + q.b)"));
    assert_eq!(
        written[1]["data_points"][0]["x"],
        json!(4),
        "the rest is as it was"
    );
}

#[test]
fn a_5_0_project_written_as_4_10_carries_the_converted_values() {
    let mut bezier = key(
        "rotation",
        "k3",
        json!(1),
        "bezier",
        json!([point(json!(4), json!(-2.5), json!(1))]),
    );
    bezier["bezier_linked"] = json!(true);
    bezier["bezier_left_time"] = json!([-0.5, -0.5, -0.5]);
    bezier["bezier_left_value"] = json!([-2, 3.5, 4]);
    bezier["bezier_right_time"] = json!([0.5, 0.5, 0.5]);
    bezier["bezier_right_value"] = json!([2, -3.5, -4]);
    let mut project = project(json!([{
        "uuid": "a1", "name": "signs", "loop": "once", "override": false, "length": 2,
        "snapping": 24,
        "animators": {"g1": {"name": "root", "type": "bone", "keyframes": [
            key("rotation", "k1", json!(0), "linear",
                json!([point(json!("2.5"), json!("v.a + 1"), json!("7"))])),
            key("rotation", "k2", json!(0.5), "linear",
                json!([point(json!("math.sin(query.anim_time)"), json!(-3), json!(1))])),
            bezier,
            key("position", "k4", json!(0), "linear",
                json!([point(json!(1.25), json!("4"), json!(-2))])),
            key("scale", "k5", json!(0), "linear",
                json!([point(json!("1.50"), json!("2"), json!(3))])),
        ]}},
    }]));
    project["meta"]["format_version"] = json!("5.0");

    // The model takes the 5.0 project as it is.
    let doc = read(&project);
    let walk = &doc.animations.items()[0].tracks[0];
    assert_eq!(walk.rotation[0].value[0].number(), Some(2.5));
    assert_eq!(walk.position[0].value[0], Value::Num(1.25));

    let out = write(&doc);
    assert_eq!(out["meta"]["format_version"], json!("4.10"));
    let written = &out["animations"][0]["animators"]["g1"]["keyframes"];
    let at = |key: usize, axis: &str| written[key]["data_points"][0][axis].clone();
    // The X and Y of a rotation and the X of a position are negated, text numbers and
    // expressions inverted; the rest is as it was.
    assert_eq!(
        (at(0, "x"), at(0, "y"), at(0, "z")),
        (json!("-2.5"), json!("-(v.a + 1)"), json!("7"))
    );
    assert_eq!(at(1, "x"), json!("-math.sin(query.anim_time)"));
    assert_eq!((at(1, "y"), at(1, "z")), (json!(3), json!(1)));
    assert_eq!(
        (at(2, "x"), at(2, "y"), at(2, "z")),
        (json!(-4), json!(2.5), json!(1))
    );
    assert_eq!(
        (at(3, "x"), at(3, "y"), at(3, "z")),
        (json!(-1.25), json!("4"), json!(-2))
    );
    assert_eq!(
        (at(4, "x"), at(4, "y"), at(4, "z")),
        (json!("1.50"), json!("2"), json!(3))
    );
    // The handles of the same axes change too; the times do not.
    assert_eq!(written[2]["bezier_left_value"], json!([2, -3.5, 4]));
    assert_eq!(written[2]["bezier_right_value"], json!([-2, 3.5, -4]));
    assert_eq!(written[2]["bezier_left_time"], json!([-0.5, -0.5, -0.5]));

    // Read again as the 4.10 project it now is, the motion is the same.
    let again = read(&out);
    let values = |doc: &Document| {
        let track = &doc.animations.items()[0].tracks[0];
        (
            track.rotation[2].value.clone(),
            track.rotation[2].bezier.clone(),
            track.position[0].value.clone(),
        )
    };
    assert_eq!(values(&again), values(&doc));
}

/// Animations whose animators the model keeps as JSON: one of a bone with three data points, one
/// of a uuid that names no group, and (after the animations that fill the list) one that has no
/// room.
fn kept_whole() -> Json {
    let mut bezier = key(
        "rotation",
        "k1",
        json!(0),
        "bezier",
        json!([point(json!("2.5"), json!(-3), json!(4))]),
    );
    bezier["bezier_left_value"] = json!([1, 2, 3]);
    bezier["bezier_right_value"] = json!([0, -2, 3]);
    let three_points = json!([
        point(json!("1"), json!("0"), json!("0")),
        point(json!("2"), json!("0"), json!("0")),
        point(json!("3"), json!("0"), json!("0")),
    ]);
    let mut animations = vec![json!({"uuid": "a1", "name": "first", "animators": {
        "g1": {"name": "root", "type": "bone", "keyframes": [
            key("rotation", "k0", json!(0), "linear", three_points)]},
        "11111111-2222-3333-4444-555555555555": {"name": "gone", "type": "bone", "keyframes": [
            bezier,
            key("position", "k2", json!(0), "linear", json!([point(json!(5), json!(6), json!(7))])),
            key("scale", "k3", json!(0), "linear", json!([point(json!(8), json!(9), json!(10))])),
            key("rotation", "k4", json!(1), "linear",
                json!([point(json!(0), json!(""), json!("x"))])),
        ]},
    }})];
    animations.extend((1..MAX_ANIMATIONS).map(|i| json!({"name": format!("a{i}")})));
    animations.push(
        json!({"name": "last", "animators": {"g1": {"name": "root", "type": "bone",
        "keyframes": [key("rotation", "k5", json!(0), "linear",
            json!([point(json!("6"), json!(0), json!(0))]))]}}}),
    );
    Json::Array(animations)
}

#[test]
fn what_stays_json_is_written_in_the_signs_of_4_10() {
    let animations = kept_whole();
    let at = |out: &Json, path: &str| out.pointer(path).cloned().unwrap_or(Json::Null);
    let first = "/animations/0/animators/11111111-2222-3333-4444-555555555555/keyframes";

    // A project of 4.10 has its raw animators written exactly as they were.
    let old = project(animations.clone());
    let out = write(&read(&old));
    assert_eq!(
        out["animations"][0]["animators"],
        old["animations"][0]["animators"]
    );
    assert_eq!(
        out["animations"][MAX_ANIMATIONS],
        old["animations"][MAX_ANIMATIONS]
    );

    // A project of 5.0 has them converted when it is read: the animator of a bone with three data
    // points, the animator of a uuid that names no group, and an animation without room.
    let mut new = project(animations);
    new["meta"]["format_version"] = json!("5.0");
    let out = write(&read(&new));
    let points = at(&out, "/animations/0/animators/g1/keyframes/0/data_points");
    assert_eq!(points[0]["x"], json!("-1"));
    assert_eq!(points[2]["x"], json!("-3"));
    assert_eq!(points[1]["y"], json!("0"), "Y of a rotation: zero stays");
    let gone = at(&out, first);
    // The X and Y of a rotation, with the handles of the Bezier keyframe.
    assert_eq!(
        gone[0]["data_points"][0],
        json!({"x": "-2.5", "y": 3, "z": 4})
    );
    assert_eq!(gone[0]["bezier_left_value"], json!([-1, -2, 3]));
    assert_eq!(gone[0]["bezier_right_value"], json!([0, 2, 3]));
    // The X of a position; the scale does not change; zero and empty text stay.
    assert_eq!(gone[1]["data_points"][0], json!({"x": -5, "y": 6, "z": 7}));
    assert_eq!(gone[2]["data_points"][0], json!({"x": 8, "y": 9, "z": 10}));
    assert_eq!(
        gone[3]["data_points"][0],
        json!({"x": 0, "y": "", "z": "x"})
    );
    let last = at(
        &out,
        &format!("/animations/{MAX_ANIMATIONS}/animators/g1/keyframes/0"),
    );
    assert_eq!(last["data_points"][0]["x"], json!("-6"));
    // What the model holds is converted as before, and reading the output again changes nothing.
    assert_eq!(write(&read(&out))["animations"], out["animations"]);
}

// --- A random project -----------------------------------------------------------------------

/// A number of quarters as the editor prints it: a whole number as an integer.
fn quarters(n: i32) -> Json {
    if n % 4 == 0 {
        json!(n / 4)
    } else {
        json!(f64::from(n) / 4.0)
    }
}

/// A value as the project writes it: a number, text of a number or an expression.
fn value() -> impl Strategy<Value = Json> {
    prop_oneof![
        (-40_i32..=40).prop_map(quarters),
        (-40_i32..=40).prop_map(|n| json!(format!("{}", f64::from(n) / 4.0))),
        Just(json!("math.sin(query.anim_time * 90)")),
        Just(json!("")),
    ]
}

fn handle(range: std::ops::RangeInclusive<i32>) -> impl Strategy<Value = Json> {
    prop::collection::vec(range, 3).prop_map(|v| Json::Array(v.into_iter().map(quarters).collect()))
}

fn keyframe(channel: &'static str, quarters_in: i32, uuid: String) -> impl Strategy<Value = Json> {
    (
        prop::collection::vec(value(), 3),
        prop::option::of(prop::collection::vec(value(), 3)),
        prop::sample::select(vec!["linear", "catmullrom", "bezier", "step"]),
        (
            any::<bool>(),
            handle(-8..=-1),
            handle(-4..=4),
            handle(1..=8),
            handle(-4..=4),
        ),
        -1_i32..=7,
        any::<bool>(),
    )
        .prop_map(move |(first, second, interp, handles, color, uniform)| {
            let to_point = |v: Vec<Json>| point(v[0].clone(), v[1].clone(), v[2].clone());
            let mut points = vec![to_point(first)];
            points.extend(second.map(to_point));
            let mut out = key(
                channel,
                &uuid,
                quarters(quarters_in),
                interp,
                Json::Array(points),
            );
            out["color"] = json!(color);
            if channel == "scale" {
                out["uniform"] = json!(uniform);
            }
            if interp == "bezier" {
                let (linked, left_time, left_value, right_time, right_value) = handles;
                out["bezier_linked"] = json!(linked);
                out["bezier_left_time"] = left_time;
                out["bezier_left_value"] = left_value;
                out["bezier_right_time"] = right_time;
                out["bezier_right_value"] = right_value;
            }
            out
        })
}

/// The keyframes of the three channels, each with times that grow by quarters of a second.
fn animator(group: usize, prefix: String) -> impl Strategy<Value = Json> {
    let channels = ["rotation", "position", "scale"];
    let each: Vec<_> = channels
        .into_iter()
        .map(move |channel| {
            let prefix = prefix.clone();
            prop::collection::vec(1_i32..=3, 0..4).prop_flat_map(move |steps| {
                let mut time = 0_i32;
                let mut strategies = Vec::new();
                for (n, step) in steps.iter().enumerate() {
                    time += step;
                    strategies.push(keyframe(channel, time, format!("{prefix}-{channel}-{n}")));
                }
                strategies
            })
        })
        .collect();
    let extra = prop::option::of(any::<bool>());
    // Editors list the channels of an animator in different orders.
    let order = prop::sample::select(vec![
        [0, 1, 2],
        [1, 0, 2],
        [2, 1, 0],
        [0, 2, 1],
        [1, 2, 0],
        [2, 0, 1],
    ]);
    (each, extra, order).prop_map(move |(channels, extra, order)| {
        let keys: Vec<Json> = order
            .into_iter()
            .flat_map(|at| channels[at].clone())
            .collect();
        let name = ["root", "arm", "tail"][group];
        let mut node = json!({"name": name, "type": "bone"});
        if let Some(global) = extra {
            node["rotation_global"] = json!(global);
        }
        if !keys.is_empty() {
            node["keyframes"] = Json::Array(keys);
        }
        node
    })
}

fn animation(n: usize) -> impl Strategy<Value = Json> {
    let animators = (
        prop::option::of(animator(0, format!("a{n}g1"))),
        prop::option::of(animator(1, format!("a{n}g2"))),
        prop::option::of(animator(2, format!("a{n}g3"))),
        any::<bool>(),
    );
    (
        prop::sample::select(vec!["once", "loop", "hold"]),
        0_i32..=16,
        10_u32..=500,
        any::<bool>(),
        animators,
    )
        .prop_map(
            move |(mode, length, snapping, overrides, (a, b, c, effects))| {
                let mut map = serde_json::Map::new();
                for (uuid, node) in [("g1", a), ("g2", b), ("g3", c)] {
                    if let Some(node) = node {
                        map.insert(uuid.to_owned(), node);
                    }
                }
                if effects {
                    map.insert(
                        "effects".to_owned(),
                        json!({"name": "Effects", "type": "effect", "keyframes": [
                        {"channel": "timeline", "time": 0.25, "uuid": format!("t{n}"),
                         "data_points": [{"script": "v.x = 1;"}]}]}),
                    );
                }
                let mut item = json!({
                    "uuid": format!("anim-{n}"), "name": format!("anim {n}"), "loop": mode,
                    "override": overrides, "length": quarters(length), "snapping": snapping,
                    "selected": false, "blend_weight": "",
                });
                if !map.is_empty() {
                    item["animators"] = Json::Object(map);
                }
                item
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Whatever the animations hold, reading and writing an unchanged project gives the same
    /// `animations` as JSON values, and the model they make is sound.
    #[test]
    fn an_unchanged_project_writes_the_animations_it_read(
        animations in prop::collection::vec(0..4_usize, 1..3).prop_flat_map(|ns| {
            ns.iter().enumerate().map(|(i, _)| animation(i)).collect::<Vec<_>>()
        })
    ) {
        let project = project(Json::Array(animations));
        let doc = read(&project);
        let out = write(&doc);
        prop_assert!(
            out["animations"] == project["animations"],
            "{}",
            diff(&out["animations"], &project["animations"])
        );
        prop_assert!(doc.check_animations().is_ok());
        let again = write(&read(&out));
        prop_assert!(again["animations"] == out["animations"]);
        let counted: usize = doc.animations.items().iter().map(|a| a.keyframe_count()).sum();
        let from_json: usize = project["animations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["animators"].as_object())
            .flatten()
            .filter(|(key, _)| key.as_str() != "effects")
            .map(|(_, node)| node["keyframes"].as_array().map_or(0, Vec::len))
            .sum();
        prop_assert_eq!(counted, from_json);
    }
}
