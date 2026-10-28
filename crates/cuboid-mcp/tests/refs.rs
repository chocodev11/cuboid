//! The brief and the style references, and the size of what the tools return.

mod common;

use std::fs;

use common::{Client, Fake, project};
use serde_json::{Value, json};

/// The most a result may weigh: 8,000 tokens at 3 bytes a token.
const BUDGET: usize = 24_000;

fn size(value: &Value) -> usize {
    value.to_string().len()
}

/// A Minecraft model with a few elements; `n` makes its content its own.
fn reference(n: usize) -> String {
    let mut elements = Vec::new();
    for at in 0..=(n % 5) {
        let to = [(n % 16) + 1, ((n / 16) % 16) + 1, at + 1];
        elements.push(format!(
            r##"{{"from": [0, 0, 0], "to": [{}, {}, {}], "faces": {{"north": {{"uv": [0, 0, 4, 4], "texture": "#t"}}}}}}"##,
            to[0], to[1], to[2]
        ));
    }
    format!(
        r##"{{"comment": "reference {n}", "textures": {{"t": "block/stone"}}, "elements": [{}]}}"##,
        elements.join(",")
    )
}

fn references(root: &std::path::Path, folder: &str, count: usize) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    for n in 0..count {
        fs::write(dir.join(format!("ref{n}.json")), reference(n)).unwrap();
    }
}

#[test]
fn a_folder_of_references_is_remembered_and_the_brief_lists_them() {
    let dir = project();
    references(dir.path(), "refs/swords", 3);
    fs::write(
        dir.path().join("refs/swords/tex.png"),
        cuboid_mcp::Picture::new(8, 8, [10, 20, 30, 255])
            .png()
            .unwrap(),
    )
    .unwrap();
    fs::write(dir.path().join("refs/swords/notes.exe"), b"MZ").unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let empty = client.ok("brief_get", json!({}));
    assert_eq!(
        (
            empty["total_entries"].as_u64(),
            empty["entries"].as_array().map(Vec::len)
        ),
        (Some(0), Some(0))
    );
    let first = client.ok(
        "refs_import",
        json!({"folder": "refs/swords", "licence": "CC0", "tags": ["sword"]}),
    );
    assert_eq!(first["added"], 4);
    assert_eq!(first["skipped"], 1);
    assert_eq!(first["folder"], "refs/swords");
    let brief = client.ok("brief_get", json!({}));
    assert_eq!(brief["total_entries"], 4);
    assert_eq!(brief["style_sources"][0]["path"], "refs/swords");
    assert_eq!(brief["style_sources"][0]["licence"], "CC0");
    assert_eq!(brief["style"]["models"], 3);
    assert!(
        brief["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["role"] == "style" && e["tags"] == json!(["sword"]))
    );
    assert!(
        brief["style"]["typical"]
            .as_array()
            .is_some_and(|t| !t.is_empty())
    );
    // The same folder again is read again: nothing new.
    let again = client.ok(
        "refs_import",
        json!({"folder": "refs/swords", "licence": "CC0", "tags": ["sword"]}),
    );
    assert_eq!(
        (again["added"].as_u64(), again["duplicates"].as_u64()),
        (Some(0), Some(4))
    );
    assert_eq!(
        client.ok("brief_get", json!({}))["style_sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Not a place for references.
    assert_eq!(
        client.code("refs_import", json!({"folder": "cuboid-library"})),
        "E_PATH"
    );
    assert_eq!(client.code("refs_import", json!({"folder": "."})), "E_PATH");
    assert_eq!(
        client.code("refs_import", json!({"folder": "nowhere"})),
        "E_PATH"
    );
}

#[test]
fn refs_get_gives_the_outline_of_a_model_reference_and_the_numbers_of_a_texture() {
    let dir = project();
    let many: Vec<String> = (0..30)
        .map(|n| format!(r#"{{"from": [0, 0, {n}], "to": [1, 1, {}]}}"#, n + 1))
        .collect();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(
        dir.path().join("refs/many.json"),
        format!(r#"{{"elements": [{}]}}"#, many.join(",")),
    )
    .unwrap();
    fs::write(
        dir.path().join("refs/tex.png"),
        cuboid_mcp::Picture::new(8, 4, [10, 20, 30, 255])
            .png()
            .unwrap(),
    )
    .unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok("refs_import", json!({"folder": "refs"}));
    let brief = client.ok("brief_get", json!({}));
    let id_of = |kind: &str| {
        brief["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let model = client.ok("refs_get", json!({"id": id_of("model")}));
    assert_eq!(model["role"], "style");
    assert_eq!(model["outline"]["total"], 30);
    assert_eq!(model["outline"]["elements"].as_array().unwrap().len(), 24);
    assert_eq!(model["outline"]["next"], "24");
    assert!(
        model["outline"]["elements"][0].get("id").is_none(),
        "a reference has no ids to edit by"
    );
    assert!(model["meta"]["style"].is_object());
    let rest = client.ok(
        "refs_get",
        json!({"id": id_of("model"), "cursor": "24", "limit": 1000}),
    );
    assert_eq!(rest["outline"]["elements"].as_array().unwrap().len(), 6);
    assert!(rest["outline"].get("next").is_none());
    let texture = client.ok("refs_get", json!({"id": id_of("texture")}));
    assert_eq!(
        (
            texture["meta"]["width"].as_u64(),
            texture["meta"]["height"].as_u64()
        ),
        (Some(8), Some(4))
    );
    assert!(texture["meta"]["palette"].is_array());
    assert!(texture.get("outline").is_none());
    assert_eq!(
        client.code("refs_get", json!({"id": "0123456789abcdef"})),
        "E_NO_MATCH"
    );
}

#[test]
fn three_hundred_references_page_through_the_brief_under_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    references(dir.path(), "refs", 300);
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let imported = client.ok("refs_import", json!({"folder": "refs", "licence": "CC0"}));
    assert_eq!(imported["added"], 300);
    assert!(size(&imported) < BUDGET);
    for limit in [None, Some(1000)] {
        let mut ids = std::collections::BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let mut arguments = json!({});
            if let Some(limit) = limit {
                arguments["limit"] = json!(limit);
            }
            if let Some(cursor) = &cursor {
                arguments["cursor"] = json!(cursor);
            }
            let page = client.ok("brief_get", arguments);
            assert!(
                size(&page) < BUDGET,
                "page {pages} is {} bytes",
                size(&page)
            );
            assert_eq!(page["total_entries"], 300);
            assert_eq!(
                pages == 0,
                page.get("style").is_some(),
                "the summary is on the first page only"
            );
            ids.extend(
                page["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["id"].as_str().unwrap().to_owned()),
            );
            pages += 1;
            match page["next"].as_str() {
                Some(next) => cursor = Some(next.to_owned()),
                None => break,
            }
        }
        assert_eq!(ids.len(), 300);
        assert!(pages >= 6, "{pages} pages");
    }
}

/// A model of `count` elements with the longest names, six textured faces and a rotation, the
/// first `groups` of them each in a group of its own.
fn busy(client: &mut Client<'_>, count: usize, groups: usize) {
    client.ok(
        "project_open",
        json!({"model": "busy.cubik", "create": "block"}),
    );
    for start in (0..count).step_by(100) {
        let mut ops = Vec::new();
        for n in start..(start + 100).min(count) {
            let name = format!("a_rather_long_element_name_for_the_size_test_{n}");
            ops.push(json!({"op": "add_cube", "name": name, "from": [n % 16, n % 7, 0], "to": [n % 16 + 1, n % 7 + 2, 3], "texture": "texture"}));
            ops.push(
                json!({"op": "set", "select": name, "rotation": [0, 22.5, 0], "pivot": [8, 8, 8]}),
            );
        }
        client.edit("busy.cubik", json!(ops));
    }
    let ops: Vec<Value> = (0..groups)
        .map(|n| json!({"op": "group", "select": format!("a_rather_long_element_name_for_the_size_test_{n}"), "name": format!("group_with_a_long_name_{n}")}))
        .collect();
    client.edit("busy.cubik", json!(ops));
}

#[test]
fn what_the_model_tools_return_stays_under_the_budget_for_a_large_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    busy(&mut client, 300, 12);
    let first = client.ok(
        "model_outline",
        json!({"model": "busy.cubik", "limit": 1000}),
    );
    assert_eq!(first["elements"].as_array().unwrap().len(), 48);
    assert!(size(&first) < BUDGET, "outline page {}", size(&first));
    let default = client.ok("model_outline", json!({"model": "busy.cubik"}));
    assert!(
        size(&default) < size(&first) && size(&default) < 14_000,
        "{}",
        size(&default)
    );
    let later = client.ok(
        "model_outline",
        json!({"model": "busy.cubik", "cursor": "48", "limit": 1000}),
    );
    assert!(size(&later) < BUDGET);
    let get = client.ok("model_get", json!({"model": "busy.cubik", "select": "a_rather_long_element_name_for_the_size_test_*", "limit": 1000}));
    assert_eq!(get["objects"].as_array().unwrap().len(), 32);
    assert!(size(&get) < BUDGET, "model_get {}", size(&get));
    let find = client.ok(
        "model_find",
        json!({"model": "busy.cubik", "name": "*", "limit": 1000}),
    );
    assert_eq!(find["matches"].as_array().unwrap().len(), 300);
    assert!(size(&find) < BUDGET, "model_find {}", size(&find));
    let view = client.ok(
        "texture_view",
        json!({"model": "busy.cubik", "texture": "texture"}),
    );
    assert!(size(&view) < BUDGET, "texture_view {}", size(&view));
    let stats = client.ok("model_stats", json!({"model": "busy.cubik"}));
    assert!(size(&stats) < BUDGET);
    let history = client.ok("history", json!({"model": "busy.cubik", "action": "list"}));
    assert!(size(&history) < BUDGET);
}

#[test]
fn the_first_outline_page_of_200_groups_lists_60_of_them_and_stays_under_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    busy(&mut client, 300, 200);
    let default = client.ok("model_outline", json!({"model": "busy.cubik"}));
    assert_eq!(default["groups"].as_array().unwrap().len(), 60);
    assert_eq!(default["groups_total"], 200);
    assert!(size(&default) < BUDGET, "outline page {}", size(&default));
    // A narrow selection lists the groups it holds, all of them when they are few.
    let one = client.ok(
        "model_outline",
        json!({"model": "busy.cubik", "select": "group_with_a_long_name_150"}),
    );
    assert_eq!(one["groups"].as_array().unwrap().len(), 1);
    assert!(one.get("groups_total").is_none());
    // The pages after the first carry no groups, as before.
    let later = client.ok(
        "model_outline",
        json!({"model": "busy.cubik", "cursor": "24"}),
    );
    assert!(later.get("groups").is_none() && later.get("groups_total").is_none());
    // The numbers still count every group.
    let stats = client.ok("model_stats", json!({"model": "busy.cubik"}));
    assert_eq!(stats["groups"], 200);
}

#[test]
fn a_page_of_validator_issues_and_a_page_of_the_file_list_stay_under_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..300 {
        fs::write(
            dir.path()
                .join(format!("a_model_with_a_long_file_name_number_{n}.json")),
            reference(n),
        )
        .unwrap();
    }
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let list = client.ok("project_list", json!({"limit": 1000}));
    assert_eq!(list["total"], 300);
    assert!(size(&list) < BUDGET, "project_list {}", size(&list));
    let narrow = client.ok("project_list", json!({"glob": "*number_1?.json"}));
    assert_eq!(narrow["total"], 10);
    client.ok(
        "project_open",
        json!({"model": "issues.cubik", "create": "block"}),
    );
    let ops: Vec<Value> = (0..120)
        .map(|n| json!({"op": "add_cube", "name": format!("far_outside_the_block_{n}"), "from": [-30 - n, 0, 0], "to": [40, 2, 2], "texture": "texture"}))
        .collect();
    client.edit("issues.cubik", json!(ops));
    let issues = client.ok("validate", json!({"model": "issues.cubik", "limit": 1000}));
    assert!(issues["issues"].as_array().unwrap().len() >= 40);
    assert!(size(&issues) < BUDGET, "validate {}", size(&issues));
}
