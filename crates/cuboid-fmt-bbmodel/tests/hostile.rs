//! Projects that are damaged or built to hurt: the import returns an error or a notice, never
//! panics, and checks sizes before it allocates.

mod common;

use std::path::Path;

use common::{NoFiles, data_url, elements_of, import_json, open, text_of};
use cuboid_fmt_bbmodel::data_url::{decode, encode};
use cuboid_fmt_bbmodel::{BbError, DiskFiles, Limits, Notice, import, import_with};
use cuboid_io::texture_files::Refusal;
use serde_json::{Value, json};

fn project(elements: Value) -> Value {
    json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "elements": elements,
    })
}

fn try_import(text: &[u8], limits: &Limits) -> Result<cuboid_fmt_bbmodel::BbImport, BbError> {
    import_with(text, Path::new("hostile.bbmodel"), &NoFiles, limits)
}

#[test]
fn text_that_is_no_project_is_an_error() {
    let limits = Limits::default();
    for text in [
        &b""[..],
        b"   ",
        b"not json at all",
        b"[1, 2, 3]",
        b"{}",
        b"{\"meta\": 3}",
        b"{\"meta\": {}",
        b"\xEF\xBB\xBF{\"meta\": {}, \"elements\": [",
        b"\xFF\xFE\x00\x01",
    ] {
        assert!(try_import(text, &limits).is_err(), "{text:?}");
    }
}

#[test]
fn a_byte_order_mark_is_skipped() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(text_of(&project(json!([]))));
    assert!(try_import(&bytes, &Limits::default()).is_ok());
}

#[test]
fn a_compressed_backup_is_named_as_such() {
    let result = try_import(b"<lz>ABCDEF", &Limits::default());
    assert_eq!(result.unwrap_err(), BbError::Compressed);
}

#[test]
fn a_file_over_the_size_limit_is_refused_unread() {
    let limits = Limits {
        max_text: 10,
        ..Limits::default()
    };
    let result = try_import(&text_of(&project(json!([]))), &limits);
    assert_eq!(result.unwrap_err(), BbError::Limit("the file size"));
}

#[test]
fn a_flood_of_values_is_refused_before_the_tree_is_built() {
    let flood = format!("{{\"meta\":{{}},\"elements\":[{}0]}}", "0,".repeat(100_000));
    let limits = Limits {
        max_values: 1000,
        ..Limits::default()
    };
    assert_eq!(
        try_import(flood.as_bytes(), &limits).unwrap_err(),
        BbError::Limit("values")
    );
}

#[test]
fn too_many_elements_textures_and_mesh_items_are_refused() {
    let cubes: Vec<Value> = (0..20)
        .map(|_| json!({"from": [0, 0, 0], "to": [1, 1, 1]}))
        .collect();
    let limits = Limits {
        max_objects: 10,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&project(Value::Array(cubes))), &limits).unwrap_err(),
        BbError::Limit("elements")
    );

    let mut with_textures = project(json!([]));
    with_textures["textures"] = Value::Array((0..20).map(|_| json!({"name": "a.png"})).collect());
    let limits = Limits {
        max_textures: 5,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&with_textures), &limits).unwrap_err(),
        BbError::Limit("texture entries")
    );

    let vertices: serde_json::Map<String, Value> = (0..50)
        .map(|i| (format!("v{i}"), json!([i, 0, 0])))
        .collect();
    let mesh = project(json!([{
        "type": "mesh", "vertices": vertices,
        "faces": {"f": {"vertices": ["v0", "v1", "v2"], "uv": {}}}
    }]));
    let limits = Limits {
        max_mesh_items: 20,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&mesh), &limits).unwrap_err(),
        BbError::Limit("mesh data")
    );
}

#[test]
fn deeply_nested_groups_are_refused() {
    // 40 groups deep is within what JSON allows and beyond the limit set here.
    let mut tree = json!(["e0"]);
    for _ in 0..40 {
        tree = json!([{"uuid": "x", "children": tree}]);
    }
    let mut p = project(json!([{"uuid": "e0", "from": [0, 0, 0], "to": [1, 1, 1]}]));
    p["outliner"] = tree;
    assert_eq!(
        try_import(
            &text_of(&p),
            &Limits {
                max_depth: 10,
                ..Limits::default()
            }
        )
        .unwrap_err(),
        BbError::Limit("group depth")
    );
}

#[test]
fn json_nested_beyond_any_sense_is_a_syntax_error() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let text = format!("{{\"meta\":{{}},\"elements\":{deep}}}");
    assert!(matches!(
        try_import(text.as_bytes(), &Limits::default()),
        Err(BbError::Syntax(_))
    ));
}

#[test]
fn a_group_cycle_through_the_same_element_builds_it_once() {
    let mut p = project(json!([{"uuid": "e0", "from": [0, 0, 0], "to": [1, 1, 1]}]));
    p["outliner"] = json!(["e0", "e0", {"uuid": "g", "children": ["e0", 0]}, 0]);
    let doc = open(import_json(&p));
    assert_eq!(doc.elements.len(), 1);
}

#[test]
fn damaged_textures_become_checkerboards_with_a_notice() {
    let good = data_url(2, 2, [1, 2, 3, 255]);
    let truncated = &good[..good.len() - 12];
    let mut huge_header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    huge_header.extend_from_slice(b"IHDR");
    huge_header.extend_from_slice(&[
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 8, 6, 0, 0, 0,
    ]);
    huge_header.extend_from_slice(&[0, 0, 0, 0]);
    let huge = format!("data:image/png;base64,{}", encode(&huge_header));
    let mut p = project(json!([]));
    p["textures"] = json!([
        {"name": "a.png", "source": "data:image/png;base64,@@@@"},
        {"name": "b.png", "source": truncated},
        {"name": "c.png", "source": huge},
        {"name": "d.png", "source": "data:text/html;base64,PGI+"},
        {"name": "e.png", "source": 5},
        {"name": "f.png"},
        {"name": "g.png", "source": good},
    ]);
    let imported = import_json(&p);
    let missing = imported
        .notices
        .iter()
        .filter(|n| matches!(n, Notice::TextureMissing { .. }))
        .count();
    assert_eq!(missing, 6, "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.textures.entries.len(), 7);
    assert_eq!(doc.textures.entries[6].texture.image.w, 2);
    assert_eq!(doc.textures.entries[0].texture.image.w, 16);
}

#[test]
fn the_texel_budget_is_enforced() {
    let mut p = project(json!([]));
    p["textures"] = Value::Array(
        (0..4)
            .map(|_| json!({"name": "a.png", "source": data_url(32, 32, [0, 0, 0, 255])}))
            .collect(),
    );
    let limits = Limits {
        max_texels: 2500,
        ..Limits::default()
    };
    let imported = try_import(&text_of(&p), &limits).unwrap();
    assert!(imported.notices.contains(&Notice::TexelBudget));
}

#[test]
fn paths_to_the_network_and_to_devices_are_refused() {
    for (path, why) in [
        (r"\\host\share\a.png", Refusal::Network),
        ("//host/share/a.png", Refusal::Network),
        (r"\\.\COM1", Refusal::Device),
        (r"\\?\C:\a.png", Refusal::Device),
        ("C:a.png", Refusal::Device),
        ("textures/nul.png", Refusal::Device),
        ("a.png:stream", Refusal::Device),
        ("/dev/zero", Refusal::Device),
    ] {
        for key in ["relative_path", "path"] {
            let mut p = project(json!([]));
            p["textures"] = json!([{ "name": "a.png", key: path }]);
            let imported = import(&text_of(&p), Path::new("m.bbmodel"), &DiskFiles).unwrap();
            assert!(
                imported.notices.iter().any(|n| matches!(
                    n,
                    Notice::TextureRefused { reason, .. } if *reason == why
                )),
                "{path} ({key}): {:?}",
                imported.notices
            );
        }
    }
}

#[test]
fn a_file_over_the_cap_and_a_folder_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("folder.png")).unwrap();
    let file = dir.path().join("m.bbmodel");
    let mut p = project(json!([]));
    p["textures"] = json!([{"name": "a.png", "relative_path": "folder.png"}]);
    let imported = import(&text_of(&p), &file, &DiskFiles).unwrap();
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::TextureMissing { .. })),
        "{:?}",
        imported.notices
    );
}

#[test]
fn hostile_numbers_do_no_harm() {
    let big = 1.0e300;
    let p = project(json!([{
        "name": "x", "uuid": "e0",
        "from": [big, -big, 1e9], "to": [1e9, 1e9, 1e9], "origin": [big, 0, 0],
        "rotation": [1e9, -1e9, big], "inflate": big, "stretch": [big, 0, -big],
        "uv_offset": [big, big], "box_uv": true,
        "faces": {
            "north": {"uv": [big, -big, 1e12, 5], "texture": 1e20, "rotation": big,
                      "tint": -1e30, "cullface": 5},
            "south": {"uv": "no", "texture": {"a": 1}, "rotation": "x"},
            "up": 17, "down": null
        },
        "color": -5
    }, {
        "type": "mesh", "origin": [big, 0, 0], "rotation": [big, 0, 0],
        "vertices": {"a": [big, 0, 0], "b": [0, 1e9, 0], "c": "bad", "d": [1]},
        "faces": {"f": {"vertices": ["a", "b", "a", 5, null, "zz"], "uv": {"a": [big, big]}},
                  "g": 3, "h": {"vertices": "no"}}
    }, 5, "text", null, {"type": 7}, {"type": "cube", "from": "a"}]));
    let imported = import_json(&p);
    let doc = open(imported);
    assert!(doc.elements.len() >= 2);
}

#[test]
fn display_and_group_data_of_the_wrong_type_are_ignored() {
    let mut p = project(json!([{"uuid": "e0", "from": [0, 0, 0], "to": [1, 1, 1]}]));
    p["display"] = json!({"gui": 5, "head": {"rotation": "x", "scale": [1]}, "ground": [1]});
    p["groups"] = json!(["a", 5, {"uuid": 3}]);
    p["outliner"] =
        json!([{"children": 5}, {"children": [{"children": ["e0"]}], "name": 7}, [], null]);
    p["textures"] = json!({"not": "an array"});
    p["animations"] = json!("none");
    let doc = open(import_json(&p));
    assert_eq!(doc.elements.len(), 1);
}

#[test]
fn every_truncation_of_a_project_is_handled() {
    let mut p = project(json!([
        {"name": "a", "from": [0, 0, 0], "to": [4, 4, 4], "rotation": [0, 22.5, 0],
         "faces": {"north": {"uv": [0, 0, 4, 4], "texture": 0}}, "uuid": "e0"}
    ]));
    p["textures"] = json!([{"name": "t.png", "source": data_url(4, 4, [9, 9, 9, 255])}]);
    p["display"] = json!({"gui": {"scale": [1, 2, 3]}});
    let bytes = text_of(&p);
    for end in 0..bytes.len() {
        let _ = try_import(&bytes[..end], &Limits::default());
    }
    // And with every byte replaced in turn.
    let mut changed = bytes.clone();
    for at in (0..bytes.len()).step_by(7) {
        changed[at] = b'#';
        let _ = try_import(&changed, &Limits::default());
        changed[at] = bytes[at];
    }
}

#[test]
fn the_base64_decoder_checks_the_size_before_it_allocates() {
    assert_eq!(decode(&"A".repeat(1 << 24), 100), None);
    assert!(decode("AAAA", 3).is_some());
}

/// A project with the cubes `c0..` under the group tree `outliner`.
fn grouped(cubes: usize, outliner: Value) -> Value {
    let elements: Vec<Value> = (0..cubes)
        .map(|i| json!({"uuid": format!("c{i}"), "name": "cube", "from": [0, 0, 0], "to": [1, 1, 1]}))
        .collect();
    let mut p = project(Value::Array(elements));
    p["outliner"] = outliner;
    p
}

/// The longest name of an element or a group, in characters.
fn longest_label(imported: &cuboid_fmt_bbmodel::BbImport) -> usize {
    let groups = group_names(imported).into_iter().map(|n| n.chars().count());
    elements_of(imported)
        .iter()
        .map(|e| e.base.label.chars().count())
        .chain(groups)
        .max()
        .unwrap_or(0)
}

fn group_names(imported: &cuboid_fmt_bbmodel::BbImport) -> Vec<String> {
    imported
        .doc
        .sections
        .groups
        .as_ref()
        .map(|snap| snap.list.items.iter().map(|g| g.name.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn a_huge_group_name_is_cut_once() {
    let cubes = 2000;
    let ids: Vec<Value> = (0..cubes).map(|i| json!(format!("c{i}"))).collect();
    let p = grouped(
        cubes,
        json!([{"name": "x".repeat(2_000_000), "children": ids}]),
    );
    let imported = try_import(&text_of(&p), &Limits::default()).expect("imports");
    assert_eq!(elements_of(&imported).len(), cubes);
    assert!(longest_label(&imported) <= 64);
    assert!(
        imported
            .notices
            .contains(&Notice::LabelsShortened { count: 1 })
    );
}

#[test]
fn a_deep_tree_of_long_names_is_cut_group_by_group() {
    let mut tree = json!(["c0"]);
    for depth in (0..60).rev() {
        tree = json!([{"name": format!("g{depth}-{}", "y".repeat(5000)), "children": tree}]);
    }
    let imported = try_import(&text_of(&grouped(1, tree)), &Limits::default()).expect("imports");
    let names = group_names(&imported);
    assert_eq!(names.len(), 60);
    assert!(names.iter().all(|n| n.chars().count() <= 64));
    assert!(names[0].starts_with("g0-"), "{}", names[0]);
    assert!(names[0].ends_with('\u{2026}'));
    assert_eq!(&*elements_of(&imported)[0].base.label, "cube");
    assert!(
        imported
            .notices
            .contains(&Notice::LabelsShortened { count: 60 })
    );
}

#[test]
fn a_tree_deeper_than_the_limit_or_with_too_many_groups_is_refused() {
    // The JSON parser stops at 128 levels, so a lower limit shows the check of the importer.
    let mut tree = json!(["c0"]);
    for _ in 0..20 {
        tree = json!([{"name": "g", "children": tree}]);
    }
    let shallow = Limits {
        max_depth: 10,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&grouped(1, tree)), &shallow).unwrap_err(),
        BbError::Limit("group depth")
    );
    // The groups of one level: 65 537 of them are one more than the model holds.
    let many: Vec<Value> = (0..=cuboid_model::MAX_GROUPS)
        .map(|_| json!({"name": "g", "children": []}))
        .collect();
    let p = grouped(1, Value::Array(many));
    let generous = Limits {
        max_label_bytes: u64::MAX,
        max_values: u64::MAX,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&p), &generous).unwrap_err(),
        BbError::Limit("groups")
    );
}

#[test]
fn a_wide_tree_of_long_names_stays_within_the_label_budget() {
    let groups: Vec<Value> = (0..400)
        .map(|i| json!({"name": "z".repeat(10_000), "children": [format!("c{i}")]}))
        .collect();
    let p = grouped(400, Value::Array(groups));
    let imported = try_import(&text_of(&p), &Limits::default()).expect("imports");
    assert!(longest_label(&imported) <= 64);
    assert!(
        imported
            .notices
            .contains(&Notice::LabelsShortened { count: 400 })
    );

    // The total of the labels is charged against one budget, refused before it is built.
    let tight = Limits {
        max_label_bytes: 4000,
        ..Limits::default()
    };
    assert_eq!(
        try_import(&text_of(&p), &tight).unwrap_err(),
        BbError::Limit("element names")
    );
}

#[test]
fn short_names_are_kept_whole() {
    let p = grouped(
        1,
        json!([{"name": "arm", "children": [{"name": "hand", "children": ["c0"]}]}]),
    );
    let imported = try_import(&text_of(&p), &Limits::default()).expect("imports");
    assert_eq!(&*elements_of(&imported)[0].base.label, "cube");
    assert_eq!(group_names(&imported), ["arm", "hand"]);
    assert!(imported.notices.is_empty());
}
