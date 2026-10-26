//! Groups in `.bbmodel` projects: read, write, read again (hierarchy A1 5). The projects are made
//! here: a random tree of rotated groups, and hand-written ones.

mod common;

use std::path::Path;

use common::{NoFiles, near, open};
use cuboid_fmt_bbmodel::{Notice, export, import};
use cuboid_model::{Blueprint, Document, Element, ElementType, Group, Label, ObjRef};
use cuboid_num::euler_to_quat;
use glam::Vec3;
use proptest::prelude::*;
use serde_json::{Value, json};

fn read_bytes(bytes: &[u8]) -> Document {
    open(import(bytes, Path::new("dir/Model.bbmodel"), &NoFiles).expect("imports"))
}

fn read(project: &Value) -> Document {
    read_bytes(&serde_json::to_vec(project).expect("serialises"))
}

fn write(doc: &Document) -> (Vec<u8>, Value) {
    let out = export(doc).expect("exports");
    let value = serde_json::from_slice(&out.bytes).expect("the output is JSON");
    (out.bytes, value)
}

/// The numbers of a JSON array.
fn nums(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|n| n.as_f64().expect("a number"))
        .collect()
}

/// The numbers that place the groups and cubes of a project, by uuid: `from`, `to`, `origin` and
/// the rotation (zero when the member is missing) of a cube, `origin` and the rotation of a group.
fn numbers_of(project: &Value) -> std::collections::BTreeMap<String, Vec<f64>> {
    let rotation = |v: &Value| v.get("rotation").map_or(vec![0.0; 3], nums);
    let mut out = std::collections::BTreeMap::new();
    for el in project["elements"].as_array().expect("elements") {
        let mut n = nums(&el["from"]);
        n.extend(nums(&el["to"]));
        n.extend(nums(&el["origin"]));
        n.extend(rotation(el));
        out.insert(el["uuid"].as_str().expect("uuid").to_owned(), n);
    }
    let mut stack: Vec<&Value> = project["outliner"]
        .as_array()
        .expect("outliner")
        .iter()
        .collect();
    while let Some(node) = stack.pop() {
        let Some(children) = node.get("children").and_then(Value::as_array) else {
            continue;
        };
        let mut n = nums(&node["origin"]);
        n.extend(rotation(node));
        out.insert(node["uuid"].as_str().expect("uuid").to_owned(), n);
        stack.extend(children);
    }
    out
}

/// Where two JSON values first differ, for a failure message.
fn first_difference(a: &Value, b: &Value) -> String {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (key, left) in x {
                match y.get(key) {
                    Some(right) if left != right => {
                        return format!("{key}: {}", first_difference(left, right));
                    }
                    None => return format!("{key} is missing on the right"),
                    _ => {}
                }
            }
            "an extra member on the right".to_owned()
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .position(|(l, r)| l != r)
            .map_or_else(String::new, |at| {
                format!("[{at}] {}", first_difference(&x[at], &y[at]))
            }),
        _ => format!("{a} against {b}"),
    }
}

fn same_quat(a: Vec3, b: Vec3) -> bool {
    let (a, b) = (euler_to_quat(a), euler_to_quat(b));
    a.abs_diff_eq(b, 1e-4) || a.abs_diff_eq(-b, 1e-4)
}

/// Everything that puts an object or a group in the world is the same within `tolerance`.
fn same_world(a: &Document, b: &Document, tolerance: f32) -> Result<(), TestCaseError> {
    prop_assert_eq!(a.elements.len(), b.elements.len());
    prop_assert_eq!(a.groups.len(), b.groups.len());
    for (x, y) in a.elements.items().iter().zip(b.elements.items()) {
        let (x, y) = (&x.base, &y.base);
        prop_assert!(x.p.abs_diff_eq(y.p, tolerance), "p {:?} {:?}", x.p, y.p);
        prop_assert!(x.o.abs_diff_eq(y.o, tolerance), "o {:?} {:?}", x.o, y.o);
        prop_assert!(x.s.abs_diff_eq(y.s, tolerance), "s {:?} {:?}", x.s, y.s);
        prop_assert!(same_quat(x.e, y.e), "e {:?} {:?}", x.e, y.e);
    }
    for (&p, &q) in a.groups.ids().iter().zip(b.groups.ids()) {
        let (g, h) = (
            a.groups.get(p).expect("group"),
            b.groups.get(q).expect("group"),
        );
        prop_assert!(
            g.origin.abs_diff_eq(h.origin, tolerance),
            "{:?} {:?}",
            g.origin,
            h.origin
        );
        prop_assert!(
            same_quat(g.rotation, h.rotation),
            "{:?} {:?}",
            g.rotation,
            h.rotation
        );
        prop_assert_eq!(
            (g.visible, g.locked, g.export),
            (h.visible, h.locked, h.export)
        );
        prop_assert_eq!(&g.name, &h.name);
        prop_assert_eq!(a.groups.depth(p), b.groups.depth(q));
    }
    Ok(())
}

// --- A random tree of groups -------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CubeSpec {
    from: [f32; 3],
    size: [f32; 3],
    pivot: [f32; 3],
    rotation: [f32; 3],
}

#[derive(Clone, Debug)]
struct GroupSpec {
    origin: [f32; 3],
    rotation: [f32; 3],
    visible: bool,
    locked: bool,
}

#[derive(Clone, Debug)]
enum Spec {
    Cube(CubeSpec),
    Group(GroupSpec, Vec<Spec>),
}

fn quarters(range: std::ops::RangeInclusive<i32>) -> impl Strategy<Value = f32> {
    range.prop_map(|n| n as f32 * 0.25)
}

fn vec3(range: std::ops::RangeInclusive<i32>) -> impl Strategy<Value = [f32; 3]> {
    [
        quarters(range.clone()),
        quarters(range.clone()),
        quarters(range),
    ]
}

/// Angles in steps of 5 degrees up to `steps` of them, a lot of them zero.
fn angles(steps: i32) -> impl Strategy<Value = [f32; 3]> {
    let angle = prop_oneof![
        5 => Just(0.0_f32),
        4 => (-steps..=steps).prop_map(|n| n as f32 * 5.0),
    ];
    [angle.clone(), angle.clone(), angle]
}

/// The rotation of a cube the way the Minecraft rules have it: one axis.
fn one_axis(steps: i32) -> impl Strategy<Value = [f32; 3]> {
    (0..3_usize, -steps..=steps).prop_map(|(axis, n)| {
        let mut angles = [0.0; 3];
        angles[axis] = n as f32 * 5.0;
        angles
    })
}

/// A tree of groups with the given rotations of cubes and groups. The rotations of a chain add
/// up, and where they add up to a quarter turn the angles of the Euler form are not unique, so
/// the property that compares numbers keeps them small.
fn spec(
    cube_rotation: impl Strategy<Value = [f32; 3]> + 'static,
    group_rotation: impl Strategy<Value = [f32; 3]> + 'static,
) -> impl Strategy<Value = Spec> {
    let cube = (vec3(-32..=32), vec3(2..=32), vec3(-32..=32), cube_rotation).prop_map(
        |(from, size, pivot, rotation)| {
            Spec::Cube(CubeSpec {
                from,
                size,
                pivot,
                rotation,
            })
        },
    );
    let group_rotation = group_rotation.boxed();
    cube.prop_recursive(3, 14, 3, move |inner| {
        (
            vec3(-32..=32),
            group_rotation.clone(),
            any::<bool>(),
            any::<bool>(),
            prop::collection::vec(inner, 1..4),
        )
            .prop_map(|(origin, rotation, visible, locked, children)| {
                let group = GroupSpec {
                    origin,
                    rotation,
                    visible,
                    locked,
                };
                Spec::Group(group, children)
            })
    })
}

/// The outliner entries for `specs`; the cubes go to `elements`.
fn render(specs: &[Spec], elements: &mut Vec<Value>, counter: &mut usize) -> Vec<Value> {
    let mut out = Vec::new();
    for spec in specs {
        *counter += 1;
        let n = *counter;
        match spec {
            Spec::Cube(c) => {
                let to: Vec<f32> = c.from.iter().zip(c.size).map(|(f, s)| f + s).collect();
                elements.push(json!({
                    "name": format!("cube {n}"), "from": c.from, "to": to, "origin": c.pivot,
                    "rotation": c.rotation, "faces": {}, "type": "cube", "uuid": format!("e{n}"),
                }));
                out.push(Value::String(format!("e{n}")));
            }
            Spec::Group(g, children) => {
                let children = render(children, elements, counter);
                out.push(json!({
                    "name": format!("group {n}"), "origin": g.origin, "rotation": g.rotation,
                    "uuid": format!("g{n}"), "export": true, "visibility": g.visible,
                    "locked": g.locked, "children": children,
                }));
            }
        }
    }
    out
}

fn project_of(specs: &[Spec]) -> Value {
    let mut elements = Vec::new();
    let outliner = render(specs, &mut elements, &mut 0);
    json!({
        "meta": {"format_version": "4.10", "model_format": "free", "box_uv": false},
        "name": "tree",
        "resolution": {"width": 16, "height": 16},
        "elements": elements,
        "outliner": outliner,
        // Animations keep the project a free one, whatever its cubes would allow.
        "animations": [],
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Reading, writing and reading again keeps what is in the world, and writing what was read
    /// again gives the same groups and elements as the first write.
    #[test]
    fn a_tree_of_rotated_groups_survives_read_write_read(specs in prop::collection::vec(spec(one_axis(3), angles(3)), 1..4)) {
        let project = project_of(&specs);
        let first = read(&project);
        let (bytes, json_a) = write(&first);
        let second = read_bytes(&bytes);
        same_world(&first, &second, 1e-4)?;
        let (_, json_b) = write(&second);
        for key in ["elements", "outliner", "groups"] {
            prop_assert!(json_a[key] == json_b[key], "{key} differs: {}", first_difference(&json_a[key], &json_b[key]));
        }
        // The file the groups came from is what the first write gives, number for number.
        prop_assert_eq!(numbers_of(&project), numbers_of(&json_a));
    }

    /// Cubes that turn on several axes lose at most the last decimal of the file on the way.
    #[test]
    fn cubes_on_several_axes_keep_their_place_in_the_world(specs in prop::collection::vec(spec(angles(12), angles(12)), 1..4)) {
        let first = read(&project_of(&specs));
        let second = read_bytes(&write(&first).0);
        same_world(&first, &second, 3e-4)?;
    }
}

// --- Hand-written projects ---------------------------------------------------------------------

/// Three nested groups, each turned about its own pivot, and a cube in the innermost.
fn chain() -> Value {
    json!({
        "meta": {"format_version": "4.10", "model_format": "free", "box_uv": false},
        "name": "chain",
        "resolution": {"width": 16, "height": 16},
        "elements": [
            {"name": "hand", "from": [1, 2, 3], "to": [3, 4, 5], "origin": [2, 3, 4],
             "faces": {}, "type": "cube", "uuid": "c1"},
            {"name": "loose", "from": [0, 0, 0], "to": [1, 1, 1], "origin": [0, 0, 0],
             "faces": {}, "type": "cube", "uuid": "c2"}
        ],
        "outliner": [
            {"name": "arm", "origin": [0, 10, 0], "rotation": [0, 0, 90], "uuid": "g1",
             "children": [
                {"name": "forearm", "origin": [4, 10, 0], "rotation": [0, 45, 0], "uuid": "g2",
                 "children": [
                    {"name": "wrist", "origin": [6, 12, 0], "rotation": [30, 0, 0], "uuid": "g3",
                     "children": ["c1"]}
                 ]}
             ]},
            "c2"
        ],
    })
}

#[test]
fn a_chain_of_three_groups_writes_the_numbers_of_the_file_it_came_from() {
    let doc = read(&chain());
    assert_eq!(doc.groups.len(), 3);
    assert_eq!(doc.groups.depth(doc.groups.ids()[2]), 3);
    let (_, out) = write(&doc);

    let outliner = out["outliner"].as_array().expect("an outliner");
    assert_eq!(outliner.len(), 2);
    assert_eq!(outliner[1], json!("c2"));
    let (mut node, mut names) = (&outliner[0], Vec::new());
    let expected = [
        ("arm", [0.0, 10.0, 0.0], [0.0, 0.0, 90.0], "g1"),
        ("forearm", [4.0, 10.0, 0.0], [0.0, 45.0, 0.0], "g2"),
        ("wrist", [6.0, 12.0, 0.0], [30.0, 0.0, 0.0], "g3"),
    ];
    for (name, origin, rotation, uuid) in expected {
        assert_eq!(node["name"], json!(name));
        assert_eq!(nums(&node["origin"]), origin, "{name}");
        assert_eq!(nums(&node["rotation"]), rotation, "{name}");
        assert_eq!(node["uuid"], json!(uuid));
        names.push(name);
        node = &node["children"][0];
    }
    assert_eq!(node, &json!("c1"));

    let elements = out["elements"].as_array().expect("elements");
    let hand = &elements[0];
    assert_eq!(hand["uuid"], json!("c1"));
    assert_eq!(nums(&hand["from"]), [1.0, 2.0, 3.0]);
    assert_eq!(nums(&hand["to"]), [3.0, 4.0, 5.0]);
    assert_eq!(nums(&hand["origin"]), [2.0, 3.0, 4.0]);
    assert!(hand.get("rotation").is_none(), "no rotation of its own");
    assert_eq!(out["meta"]["model_format"], json!("free"));
}

#[test]
fn flags_names_and_the_order_of_the_children_come_back() {
    let mut project = chain();
    project["outliner"] = json!([
        "c2",
        {"name": "folder", "origin": [0, 0, 0], "uuid": "g9", "export": false, "locked": true,
         "visibility": false, "children": ["c1"]}
    ]);
    let doc = read(&project);
    let group = doc.groups.get(doc.groups.ids()[0]).unwrap();
    assert!(!group.visible && group.locked && !group.export);
    let (_, out) = write(&doc);
    let outliner = out["outliner"].as_array().unwrap();
    assert_eq!(outliner[0], json!("c2"), "the loose cube stays first");
    assert_eq!(outliner[1]["export"], json!(false));
    assert_eq!(outliner[1]["locked"], json!(true));
    assert_eq!(outliner[1]["visibility"], json!(false));
    assert_eq!(outliner[1]["children"], json!(["c1"]));
}

#[test]
fn uuids_come_back_and_new_objects_get_fresh_ones() {
    let mut doc = read(&chain());
    // A new cube and a new group, as the editor makes them.
    let blueprint = Blueprint::default();
    let el = Element::new(
        ElementType::Cube,
        &blueprint,
        3,
        [1.0; 4],
        &Label::from("a"),
    );
    let id = doc.elements.push(el);
    doc.insert_group(Group::new("fresh", Vec3::ZERO), &[ObjRef::Element(id)])
        .expect("a group");
    let (_, out) = write(&doc);

    let mut uuids: Vec<String> = out["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["uuid"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(uuids[..2], ["c1", "c2"], "the project's own uuids");
    let mut stack: Vec<&Value> = out["outliner"].as_array().unwrap().iter().collect();
    while let Some(node) = stack.pop() {
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            uuids.push(node["uuid"].as_str().unwrap().to_owned());
            stack.extend(children);
        }
    }
    let total = uuids.len();
    assert_eq!(total, 7, "three cubes and four groups");
    uuids.sort();
    uuids.dedup();
    assert_eq!(uuids.len(), total, "no uuid twice");
    assert!(uuids.iter().any(|u| u == "g2"));

    // The uuids survive another round.
    let again = read_bytes(&write(&doc).0);
    let (_, out2) = write(&again);
    assert_eq!(out["elements"], out2["elements"]);
}

#[test]
fn a_uuid_that_two_nodes_claim_goes_to_one_of_them() {
    let mut project = chain();
    project["elements"][1]["uuid"] = json!("c1");
    project["outliner"][1] = json!("c1");
    // The second cube is no longer listed by its own uuid; both elements claim "c1".
    let doc = read(&project);
    let (_, out) = write(&doc);
    let uuids: Vec<&str> = out["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["uuid"].as_str().unwrap())
        .collect();
    assert_eq!(uuids.len(), 2);
    assert_ne!(uuids[0], uuids[1]);
}

#[test]
fn members_the_model_cannot_hold_are_written_back_unchanged() {
    let mut project = chain();
    project["animations"] = json!([
        {"uuid": "a1", "name": "walk", "loop": "loop", "length": 1.5, "snapping": 24,
         "animators": {"g1": {"name": "arm", "type": "bone",
            "keyframes": [{"channel": "rotation", "time": 0.5, "interpolation": "linear",
                           "data_points": [{"x": 0, "y": "math.sin(query.anim_time)", "z": 1.25}]}]}}}
    ]);
    project["animation_variable_placeholders"] = json!("v.x = 1;");
    project["editor_state"] = json!({"zoom": 2.5, "tags": ["a", "b"]});
    let imported = import(
        &serde_json::to_vec(&project).unwrap(),
        Path::new("dir/Model.bbmodel"),
        &NoFiles,
    )
    .expect("imports");
    assert!(imported.notices.contains(&Notice::Kept {
        what: "animations".to_owned(),
        count: 1
    }));
    assert!(
        !imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Skipped { .. }))
    );
    let doc = open(imported);
    let (_, out) = write(&doc);
    for key in [
        "animations",
        "animation_variable_placeholders",
        "editor_state",
    ] {
        assert_eq!(out[key], project[key], "{key}");
    }
    // Animations are not for a Java block model.
    assert_eq!(out["meta"]["model_format"], json!("free"));
    // The kept members survive a second round and the groups they name keep their uuids.
    let (_, out2) = write(&read_bytes(&write(&doc).0));
    assert_eq!(out2["animations"], project["animations"]);
    assert_eq!(out2["outliner"], out["outliner"]);

    // A model that did not come from a project has nothing to write back.
    let mut fresh = Document::new(Default::default());
    fresh.elements.push(Element::new(
        ElementType::Cube,
        &Blueprint::default(),
        1,
        [1.0; 4],
        &Label::from("a"),
    ));
    assert!(write(&fresh).1.get("animations").is_none());
}

#[test]
fn a_java_block_project_with_folders_stays_one_until_a_group_turns() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
        "elements": [{"name": "c", "from": [0, 0, 0], "to": [4, 4, 4], "faces": {},
            "type": "cube", "uuid": "c1"}],
        "outliner": [{"name": "folder", "origin": [8, 8, 8], "rotation": [0, 0, 0],
            "uuid": "g1", "children": ["c1"]}],
    });
    let mut doc = read(&project);
    let (_, out) = write(&doc);
    assert_eq!(out["meta"]["model_format"], json!("java_block"));
    assert_eq!(nums(&out["outliner"][0]["origin"]), [8.0, 8.0, 8.0]);
    assert_eq!(nums(&out["elements"][0]["from"]), [0.0, 0.0, 0.0]);

    // Turn the folder: Java blocks have no bones, so the project becomes a free one and the
    // cube keeps its place in the world.
    let id = doc.groups.ids()[0];
    let before = doc.elements.items()[0].base.clone();
    doc.groups
        .edit(id, cuboid_model::ChangeFlags::all(), &mut doc.journal)
        .unwrap()
        .rotation = Vec3::new(0.0, 45.0, 0.0);
    let (bytes, out) = write(&doc);
    assert_eq!(out["meta"]["model_format"], json!("free"));
    let back = read_bytes(&bytes);
    let after = &back.elements.items()[0].base;
    assert!(near(after.p, before.p), "{:?} {:?}", after.p, before.p);
    assert!(same_quat(after.e, before.e));
    assert!(near(
        back.groups.get(back.groups.ids()[0]).unwrap().rotation,
        Vec3::new(0.0, 45.0, 0.0)
    ));
}

#[test]
fn a_group_moved_in_the_model_writes_the_local_numbers_that_keep_the_world() {
    let mut doc = read(&chain());
    // Move the whole arm up by 3 units, as the move command does: the pivots of its groups and
    // the world positions of its cubes change by the same vector.
    let arm = doc.groups.ids()[0];
    let delta = Vec3::new(0.0, 3.0, 0.0);
    let hand = doc.elements.ids()[0];
    for id in doc.groups.ids().to_vec() {
        doc.groups
            .edit(id, cuboid_model::ChangeFlags::all(), &mut doc.journal)
            .unwrap()
            .origin += delta;
    }
    let el = doc.elements.edit(hand, &mut doc.journal).unwrap();
    el.base.p += delta;
    el.base.o += delta;
    assert!(doc.groups.get(arm).is_some());
    let (bytes, _) = write(&doc);
    let back = read_bytes(&bytes);
    let moved = &back.elements.items()[0].base;
    let original = &doc.elements.items()[0].base;
    assert!(near(moved.p, original.p), "{:?} {:?}", moved.p, original.p);
    assert!(same_quat(moved.e, original.e));
}
