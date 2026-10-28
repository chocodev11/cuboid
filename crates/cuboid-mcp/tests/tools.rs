//! The tools on models made in the test: reading, editing, the revision rules, dry runs, the
//! history, textures and the check.

mod common;

use common::{Client, Fake, conforms, project};
use cuboid_mcp::Picture;
use cuboid_mcp::tools::TOOLS;
use serde_json::{Value, json};

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|n| n.as_f64().expect("a number"))
        .collect()
}

/// The label of the texture of the lamp.
fn texture_of(client: &mut Client<'_>) -> String {
    let opened = client.ok("project_open", json!({"model": "lamp.bbmodel"}));
    opened["textures"][0]["name"]
        .as_str()
        .expect("a texture")
        .to_owned()
}

#[test]
fn the_outline_of_a_model_names_ids_and_pages_its_elements() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let outline = client.ok(
        "model_outline",
        json!({"model": "lamp.bbmodel", "limit": 2}),
    );
    assert_eq!(outline["total"], 3);
    assert_eq!(outline["next"], "2");
    assert_eq!(outline["groups"][0]["path"], "lamp_arm");
    assert!(
        outline["groups"][0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with('g'))
    );
    assert_eq!(outline["elements"][0]["path"], "base");
    assert!(
        outline["elements"][0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with('e'))
    );
    let last = client.ok(
        "model_outline",
        json!({"model": "lamp.bbmodel", "cursor": "2"}),
    );
    assert_eq!(last["elements"][0]["path"], "lamp_arm/shade");
    assert!(last.get("next").is_none() && last.get("groups").is_none());
    let arm = client.ok(
        "model_outline",
        json!({"model": "lamp.bbmodel", "select": "lamp_arm"}),
    );
    assert_eq!(arm["total"], 2);
    assert_eq!(arm["elements"][0]["path"], "lamp_arm/arm_left");
    let none = client.ok(
        "model_outline",
        json!({"model": "lamp.bbmodel", "depth": 0}),
    );
    assert!(none["groups"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn model_get_gives_the_record_of_an_object_in_model_space() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let arm = client.get("lamp.bbmodel", "lamp_arm/arm_left");
    assert_eq!(numbers(&arm["from"]), [7.0, 2.0, 7.0]);
    assert_eq!(numbers(&arm["to"]), [9.0, 10.0, 9.0]);
    assert_eq!(numbers(&arm["rotation"]), [0.0, 0.0, 22.5]);
    assert_eq!(numbers(&arm["pivot"]), [8.0, 2.0, 8.0]);
    assert_eq!(arm["visible"], true);
    assert_eq!(arm["locked"], false);
    assert_eq!(arm["name"], "arm_left");
    assert!(arm["faces"]["east"].is_array());
    let group = client.get("lamp.bbmodel", "lamp_arm");
    assert_eq!(group["children"], json!(["arm_left", "shade"]));
    let some = client.ok(
        "model_get",
        json!({"model": "lamp.bbmodel", "select": ["base", "lamp_arm/*"], "fields": ["from"], "limit": 2}),
    );
    assert_eq!(
        (some["total"].as_u64(), some["next"].as_str()),
        (Some(3), Some("2"))
    );
    // The id, the path and the field asked for.
    assert_eq!(some["objects"][0].as_object().expect("a record").len(), 3);
    assert_eq!(
        client.code(
            "model_get",
            json!({"model": "lamp.bbmodel", "select": "base", "fields": ["colour"]})
        ),
        "E_INPUT"
    );
}

#[test]
fn a_selector_that_matches_nothing_or_too_much_is_an_error_never_the_selection() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    assert_eq!(
        client.code(
            "model_get",
            json!({"model": "lamp.bbmodel", "select": "missing"})
        ),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.code(
            "model_get",
            json!({"model": "lamp.bbmodel", "select": "e99999"})
        ),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.code(
            "model_get",
            json!({"model": "lamp.bbmodel", "select": ["base", "missing"]})
        ),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.edit_code("lamp.bbmodel", json!([{"op": "delete", "select": "no/*"}])),
        "E_NO_MATCH"
    );
    // An empty list names nothing: it is a bad argument, not a selection that is empty.
    assert_eq!(
        client.code("model_get", json!({"model": "lamp.bbmodel", "select": []})),
        "E_INPUT"
    );
    for op in ["delete", "duplicate"] {
        assert_eq!(
            client.edit_code("lamp.bbmodel", json!([{"op": op, "select": []}])),
            "E_INPUT",
            "{op}"
        );
    }
    assert_eq!(
        client.edit_code(
            "lamp.bbmodel",
            json!([{"op": "move", "select": [], "by": [1, 0, 0]}])
        ),
        "E_INPUT"
    );
    // Two objects with one path.
    client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "duplicate", "select": "base"},
        ]),
    );
    assert_eq!(
        client.code(
            "model_get",
            json!({"model": "lamp.bbmodel", "select": "base"})
        ),
        "E_AMBIGUOUS"
    );
    assert_eq!(
        client.edit_code("lamp.bbmodel", json!([{"op": "delete", "select": "base"}])),
        "E_AMBIGUOUS"
    );
    let both = client.ok(
        "model_get",
        json!({"model": "lamp.bbmodel", "select": "b*"}),
    );
    assert_eq!(both["total"], 2);
}

#[test]
fn model_find_filters_by_name_group_box_texture_and_untextured_faces() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    let find = |client: &mut Client<'_>, filter: Value| {
        let mut arguments = json!({"model": "lamp.bbmodel"});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(filter.as_object().unwrap().clone());
        let found = client.ok("model_find", arguments);
        let paths: Vec<String> = found["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["path"].as_str().unwrap().to_owned())
            .collect();
        (found["total"].as_u64().unwrap(), paths)
    };
    assert_eq!(
        find(&mut client, json!({"name": "*arm*"})).1,
        ["lamp_arm", "lamp_arm/arm_left"]
    );
    assert_eq!(
        find(&mut client, json!({"name": "lamp_arm/s*"})).1,
        ["lamp_arm/shade"]
    );
    assert_eq!(
        find(&mut client, json!({"group": "lamp_arm"})).1,
        ["lamp_arm/arm_left", "lamp_arm/shade"]
    );
    assert_eq!(find(&mut client, json!({"texture": label})).0, 3);
    assert_eq!(find(&mut client, json!({"texture": "none"})).0, 0);
    assert_eq!(
        find(&mut client, json!({"inside": [[0, 0, 0], [16, 16, 16]]})).0,
        3
    );
    assert_eq!(
        find(&mut client, json!({"inside": [[5, 0, 5], [11, 11, 11]]})).1,
        ["lamp_arm/arm_left", "lamp_arm/shade"]
    );
    assert_eq!(
        find(&mut client, json!({"inside": [[5, 10, 5], [11, 11, 11]]})).1,
        ["lamp_arm/shade"]
    );
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "add_cube", "name": "bare", "from": [0, 0, 0], "to": [1, 1, 1]}]),
    );
    // The lamp's elements have faces that are on and have no texture; a new cube has the
    // texture of the model on all six.
    let bare = find(&mut client, json!({"untextured": true}));
    assert_eq!(bare.1, ["base", "lamp_arm/arm_left", "lamp_arm/shade"]);
    let textured = find(&mut client, json!({"untextured": false}));
    assert_eq!(textured.1, ["bare"]);
}

#[test]
fn model_stats_counts_bounds_and_the_targets_of_the_brief() {
    let dir = project();
    let library = dir.path().join("cuboid-library");
    let mut lib = cuboid_library::Library::new(library, cuboid_library::Scope::Project);
    let mut brief = lib.brief().clone();
    brief.max_elements = Some(2);
    brief.texture_size = Some(16);
    lib.set_brief(brief);
    lib.save().expect("saves");
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let stats = client.ok("model_stats", json!({"model": "lamp.bbmodel"}));
    assert_eq!(
        (stats["elements"].as_u64(), stats["groups"].as_u64()),
        (Some(3), Some(1))
    );
    assert_eq!(numbers(&stats["bounds"]["from"]), [4.0, 0.0, 4.0]);
    assert_eq!(numbers(&stats["bounds"]["to"]), [12.0, 11.0, 12.0]);
    let targets = stats["targets"].as_array().unwrap();
    let met = |name: &str| {
        targets
            .iter()
            .find(|t| t["name"] == name)
            .map(|t| t["met"].clone())
    };
    assert_eq!(met("max_elements"), Some(json!(false)));
    assert_eq!(met("texture_size"), Some(json!(true)));
    assert_eq!(stats["style"]["elements"]["max"].as_f64(), Some(3.0));
    // The palette is met by the rule of the brief's suggestions: fewer than half of the colours
    // outside it. The lamp's texture is one red.
    assert_eq!(met("palette"), None);
    let mut brief = lib.brief().clone();
    brief.palette = vec![[205, 25, 20]];
    lib.set_brief(brief.clone());
    lib.save().expect("saves");
    let near = client.ok("model_stats", json!({"model": "lamp.bbmodel"}));
    let palette = near["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "palette")
        .unwrap();
    assert_eq!(
        (&palette["actual"], &palette["met"]),
        (&json!(0), &json!(true))
    );
    brief.palette = vec![[0, 0, 255]];
    lib.set_brief(brief);
    lib.save().expect("saves");
    let far = client.ok("model_stats", json!({"model": "lamp.bbmodel"}));
    let palette = far["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "palette")
        .unwrap();
    assert_eq!(
        (&palette["actual"], &palette["met"]),
        (&json!(1), &json!(false))
    );
}

#[test]
fn the_changed_counts_are_objects_that_were_there_not_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "cube.cubik", "create": "block"}),
    );
    // The size, position, pivot and name of a new cube are not changes to something old.
    let made = client.edit(
        "cube.cubik",
        json!([{"op": "add_cube", "name": "core", "from": [0, 0, 0], "to": [4, 4, 4]}]),
    );
    assert_eq!(
        made["changed"],
        json!({"added": 1, "updated": 0, "removed": 0})
    );
    let id = made["ids"]["core"].as_str().expect("an id").to_owned();
    client.edit(
        "cube.cubik",
        json!([{"op": "add_cube", "name": "second", "from": [5, 0, 0], "to": [6, 1, 1]}]),
    );
    // Four commands on one old cube and one on another count two; the cube made by this call
    // counts as added only.
    let result = client.edit(
        "cube.cubik",
        json!([
            {"op": "set", "select": id, "name": "core2", "from": [1, 1, 1], "to": [3, 3, 3]},
            {"op": "move", "select": "second", "by": [1, 0, 0]},
            {"op": "add_cube", "name": "third", "from": [8, 0, 0], "to": [9, 1, 1]},
            {"op": "set", "select": "third", "name": "third2"},
        ]),
    );
    assert_eq!(
        result["changed"],
        json!({"added": 1, "updated": 2, "removed": 0})
    );
    // An object that is changed and then deleted is only removed.
    let removed = client.edit(
        "cube.cubik",
        json!([
            {"op": "set", "select": "second", "name": "gone"},
            {"op": "delete", "select": "gone"},
        ]),
    );
    assert_eq!(
        removed["changed"],
        json!({"added": 0, "updated": 0, "removed": 1})
    );
}

#[test]
fn a_cube_added_and_changed_reads_back_exactly_in_model_space() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "cube.cubik", "create": "block"}),
    );
    let made = client.edit(
        "cube.cubik",
        json!([{"op": "add_cube", "name": "core", "from": [0, 0, 0], "to": [4, 4, 4]}]),
    );
    assert_eq!(made["changed"]["added"], 1);
    let id = made["ids"]["core"].as_str().expect("an id").to_owned();
    let core = client.get("cube.cubik", &id);
    assert_eq!(
        (numbers(&core["from"]), numbers(&core["to"])),
        (vec![0.0; 3], vec![4.0; 3])
    );
    client.edit(
        "cube.cubik",
        json!([{"op": "set", "select": id, "from": [1, 1, 1], "to": [5, 6, 7]}]),
    );
    let core = client.get("cube.cubik", &id);
    assert_eq!(
        (numbers(&core["from"]), numbers(&core["to"])),
        (vec![1.0; 3], vec![5.0, 6.0, 7.0])
    );
    client.edit(
        "cube.cubik",
        json!([{"op": "set", "select": "core", "rotation": [0, 45, 0], "pivot": [3, 3, 4]}]),
    );
    let core = client.get("cube.cubik", &id);
    assert_eq!(numbers(&core["rotation"]), [0.0, 45.0, 0.0]);
    assert_eq!(numbers(&core["pivot"]), [3.0, 3.0, 4.0]);
    assert_eq!(
        (numbers(&core["from"]), numbers(&core["to"])),
        (vec![1.0; 3], vec![5.0, 6.0, 7.0])
    );
    // A move is in the same space: z grows to the south.
    client.edit(
        "cube.cubik",
        json!([{"op": "move", "select": id, "by": [1, 2, 3]}]),
    );
    let core = client.get("cube.cubik", &id);
    assert_eq!(numbers(&core["from"]), [2.0, 3.0, 4.0]);
    assert_eq!(numbers(&core["to"]), [6.0, 8.0, 10.0]);
    // A pivot alone moves the pivot and not the geometry.
    let made = client.edit(
        "cube.cubik",
        json!([{"op": "add_cube", "name": "flat", "from": [2, 2, 2], "to": [6, 4, 8]}]),
    );
    let flat = made["ids"]["flat"].as_str().expect("an id").to_owned();
    client.edit(
        "cube.cubik",
        json!([{"op": "set", "select": flat, "pivot": [0, 0, 0]}]),
    );
    let flat = client.get("cube.cubik", &flat);
    assert_eq!(numbers(&flat["pivot"]), [0.0, 0.0, 0.0]);
    assert_eq!(
        (numbers(&flat["from"]), numbers(&flat["to"])),
        (vec![2.0; 3], vec![6.0, 4.0, 8.0])
    );
}

#[test]
fn a_rotation_the_version_does_not_allow_is_rounded_and_the_call_says_so() {
    let dir = tempfile::tempdir().unwrap();
    // The version of the brief decides the rule: this one turns one axis in steps of 22.5.
    let mut library = cuboid_library::Library::new(
        dir.path().join("cuboid-library"),
        cuboid_library::Scope::Project,
    );
    let mut brief = library.brief().clone();
    brief.mc_version = "1.20.4".to_owned();
    library.set_brief(brief);
    library.save().expect("saves");
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "cube.cubik", "create": "block"}),
    );
    let made = client.edit(
        "cube.cubik",
        json!([{"op": "add_cube", "name": "a", "from": [0, 0, 0], "to": [2, 2, 2]}]),
    );
    let id = made["ids"]["a"].as_str().unwrap().to_owned();
    let result = client.edit(
        "cube.cubik",
        json!([{"op": "set", "select": id, "rotation": [0, 30, 0]}]),
    );
    let warning = &result["warnings"][0];
    assert_eq!(warning["code"], "W_ROTATION_SNAPPED");
    assert_eq!(warning["op"], 0);
    // It says what was kept and whose rule did it, not that a version has one fixed rule.
    let hint = warning["hint"].as_str().unwrap();
    assert!(
        hint.contains("rotation rule of this model's version"),
        "{hint}"
    );
    assert!(hint.contains("22.5") && hint.contains("[0.0, "), "{hint}");
    let turned = numbers(&client.get("cube.cubik", &id)["rotation"]);
    assert!(
        turned == [0.0, 22.5, 0.0] || turned == [0.0, 45.0, 0.0],
        "{turned:?}"
    );
    // Moving the turn to another axis goes through zero.
    client.edit(
        "cube.cubik",
        json!([{"op": "set", "select": id, "rotation": [22.5, 0, 0]}]),
    );
    assert_eq!(
        numbers(&client.get("cube.cubik", &id)["rotation"]),
        [22.5, 0.0, 0.0]
    );
}

#[test]
fn a_later_op_names_by_path_what_an_earlier_op_made() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let result = client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "add_cube", "name": "knob", "from": [7, 11, 7], "to": [9, 13, 9], "parent": "lamp_arm"},
            {"op": "set", "select": "lamp_arm/knob", "name": "top"},
            {"op": "group", "select": "lamp_arm/top", "name": "crown", "pivot": [8, 11, 8]},
            {"op": "move", "select": "lamp_arm/crown", "by": [0, 1, 0]},
        ]),
    );
    assert_eq!(result["changed"]["added"], 2);
    let ids = result["ids"].as_object().unwrap();
    assert!(
        ids.contains_key("lamp_arm/crown/top") && ids.contains_key("lamp_arm/crown"),
        "{ids:?}"
    );
    let top = client.get("lamp.bbmodel", "**/top");
    assert_eq!(numbers(&top["from"]), [7.0, 12.0, 7.0]);
    let crown = client.get("lamp.bbmodel", "lamp_arm/crown");
    assert_eq!(numbers(&crown["pivot"]), [8.0, 12.0, 8.0]);
}

#[test]
fn every_op_does_what_it_says() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    // set: name, visibility and lock of an element and a group.
    client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "set", "select": "base", "name": "foot", "visible": false, "locked": true},
            {"op": "set", "select": "lamp_arm", "visible": false},
        ]),
    );
    let foot = client.get("lamp.bbmodel", "foot");
    assert_eq!(
        (foot["visible"].as_bool(), foot["locked"].as_bool()),
        (Some(false), Some(true))
    );
    assert_eq!(client.get("lamp.bbmodel", "lamp_arm")["visible"], false);
    // delete, duplicate, mirror, align, reparent.
    let result = client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "duplicate", "select": "lamp_arm/shade"},
            {"op": "mirror", "select": "foot", "axis": "x", "copy": true},
            {"op": "delete", "select": "lamp_arm/arm_left"},
        ]),
    );
    assert_eq!(result["changed"]["removed"], 1);
    assert!(result["changed"]["added"].as_u64().unwrap() >= 2);
    let outline = client.ok("model_outline", json!({"model": "lamp.bbmodel"}));
    assert_eq!(outline["total"], 4);
    let shades = client.ok(
        "model_find",
        json!({"model": "lamp.bbmodel", "name": "shade"}),
    );
    assert_eq!(shades["total"], 2);
    let ids: Vec<String> = shades["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "reparent", "select": ids[1], "parent": null}]),
    );
    assert_eq!(client.get("lamp.bbmodel", &ids[1])["path"], "shade");
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "reparent", "select": ids[1], "parent": "lamp_arm", "index": 0}]),
    );
    assert_eq!(
        client.get("lamp.bbmodel", &ids[1])["path"],
        "lamp_arm/shade"
    );
    client.edit("lamp.bbmodel", json!([{"op": "align", "select": [ids[0].clone(), ids[1].clone()], "axis": "y", "edge": "min"}]));
    let (a, b) = (
        client.get("lamp.bbmodel", &ids[0]),
        client.get("lamp.bbmodel", &ids[1]),
    );
    assert_eq!(a["from"][1], b["from"][1]);
    // add_texture and remove_texture.
    let added = client.edit(
        "lamp.bbmodel",
        json!([{"op": "add_texture", "name": "body", "size": [8, 4], "fill": "#808080"}]),
    );
    assert_eq!(added["textures"], json!(["body"]));
    let texture = client.ok(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": "body", "scale": 1}),
    );
    assert_eq!(texture["size"], json!([8, 4]));
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "remove_texture", "name": "body"}]),
    );
    assert_eq!(
        client.edit_code(
            "lamp.bbmodel",
            json!([{"op": "remove_texture", "name": "body"}])
        ),
        "E_NO_MATCH"
    );
    assert!(!label.is_empty());
}

#[test]
fn faces_take_a_texture_a_rectangle_in_texels_and_the_other_face_settings() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "add_texture", "name": "body", "size": [32, 16]}]),
    );
    client.edit(
        "lamp.bbmodel",
        json!([{
            "op": "set_face", "select": "base", "faces": ["north", "south"], "texture": "body",
            "uv": [4, 2, 12, 6], "uv_rotation": 90
        }]),
    );
    let base = client.get("lamp.bbmodel", "base");
    assert_eq!(base["faces"]["north"], json!(["body", 4, 2, 12, 6, 90]));
    assert_eq!(base["faces"]["south"], json!(["body", 4, 2, 12, 6, 90]));
    // A rectangle alone keeps the texture; a rotation alone keeps the rectangle.
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set_face", "select": "base", "faces": ["north"], "uv_rotation": 180}]),
    );
    assert_eq!(
        client.get("lamp.bbmodel", "base")["faces"]["north"],
        json!(["body", 4, 2, 12, 6, 180])
    );
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set_face", "select": "base", "faces": ["up"], "enabled": false}]),
    );
    assert_eq!(
        client.get("lamp.bbmodel", "base")["disabled"],
        json!(["up"])
    );
    client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "set_face", "select": "base", "faces": ["south"], "cull": "south", "tint": 0},
            {"op": "flip_uv", "select": "base", "faces": ["north"], "axis": "u"},
            {"op": "auto_uv", "select": "lamp_arm/shade", "style": "per_face"},
        ]),
    );
    assert_ne!(
        client.get("lamp.bbmodel", "base")["faces"]["north"],
        json!(["body", 4, 2, 12, 6, 180])
    );
    assert_eq!(
        client.edit_code(
            "lamp.bbmodel",
            json!([{"op": "set_face", "select": "base", "texture": "nothing"}])
        ),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.edit_code(
            "lamp.bbmodel",
            json!([{"op": "set_face", "select": "base"}])
        ),
        "E_INPUT"
    );
    // Every face of a group's elements.
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set_face", "select": "lamp_arm", "texture": label.as_str()}]),
    );
    assert_eq!(
        client.get("lamp.bbmodel", "lamp_arm/shade")["faces"]["down"][0],
        label.as_str()
    );
}

#[test]
fn display_slots_take_values_and_a_preset() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set_display", "context": "gui", "rotation": [30, 225, 0], "translation": [0, 1.5, 0], "scale": [0.8, 0.8, 0.8]}]),
    );
    let outline = client.ok("model_outline", json!({"model": "lamp.bbmodel"}));
    let gui = outline["display"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["slot"] == "gui")
        .unwrap();
    assert_eq!(numbers(&gui["rotation"]), [30.0, 225.0, 0.0]);
    assert_eq!(numbers(&gui["scale"]), [0.8, 0.8, 0.8]);
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set_display", "context": "head", "preset": "item/handheld"}]),
    );
    assert_eq!(
        client.edit_code(
            "lamp.bbmodel",
            json!([{"op": "set_display", "context": "nowhere"}])
        ),
        "E_INPUT"
    );
}

#[test]
fn the_revision_is_checked_dry_runs_change_nothing_and_a_refused_call_changes_nothing() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let before = client.revision("lamp.bbmodel");
    let ops = json!([{"op": "add_cube", "name": "dry", "from": [0, 0, 0], "to": [1, 1, 1]}]);
    // A dry run reports what would happen and leaves the model and its revision alone.
    let dry = client.ok(
        "model_edit",
        json!({"model": "lamp.bbmodel", "ops": ops, "dry_run": true}),
    );
    assert_eq!(
        (dry["dry_run"].as_bool(), dry["revision"].as_str()),
        (Some(true), Some(before.as_str()))
    );
    assert_eq!(dry["changed"]["added"], 1);
    assert_eq!(
        client.ok("model_outline", json!({"model": "lamp.bbmodel"}))["total"],
        3
    );
    assert_eq!(client.revision("lamp.bbmodel"), before);
    assert!(client.undo_labels("lamp.bbmodel").is_empty());
    // The revision of the model the edit is based on.
    let session = before.split('-').next().unwrap().to_owned();
    let wrong = format!("{session}-1");
    assert_eq!(
        client.code(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": ops, "expected_revision": wrong})
        ),
        "E_REVISION"
    );
    assert_eq!(
        client.code(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": ops, "expected_revision": "deadbeef-1"})
        ),
        "E_SESSION"
    );
    let done = client.ok(
        "model_edit",
        json!({"model": "lamp.bbmodel", "ops": ops, "expected_revision": before}),
    );
    let after = done["revision"].as_str().unwrap().to_owned();
    assert_ne!(after, before);
    assert_eq!(client.revision("lamp.bbmodel"), after);
    // The old revision is now stale.
    assert_eq!(
        client.code(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": ops, "expected_revision": before})
        ),
        "E_REVISION"
    );
    // An op that is refused in the middle of a call undoes the call; the error names the op.
    let error = client
        .server
        .call(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [
                {"op": "add_cube", "name": "first", "from": [0, 0, 0], "to": [1, 1, 1]},
                {"op": "add_cube", "name": "second", "from": [0, 0, 0], "to": [1, 1, 1]},
                {"op": "delete", "select": "nothing"},
            ]}),
        )
        .expect_err("refused");
    assert_eq!((error.code, error.op), ("E_NO_MATCH", Some(2)));
    assert_eq!(client.revision("lamp.bbmodel"), after);
    assert_eq!(
        client.ok("model_outline", json!({"model": "lamp.bbmodel"}))["total"],
        4
    );
    // A refusal of the core rolls back too.
    let refused = client
        .server
        .call(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [
                {"op": "add_cube", "name": "third", "from": [0, 0, 0], "to": [1, 1, 1]},
                {"op": "set", "select": "third", "from": [5, 5, 5], "to": [1, 1, 1]},
            ]}),
        )
        .expect_err("refused");
    assert_eq!(refused.code, "E_INPUT");
    assert_eq!(client.revision("lamp.bbmodel"), after);
}

#[test]
fn a_call_is_one_undo_step_and_the_history_undoes_redoes_and_restores() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    client.edit(
        "lamp.bbmodel",
        json!([
            {"op": "add_cube", "name": "one", "from": [0, 0, 0], "to": [1, 1, 1]},
            {"op": "add_cube", "name": "two", "from": [1, 1, 1], "to": [2, 2, 2]},
            {"op": "remove_texture", "name": label},
        ]),
    );
    // The texture goes with the same step: undo brings it back with the cubes gone.
    assert_eq!(
        client.ok("project_open", json!({"model": "lamp.bbmodel"}))["textures"],
        json!([])
    );
    assert_eq!(client.undo_labels("lamp.bbmodel"), ["Agent edit (3 ops)"]);
    let outline = |client: &mut Client<'_>| {
        client.ok("model_outline", json!({"model": "lamp.bbmodel"}))["total"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(outline(&mut client), 5);
    let undone = client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "undo"}),
    );
    assert_eq!(
        (undone["steps"].as_u64(), undone["changed"].as_bool()),
        (Some(1), Some(true))
    );
    assert_eq!(outline(&mut client), 3);
    assert_eq!(
        client.code(
            "history",
            json!({"model": "lamp.bbmodel", "action": "undo"})
        ),
        "E_NOTHING_TO_UNDO"
    );
    let redone = client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "redo", "steps": 5}),
    );
    assert_eq!(redone["steps"], 1);
    assert_eq!(outline(&mut client), 5);
    // A checkpoint is restored as one step, and restoring the model as it is changes nothing.
    client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": "five"}),
    );
    assert_eq!(
        client.ok(
            "history",
            json!({"model": "lamp.bbmodel", "action": "restore", "name": "five"})
        )["changed"],
        false
    );
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "delete", "select": ["one", "two"]}]),
    );
    assert_eq!(outline(&mut client), 3);
    let restored = client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "restore", "name": "five"}),
    );
    assert_eq!(restored["changed"], true);
    assert_eq!(outline(&mut client), 5);
    assert_eq!(restored["undo"][0], "Restore five");
    client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "undo"}),
    );
    assert_eq!(outline(&mut client), 3);
    assert_eq!(
        client.code(
            "history",
            json!({"model": "lamp.bbmodel", "action": "restore", "name": "none"})
        ),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.code(
            "history",
            json!({"model": "lamp.bbmodel", "action": "checkpoint"})
        ),
        "E_INPUT"
    );
}

#[test]
fn a_model_keeps_at_most_16_checkpoints_and_a_call_at_most_500_ops() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    for n in 0..16 {
        client.ok(
            "history",
            json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": format!("c{n}")}),
        );
    }
    client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": "c3"}),
    );
    assert_eq!(
        client.code(
            "history",
            json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": "c16"})
        ),
        "E_LIMIT"
    );
    let ops: Vec<Value> = (0..501)
        .map(|_| json!({"op": "set", "select": "base", "name": "x"}))
        .collect();
    assert_eq!(client.edit_code("lamp.bbmodel", json!(ops)), "E_LIMIT");
    assert_eq!(client.edit_code("lamp.bbmodel", json!([])), "E_INPUT");
}

#[test]
fn a_ninth_model_closes_a_saved_one_and_not_one_with_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..10 {
        std::fs::write(
            dir.path().join(format!("m{n}.json")),
            r#"{"elements": [{"from": [0, 0, 0], "to": [1, 1, 1]}]}"#,
        )
        .unwrap();
    }
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    for n in 0..9 {
        client.ok("project_open", json!({"model": format!("m{n}.json")}));
    }
    let open = client.ok("project_list", json!({}));
    let open_count = open["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["open"] == true)
        .count();
    assert_eq!(open_count, 8);
    // The least recently used one was closed.
    assert_eq!(open["files"][0]["open"], false);
    // With unsaved edits in all eight there is no room.
    for n in 1..9 {
        client.edit(
            &format!("m{n}.json"),
            json!([{"op": "set", "select": "Element", "name": "x"}]),
        );
    }
    assert_eq!(
        client.code("project_open", json!({"model": "m9.json"})),
        "E_LIMIT"
    );
}

/// Ten small model files, `m0.json` to `m9.json`.
fn small_models(dir: &std::path::Path) {
    for n in 0..10 {
        std::fs::write(
            dir.join(format!("m{n}.json")),
            r#"{"elements": [{"from": [0, 0, 0], "to": [1, 1, 1]}]}"#,
        )
        .unwrap();
    }
}

#[test]
fn a_new_model_and_a_model_with_a_checkpoint_stay_open_when_others_open() {
    let dir = tempfile::tempdir().unwrap();
    small_models(dir.path());
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    // A model made from a template is not dirty, but it has no file to read it back from.
    client.ok(
        "project_open",
        json!({"model": "new.cubik", "create": "item"}),
    );
    // A saved model with a checkpoint holds something the file does not.
    client.ok(
        "history",
        json!({"model": "m0.json", "action": "checkpoint", "name": "c"}),
    );
    for n in 1..9 {
        client.ok("project_open", json!({"model": format!("m{n}.json")}));
    }
    client.ok("model_outline", json!({"model": "new.cubik"}));
    client.ok(
        "history",
        json!({"model": "m0.json", "action": "restore", "name": "c"}),
    );
    // Saved models without checkpoints did close, the oldest first.
    let listed = client.ok("project_list", json!({"glob": "m*.json"}));
    let open = |n: usize| listed["files"][n]["open"] == true;
    assert!(open(0) && !open(1) && !open(2) && open(3), "{listed}");
}

#[test]
fn a_saved_model_with_undo_steps_closes_after_one_without() {
    let dir = tempfile::tempdir().unwrap();
    small_models(dir.path());
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.edit(
        "m0.json",
        json!([{"op": "set", "select": "Element", "name": "x"}]),
    );
    client.ok("project_save", json!({"model": "m0.json"}));
    assert_eq!(client.undo_labels("m0.cubik").len(), 1);
    for n in 1..9 {
        client.ok("project_open", json!({"model": format!("m{n}.json")}));
    }
    // The ninth model made room: the oldest one without undo steps, not the older m0.cubik.
    assert_eq!(client.undo_labels("m0.cubik").len(), 1);
    let listed = client.ok("project_list", json!({"glob": "m1.json"}));
    assert_eq!(listed["files"][0]["open"], false);
}

#[test]
fn when_no_model_can_close_the_limit_says_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    small_models(dir.path());
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "a.cubik", "create": "item"}),
    );
    for n in 0..7 {
        client.ok("project_open", json!({"model": format!("m{n}.json")}));
    }
    client.edit(
        "m0.json",
        json!([{"op": "set", "select": "Element", "name": "x"}]),
    );
    for n in 1..7 {
        client.ok(
            "history",
            json!({"model": format!("m{n}.json"), "action": "checkpoint", "name": "c"}),
        );
    }
    // A new one, one with edits and six with checkpoints: nothing can close.
    let error = client
        .server
        .call("project_open", json!({"model": "m9.json"}))
        .expect_err("no room");
    assert_eq!(error.code, "E_LIMIT");
    assert!(error.hint.contains("project_save"), "{}", error.hint);
    client.ok("project_save", json!({"model": "a.cubik"}));
    client.ok("project_open", json!({"model": "m9.json"}));
}

#[test]
fn texture_paint_composes_pixels_in_one_step_and_shows_the_changed_region() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    // Away from the rectangles of the faces, which texture_view outlines.
    let grid = json!({
        "op": "paint_grid", "at": [10, 10],
        "legend": {".": null, "k": "#1d1b24", "h": "#d6dde8"},
        "rows": ["..kk", ".khk", "kkhk"]
    });
    let (result, images) = client.pictures(
        "texture_paint",
        json!({"model": "lamp.bbmodel", "texture": label, "ops": [
            {"op": "fill_rect", "rect": [0, 0, 16, 16], "color": "#00ff00"},
            grid,
        ]}),
    );
    assert_eq!(result["changed"][0]["region"], json!([0, 0, 16, 16]));
    assert_eq!(images.len(), 1);
    assert_eq!(client.undo_labels("lamp.bbmodel"), ["Agent paint (2 ops)"]);
    let (view, shown) = client.pictures(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": label, "scale": 1}),
    );
    assert_eq!(view["size"], json!([16, 16]));
    let picture = Picture::from_png(&shown[0]).expect("a png");
    assert_eq!(picture.get(15, 15), [0, 255, 0, 255]);
    assert_eq!(picture.get(12, 10), [0x1d, 0x1b, 0x24, 255]);
    assert_eq!(picture.get(12, 11), [0xd6, 0xdd, 0xe8, 255]);
    // Transparent pixels in the legend are written.
    assert_ne!(picture.get(10, 10), [0, 255, 0, 255]);
    // One undo brings the old picture back.
    client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "undo"}),
    );
    let (_, shown) = client.pictures(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": label, "scale": 1}),
    );
    assert_eq!(
        Picture::from_png(&shown[0]).unwrap().get(15, 15),
        [200, 20, 20, 255]
    );
}

#[test]
fn texture_paint_refuses_a_grid_outside_the_texture_and_a_dry_run_paints_nothing() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    let paint = |client: &mut Client<'_>, ops: Value, dry: bool| {
        client.server.call(
            "texture_paint",
            json!({"model": "lamp.bbmodel", "texture": label, "ops": ops, "dry_run": dry}),
        )
    };
    let outside =
        json!([{"op": "paint_grid", "at": [15, 15], "legend": {"k": "#000000"}, "rows": ["kk"]}]);
    assert_eq!(
        paint(&mut client, outside, false)
            .expect_err("outside")
            .code,
        "E_INPUT"
    );
    let unknown =
        json!([{"op": "paint_grid", "at": [0, 0], "legend": {"k": "#000000"}, "rows": ["kx"]}]);
    assert_eq!(
        paint(&mut client, unknown, false).expect_err("legend").code,
        "E_INPUT"
    );
    let long_key =
        json!([{"op": "paint_grid", "at": [0, 0], "legend": {"kk": "#000000"}, "rows": ["k"]}]);
    assert_eq!(
        paint(&mut client, long_key, false).expect_err("key").code,
        "E_INPUT"
    );
    let too_wide = json!([{"op": "paint_grid", "at": [0, 0], "legend": {"k": "#000000"}, "rows": ["k".repeat(513)]}]);
    assert_eq!(
        paint(&mut client, too_wide, false).expect_err("wide").code,
        "E_LIMIT"
    );
    let missing = client.server.call(
        "texture_paint",
        json!({"model": "lamp.bbmodel", "ops": [{"op": "fill_rect", "texture": "nothing", "rect": [0, 0, 1, 1], "color": "#000000"}]}),
    );
    assert_eq!(missing.expect_err("no texture").code, "E_NO_MATCH");
    let revision = client.revision("lamp.bbmodel");
    let fill = json!([{"op": "fill_rect", "rect": [0, 0, 4, 4], "color": "#ff00ff"}]);
    let dry = paint(&mut client, fill, true).expect("a dry run");
    assert_eq!(dry.value["dry_run"], true);
    assert_eq!(dry.images.len(), 1);
    assert_eq!(client.revision("lamp.bbmodel"), revision);
    let (_, shown) = client.pictures(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": label, "scale": 1}),
    );
    assert_eq!(
        Picture::from_png(&shown[0]).unwrap().get(1, 1),
        [200, 20, 20, 255]
    );
}

#[test]
fn texture_paint_imports_a_png_and_adds_the_texture_when_there_is_none() {
    let dir = project();
    let mut art = Picture::new(4, 2, [10, 20, 30, 255]);
    art.set(3, 1, [250, 0, 0, 255]);
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::write(dir.path().join("art/shade.png"), art.png().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("art/big.png"),
        Picture::new(1025, 1, [0; 4]).png().unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join("art/not.png"), b"hello").unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let result = client.ok(
        "texture_paint",
        json!({"model": "lamp.bbmodel", "ops": [{"op": "import_png", "texture": "shade", "path": "art/shade.png"}]}),
    );
    assert_eq!(result["changed"][0]["new"], true);
    let (view, shown) = client.pictures(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": "shade", "scale": 1}),
    );
    assert_eq!(view["size"], json!([4, 2]));
    assert_eq!(
        Picture::from_png(&shown[0]).unwrap().get(3, 1),
        [250, 0, 0, 255]
    );
    let import = |client: &mut Client<'_>, path: &str| {
        client.server.call(
            "texture_paint",
            json!({"model": "lamp.bbmodel", "ops": [{"op": "import_png", "texture": "shade", "path": path}]}),
        )
    };
    assert_eq!(
        import(&mut client, "art/big.png").expect_err("big").code,
        "E_LIMIT"
    );
    assert_eq!(
        import(&mut client, "art/not.png")
            .expect_err("not a png")
            .code,
        "E_INPUT"
    );
    assert_eq!(
        import(&mut client, "art/none.png").expect_err("none").code,
        "E_NO_MATCH"
    );
}

#[test]
fn texture_view_outlines_the_rectangles_of_the_faces_that_use_the_texture() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    let (view, shown) = client.pictures(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": label, "scale": 8}),
    );
    assert_eq!(view["scale"], 8);
    let rects = view["rects"].as_array().unwrap();
    let base_north = rects
        .iter()
        .find(|r| r["path"] == "base" && r["face"] == "north")
        .expect("a rectangle");
    assert_eq!(base_north["uv"], json!([0, 0, 8, 2]));
    let up = rects
        .iter()
        .find(|r| r["path"] == "base" && r["face"] == "up")
        .unwrap();
    assert_eq!(up["rotation"], 90);
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (128, 128));
    // The outline of a rectangle is drawn at its corner, and the texture shows inside it.
    assert_ne!(picture.get(0, 0), [200, 20, 20, 255]);
    assert_eq!(picture.get(100, 100), [200, 20, 20, 255]);
    let only = client.ok(
        "texture_view",
        json!({"model": "lamp.bbmodel", "texture": label, "select": "lamp_arm/*"}),
    );
    assert!(
        only["rects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["path"].as_str().unwrap().starts_with("lamp_arm/"))
    );
    assert_eq!(
        client.code(
            "texture_view",
            json!({"model": "lamp.bbmodel", "texture": "nothing"})
        ),
        "E_NO_MATCH"
    );
    // The selector is `select` like everywhere else; `faces` is not a field.
    assert_eq!(
        client.code(
            "texture_view",
            json!({"model": "lamp.bbmodel", "texture": label, "faces": ["north"]})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "texture_view",
            json!({"model": "lamp.bbmodel", "texture": label, "select": "nothing_here"})
        ),
        "E_NO_MATCH"
    );
}

#[test]
fn texture_view_fits_a_large_texture_or_scale_to_1024_px_before_it_enlarges_anything() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bbmodel"), common::big_model(4096)).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "flat.cubik", "create": "block"}),
    );
    client.edit(
        "flat.cubik",
        json!([
            {"op": "add_texture", "name": "wide", "size": 1024, "fill": "#336699"},
            {"op": "add_texture", "name": "small", "size": 128, "fill": "#993366"},
        ]),
    );
    let start = std::time::Instant::now();
    // 1024 px at scale 16 would be 16384 px, a gigabyte of pixels.
    let (wide, shown) = client.pictures(
        "texture_view",
        json!({"model": "flat.cubik", "texture": "wide", "scale": 16}),
    );
    assert_eq!(wide["scale"], 1);
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (1024, 1024));
    // 128 px takes the scale that reaches 1024 px.
    let (small, shown) = client.pictures(
        "texture_view",
        json!({"model": "flat.cubik", "texture": "small", "scale": 16}),
    );
    assert_eq!(small["scale"], 8);
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (1024, 1024));
    // 4096 px is reduced to 1024 px at once, and its rectangle is drawn at the reduced size.
    let opened = client.ok("project_open", json!({"model": "big.bbmodel"}));
    let label = opened["textures"][0]["name"].as_str().unwrap().to_owned();
    let (big, shown) = client.pictures(
        "texture_view",
        json!({"model": "big.bbmodel", "texture": label, "scale": 16}),
    );
    assert_eq!(big["scale"], 1);
    assert_eq!(big["size"], json!([4096, 4096]));
    assert_eq!(big["rects"][0]["uv"], json!([0, 0, 2048, 2048]));
    assert!(shown[0].len() < 2 << 20, "{} bytes", shown[0].len());
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (1024, 1024));
    // The rectangle covers a quarter of the texture: its outline is at 512, not 2048.
    let outline = [230, 25, 75, 255];
    assert_eq!(picture.get(512, 100), outline);
    assert_eq!(picture.get(100, 512), outline);
    assert_eq!(picture.get(700, 700), [20, 120, 200, 255]);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn the_pictures_of_texture_paint_are_at_most_1024_px_for_any_region() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bbmodel"), common::big_model(2048)).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let opened = client.ok("project_open", json!({"model": "big.bbmodel"}));
    let label = opened["textures"][0]["name"].as_str().unwrap().to_owned();
    let (painted, shown) = client.pictures(
        "texture_paint",
        json!({"model": "big.bbmodel", "texture": label, "ops": [
            {"op": "fill_rect", "rect": [0, 0, 2048, 2048], "color": "#ff0000"}
        ]}),
    );
    assert_eq!(painted["changed"][0]["region"], json!([0, 0, 2048, 2048]));
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (1024, 1024));
    // A small region is still enlarged to be seen.
    let (_, shown) = client.pictures(
        "texture_paint",
        json!({"model": "big.bbmodel", "texture": label, "ops": [
            {"op": "fill_rect", "rect": [0, 0, 4, 4], "color": "#00ff00"}
        ]}),
    );
    let picture = Picture::from_png(&shown[0]).unwrap();
    assert_eq!((picture.width, picture.height), (64, 64));
}

#[test]
fn a_dry_run_of_texture_paint_sends_the_commands_to_a_copy_and_changes_nothing() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = texture_of(&mut client);
    let before = client.revision("lamp.bbmodel");
    // The real run adds the texture under a free label; the dry run says which.
    let ops = json!([
        {"op": "import_png", "texture": "extra", "path": "extra.png"},
        {"op": "fill_rect", "texture": label, "rect": [0, 0, 2, 2], "color": "#102030"},
    ]);
    std::fs::write(
        dir.path().join("extra.png"),
        Picture::new(4, 4, [9, 9, 9, 255]).png().unwrap(),
    )
    .unwrap();
    let dry = client.ok(
        "texture_paint",
        json!({"model": "lamp.bbmodel", "dry_run": true, "ops": ops}),
    );
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["changed"][0]["new"], true);
    assert_eq!(client.revision("lamp.bbmodel"), before);
    assert_eq!(client.undo_labels("lamp.bbmodel").len(), 0);
    let real = client.ok(
        "texture_paint",
        json!({"model": "lamp.bbmodel", "ops": ops}),
    );
    assert_eq!(real["changed"], dry["changed"]);
}

#[test]
fn every_tool_has_a_schema_and_its_examples_read_and_fit() {
    for tool in TOOLS.iter() {
        let input = (tool.input)();
        let examples = input["examples"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has examples", tool.name));
        assert!(!examples.is_empty(), "{}", tool.name);
        for example in examples {
            (tool.parse)(example.clone())
                .unwrap_or_else(|error| panic!("{}: {example}: {error}", tool.name));
            conforms(&input, example)
                .unwrap_or_else(|why| panic!("{}: {example}: {why}", tool.name));
        }
        assert_eq!(input["type"], "object", "{}", tool.name);
        assert_eq!((tool.output)()["type"], "object", "{}", tool.name);
    }
    assert_eq!(TOOLS.len(), 18);
}

#[test]
fn the_list_of_tools_and_the_instructions_stay_small_enough_for_a_context() {
    let list = cuboid_mcp::tools::definitions();
    assert_eq!(list.len(), 18);
    let bytes = serde_json::to_string(&list).unwrap().len();
    assert!(bytes < 50_000, "tools/list is {bytes} bytes");
    assert!(cuboid_mcp::tools::INSTRUCTIONS.len() < 1500);
    let names: Vec<&str> = list
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 18, "names are unique");
    // A second import drops references and their copies, a save or an export can replace a file.
    let destructive: Vec<&str> = list
        .iter()
        .filter(|tool| tool["annotations"]["destructiveHint"] == true)
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        destructive,
        ["refs_import", "project_save", "project_export"]
    );
    for tool in &list {
        assert!(
            tool["name"]
                .as_str()
                .unwrap()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        );
        assert!(tool["description"].as_str().unwrap().len() < 400);
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "{}",
            tool["name"]
        );
    }
}

#[test]
fn a_call_with_an_unknown_field_is_refused() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    assert_eq!(
        client.code(
            "model_outline",
            json!({"model": "lamp.bbmodel", "colour": 1})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [{"op": "move", "select": "base", "by": [0, 0, 0], "speed": 2}]})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [{"op": "explode", "select": "base"}]})
        ),
        "E_INPUT"
    );
    assert_eq!(client.code("model_outline", json!({})), "E_INPUT");
}
