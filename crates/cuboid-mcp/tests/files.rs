//! Saving, exporting, the check, the fixes, the read-only server and the journal.

mod common;

use std::fs;
use std::path::Path;

use common::{Client, Fake, conforms, project};
use cuboid_mcp::tools::TOOLS;
use serde_json::{Value, json};

/// Every file under the folder with its size, sorted.
fn listing(root: &Path) -> Vec<(String, u64)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, u64)>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, entry.metadata().unwrap().len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// A block model with a cube far outside the block, which the validator reports and can fix.
fn broken(client: &mut Client<'_>) {
    client.ok(
        "project_open",
        json!({"model": "bad.cubik", "create": "block"}),
    );
    client.edit(
        "bad.cubik",
        json!([
            {"op": "add_cube", "name": "wall", "from": [-20, 0, 0], "to": [40, 2, 2], "texture": "texture"},
            {"op": "add_cube", "name": "ok", "from": [4, 4, 4], "to": [8, 8, 8], "texture": "texture"},
        ]),
    );
}

#[test]
fn a_model_is_saved_as_cubik_and_opens_again_with_what_was_edited() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "add_cube", "name": "extra", "from": [0, 0, 0], "to": [1, 1, 1]}]),
    );
    let listed = client.ok("project_list", json!({}));
    assert_eq!(listed["files"][0]["unsaved"], true);
    let saved = client.ok("project_save", json!({"model": "lamp.bbmodel"}));
    assert_eq!(saved["path"], "lamp.cubik");
    assert!(saved["bytes"].as_u64().unwrap() > 100);
    assert!(dir.path().join("lamp.cubik").is_file());
    assert_eq!(saved["model"], "lamp.cubik");
    let listed = client.ok("project_list", json!({}));
    let names: Vec<&str> = listed["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["lamp.bbmodel", "lamp.cubik"]);
    assert_eq!(listed["files"][1]["unsaved"], false);
    // Another run of the server reads it back.
    let mut other_fake = Fake::default();
    let mut other = Client::new(dir.path(), false, &mut other_fake);
    let opened = other.ok("project_open", json!({"model": "lamp.cubik"}));
    assert_eq!(
        (opened["elements"].as_u64(), opened["groups"].as_u64()),
        (Some(4), Some(1))
    );
    assert_eq!(opened["format"], "cubik");
    assert_eq!(other.get("lamp.cubik", "extra")["name"], "extra");
    // The saved model is the open one: its history is still there.
    assert_eq!(client.undo_labels("lamp.cubik"), ["Agent edit (1 op)"]);
}

#[test]
fn a_save_over_a_file_that_changed_needs_overwrite() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok("project_save", json!({"model": "lamp.bbmodel"}));
    // The same model saved again over its own file is fine.
    client.ok("project_save", json!({"model": "lamp.cubik"}));
    fs::write(dir.path().join("lamp.cubik"), b"someone else was here").unwrap();
    assert_eq!(
        client.code("project_save", json!({"model": "lamp.cubik"})),
        "E_FILE_CHANGED"
    );
    client.ok(
        "project_save",
        json!({"model": "lamp.cubik", "overwrite": true}),
    );
    // A file that exists and is not this model's is not replaced either.
    fs::write(dir.path().join("other.cubik"), b"mine").unwrap();
    assert_eq!(
        client.code(
            "project_save",
            json!({"model": "lamp.cubik", "path": "other.cubik"})
        ),
        "E_FILE_CHANGED"
    );
    assert_eq!(fs::read(dir.path().join("other.cubik")).unwrap(), b"mine");
    // A path in a folder that does not exist yet is made; the model is named by it afterwards.
    let moved = client.ok(
        "project_save",
        json!({"model": "lamp.cubik", "path": "models/deep/lamp2.cubik"}),
    );
    assert_eq!(moved["model"], "models/deep/lamp2.cubik");
    assert!(dir.path().join("models/deep/lamp2.cubik").is_file());
    client.ok("model_outline", json!({"model": "models/deep/lamp2.cubik"}));
}

#[test]
fn a_save_refuses_other_extensions_stale_revisions_and_the_library() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    assert_eq!(
        client.code(
            "project_save",
            json!({"model": "lamp.bbmodel", "path": "lamp.json"})
        ),
        "E_PATH"
    );
    let revision = client.revision("lamp.bbmodel");
    client.edit(
        "lamp.bbmodel",
        json!([{"op": "set", "select": "base", "name": "foot"}]),
    );
    assert_eq!(
        client.code(
            "project_save",
            json!({"model": "lamp.bbmodel", "expected_revision": revision})
        ),
        "E_REVISION"
    );
    assert_eq!(
        client.code(
            "project_save",
            json!({"model": "lamp.bbmodel", "path": "cuboid-library/lamp.cubik"})
        ),
        "E_READ_ONLY"
    );
    assert!(!dir.path().join("cuboid-library").exists());
}

#[test]
fn a_style_reference_opens_read_only_and_cannot_be_edited_or_saved() {
    let dir = project();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(dir.path().join("refs/ref.bbmodel"), common::lamp()).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let imported = client.ok("refs_import", json!({"folder": "refs", "licence": "CC0"}));
    assert_eq!(imported["added"], 1);
    let stored = fs::read_dir(dir.path().join("cuboid-library/files"))
        .unwrap()
        .flatten()
        .next()
        .unwrap();
    let name = format!(
        "cuboid-library/files/{}",
        stored.file_name().to_string_lossy()
    );
    let opened = client.ok("project_open", json!({"model": name}));
    assert_eq!(opened["read_only"], true);
    client.ok("model_outline", json!({"model": name}));
    assert_eq!(
        client.edit_code(&name, json!([{"op": "set", "select": "base", "name": "x"}])),
        "E_READ_ONLY"
    );
    assert_eq!(
        client.code("project_save", json!({"model": name, "path": "copy.cubik"})),
        "E_READ_ONLY"
    );
    assert_eq!(
        client.code(
            "history",
            json!({"model": name, "action": "checkpoint", "name": "a"})
        ),
        "E_READ_ONLY"
    );
    // A dry run changes nothing, so it is fine.
    client.ok("model_edit", json!({"model": name, "dry_run": true, "ops": [{"op": "set", "select": "base", "name": "x"}]}));
    assert!(!dir.path().join("copy.cubik").exists());
}

#[test]
fn a_model_is_exported_as_a_java_model_and_as_a_pack_inside_the_project_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok(
        "project_open",
        json!({"model": "stone.cubik", "create": "block"}),
    );
    client.edit("stone.cubik", json!([{"op": "add_cube", "name": "full", "from": [0, 0, 0], "to": [16, 16, 16], "texture": "texture"}]));
    let model = client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "java_model", "version": "1.21.4"}),
    );
    assert_eq!(model["format"], "java_model");
    let json_path = dir.path().join("exports/stone/stone.json");
    assert!(json_path.is_file(), "{model}");
    let written: Value = serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
    assert_eq!(written["elements"].as_array().unwrap().len(), 1);
    assert_eq!(numbers(&written["elements"][0]["to"]), [16.0, 16.0, 16.0]);
    assert!(
        model["paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p.as_str().unwrap().starts_with("exports/stone/"))
    );
    let pack = client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "resource_pack", "folder": "build/pack", "namespace": "splat", "version": "1.21.4"}),
    );
    assert!(
        dir.path().join("build/pack/pack.mcmeta").is_file(),
        "{pack}"
    );
    assert!(pack["pack_format"].is_string());
    assert!(
        pack["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().contains("assets/splat/")),
        "{pack}"
    );
    // What the target does not take, and what is not a version or a place.
    client.edit(
        "stone.cubik",
        json!([{"op": "set", "select": "full", "to": [100, 16, 16]}]),
    );
    assert_eq!(
        client.code(
            "project_export",
            json!({"model": "stone.cubik", "format": "java_model"})
        ),
        "E_EXPORT"
    );
    assert_eq!(
        client.code(
            "project_export",
            json!({"model": "stone.cubik", "format": "java_model", "version": "banana"})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "project_export",
            json!({"model": "stone.cubik", "format": "java_model", "folder": "../out"})
        ),
        "E_PATH"
    );
    assert!(!dir.path().parent().unwrap().join("out").exists());
}

/// Every file under the folder with its bytes, sorted.
fn contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    listing(root)
        .into_iter()
        .map(|(name, _)| {
            let bytes = fs::read(root.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

/// A block model with one full cube whose faces use the default texture, open but not saved.
fn stone(client: &mut Client<'_>) {
    client.ok(
        "project_open",
        json!({"model": "stone.cubik", "create": "block"}),
    );
    client.edit("stone.cubik", json!([{"op": "add_cube", "name": "full", "from": [0, 0, 0], "to": [16, 16, 16], "texture": "texture"}]));
}

#[test]
fn an_export_replaces_no_file_unless_overwrite_is_true_and_then_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    stone(&mut client);
    // The files of an export are in the project folder itself, as a user keeps them, and the
    // model `stone.json` that is open is one of them.
    let first = client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "java_model", "folder": "out"}),
    );
    for path in first["paths"].as_array().unwrap() {
        let from = dir.path().join(path.as_str().unwrap());
        let to = dir
            .path()
            .join(path.as_str().unwrap().trim_start_matches("out/"));
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(from, to).unwrap();
    }
    client.ok("project_open", json!({"model": "stone.json"}));
    let before = contents(dir.path());
    for name in ["stone.json", "stone.cubik"] {
        let error = client
            .server
            .call(
                "project_export",
                json!({"model": name, "format": "java_model", "folder": "."}),
            )
            .expect_err("a file is there");
        assert_eq!(error.code, "E_FILE_CHANGED", "{name}");
        assert!(error.message.contains("stone.json"), "{}", error.message);
        assert!(error.hint.contains("overwrite"), "{}", error.hint);
        assert_eq!(contents(dir.path()), before, "{name} wrote something");
    }
    // A pack names its files too, and the refusal covers all of them.
    client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "resource_pack", "folder": "pack"}),
    );
    let pack = contents(dir.path());
    assert!(pack.len() > before.len());
    assert_eq!(
        client.code(
            "project_export",
            json!({"model": "stone.cubik", "format": "resource_pack", "folder": "pack"})
        ),
        "E_FILE_CHANGED"
    );
    assert_eq!(contents(dir.path()), pack);
    // With overwrite the same call replaces them, the model's own file included.
    let own = fs::read(dir.path().join("stone.json")).unwrap();
    fs::write(dir.path().join("stone.json"), b"{}").unwrap();
    let exported = client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "java_model", "folder": ".", "overwrite": true}),
    );
    assert!(
        exported["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "stone.json"),
        "{exported}"
    );
    assert_eq!(fs::read(dir.path().join("stone.json")).unwrap(), own);
}

#[test]
fn the_default_folder_of_an_export_works_the_first_time_and_needs_overwrite_the_second() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    stone(&mut client);
    let first = client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "java_model"}),
    );
    assert!(first["count"].as_u64().unwrap() >= 1, "{first}");
    let written = contents(dir.path());
    let error = client
        .server
        .call(
            "project_export",
            json!({"model": "stone.cubik", "format": "java_model"}),
        )
        .expect_err("the files are there");
    assert_eq!(error.code, "E_FILE_CHANGED");
    assert!(error.message.contains("exports/stone/stone.json"));
    assert_eq!(contents(dir.path()), written);
    client.ok(
        "project_export",
        json!({"model": "stone.cubik", "format": "java_model", "overwrite": true}),
    );
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect()
}

#[test]
fn validate_gives_the_issues_of_cuboid_cli_check_for_the_same_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    broken(&mut client);
    client.ok("project_save", json!({"model": "bad.cubik"}));
    let tool = client.ok("validate", json!({"model": "bad.cubik", "limit": 100}));
    let cli =
        cuboid_library::check::check(&dir.path().join("bad.cubik"), None, None).expect("checks");
    assert_eq!(tool["target"], cli.target.as_str());
    assert_eq!(tool["errors"].as_u64(), Some(cli.errors as u64));
    assert_eq!(tool["warnings"].as_u64(), Some(cli.warnings as u64));
    assert_eq!(tool["infos"].as_u64(), Some(cli.infos as u64));
    let issues = tool["issues"].as_array().unwrap();
    assert_eq!(issues.len(), cli.issues.len());
    assert!(issues.iter().any(|issue| issue["code"] == "GEO_BOUNDS"));
    for (mine, theirs) in issues.iter().zip(&cli.issues) {
        assert_eq!(mine["code"], theirs.code);
        assert_eq!(mine["severity"], theirs.severity);
        assert_eq!(mine["message"], theirs.message.as_str());
        assert_eq!(mine["hint"], theirs.hint.as_str());
        assert_eq!(mine["ids"].as_array().unwrap().len(), theirs.objects.len());
        let names: Vec<&str> = mine["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().rsplit('/').next().unwrap())
            .collect();
        let expected: Vec<&str> = theirs.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, expected);
        // Only a fix the commands of the core can run is offered.
        assert!(!mine["fix"].as_bool().unwrap() || theirs.fix);
    }
}

#[test]
fn validate_sees_unsaved_edits_pages_its_issues_and_runs_the_self_test() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    broken(&mut client);
    let all = client.ok("validate", json!({"model": "bad.cubik", "limit": 100}));
    let total = all["issues"].as_array().unwrap().len();
    assert!(total >= 1);
    let first = client.ok(
        "validate",
        json!({"model": "bad.cubik", "limit": 1, "self_test": true}),
    );
    assert_eq!(first["issues"].as_array().unwrap().len(), 1);
    assert_eq!(first["next"], "1");
    assert_eq!(first["self_test"]["ok"], true);
    assert!(first["self_test"]["fired"].as_u64().unwrap() > 10);
    assert!(first["self_test"]["silent"].as_array().unwrap().is_empty());
    let target = client.ok(
        "validate",
        json!({"model": "bad.cubik", "target": "pack", "limit": 100}),
    );
    assert!(
        target["target"]
            .as_str()
            .unwrap()
            .starts_with("a resource pack")
    );
}

#[test]
fn apply_fixes_is_one_undo_step_and_undo_brings_the_issue_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    broken(&mut client);
    let count = |client: &mut Client<'_>, code: &str| {
        let found = client.ok("validate", json!({"model": "bad.cubik", "limit": 100}));
        found["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|issue| issue["code"] == code)
            .count()
    };
    assert_eq!(count(&mut client, "GEO_BOUNDS"), 1);
    let issue = client.ok("validate", json!({"model": "bad.cubik"}))["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["code"] == "GEO_BOUNDS")
        .cloned()
        .unwrap();
    assert_eq!(issue["fix"], true);
    let steps = client.undo_labels("bad.cubik").len();
    let fixed = client.edit(
        "bad.cubik",
        json!([{"op": "apply_fixes", "codes": ["GEO_BOUNDS"]}]),
    );
    assert!(
        fixed["changed"]["updated"].as_u64().unwrap() >= 1,
        "{fixed}"
    );
    assert_eq!(count(&mut client, "GEO_BOUNDS"), 0);
    // The size and the move of the fix are one step, with whatever else the call did.
    let labels = client.undo_labels("bad.cubik");
    assert_eq!(
        (labels.len(), labels[0].as_str()),
        (steps + 1, "Agent edit (1 op)")
    );
    client.ok("history", json!({"model": "bad.cubik", "action": "undo"}));
    assert_eq!(count(&mut client, "GEO_BOUNDS"), 1);
    assert_eq!(client.undo_labels("bad.cubik").len(), steps);
    // A code that is not one of the validator's is refused; fixes of other codes leave it alone.
    assert_eq!(
        client.edit_code(
            "bad.cubik",
            json!([{"op": "apply_fixes", "codes": ["NOT_A_CODE"]}])
        ),
        "E_INPUT"
    );
    let none = client.edit(
        "bad.cubik",
        json!([{"op": "apply_fixes", "codes": ["TEX_UNUSED"]}]),
    );
    assert_eq!(none["warnings"][0]["code"], "W_UNCHANGED");
    assert_eq!(count(&mut client, "GEO_BOUNDS"), 1);
}

#[test]
fn the_fix_mapping_is_usable_by_the_application_in_one_step() {
    let mut core = cuboid_core::Core::new(cuboid_core::Compat::port(), 1);
    core.execute(cuboid_core::Command::AddElement(
        cuboid_core::ElementKind::Cube,
    ));
    core.execute(cuboid_core::Command::SetPosition {
        targets: vec![cuboid_core::Target::Element(0)],
        value: [Some(-40.0), None, None],
    });
    let profile = cuboid_validate::Profile::new(cuboid_validate::Target::JavaBlock {
        version: cuboid_fmt_minecraft::latest_version(),
    });
    let issues = cuboid_validate::validate(core.doc(), &profile);
    let bounds: Vec<&cuboid_validate::Issue> = issues
        .iter()
        .filter(|i| i.code == "GEO_BOUNDS" && cuboid_mcp::fixable(i))
        .collect();
    assert_eq!(bounds.len(), 1);
    let steps = core.history().undo_len();
    let summary = cuboid_mcp::apply_fixes(&mut core, &bounds);
    assert!(
        summary.applied >= 1 && summary.failed.is_empty(),
        "{summary:?}"
    );
    assert_eq!(core.history().undo_len(), steps + 1);
    let after = cuboid_validate::validate(core.doc(), &profile);
    assert!(after.iter().all(|i| i.code != "GEO_BOUNDS"));
}

#[test]
fn a_read_only_server_refuses_every_write_and_writes_nothing() {
    let dir = project();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(dir.path().join("refs/ref.bbmodel"), common::lamp()).unwrap();
    let before = listing(dir.path());
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), true, &mut fake);
    let writes = [
        ("refs_import", json!({"folder": "refs"})),
        (
            "project_open",
            json!({"model": "new.cubik", "create": "item"}),
        ),
        ("project_save", json!({"model": "lamp.bbmodel"})),
        (
            "project_export",
            json!({"model": "lamp.bbmodel", "format": "java_model"}),
        ),
        (
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [{"op": "set", "select": "base", "name": "x"}]}),
        ),
        (
            "texture_paint",
            json!({"model": "lamp.bbmodel", "texture": "0", "ops": [{"op": "fill_rect", "rect": [0, 0, 1, 1]}]}),
        ),
        (
            "history",
            json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": "a"}),
        ),
        (
            "history",
            json!({"model": "lamp.bbmodel", "action": "undo"}),
        ),
    ];
    for (tool, arguments) in writes {
        assert_eq!(client.code(tool, arguments), "E_READ_ONLY", "{tool}");
    }
    // Reading, trying and drawing still work; a picture is returned and no file is made.
    client.ok("brief_get", json!({}));
    client.ok("project_list", json!({}));
    client.ok("model_outline", json!({"model": "lamp.bbmodel"}));
    client.ok("validate", json!({"model": "lamp.bbmodel"}));
    client.ok("model_edit", json!({"model": "lamp.bbmodel", "dry_run": true, "ops": [{"op": "set", "select": "base", "name": "x"}]}));
    client.ok(
        "history",
        json!({"model": "lamp.bbmodel", "action": "list"}),
    );
    let (view, images) = client.pictures("render_view", json!({"model": "lamp.bbmodel"}));
    assert!(view.get("path").is_none() && images.len() == 1);
    client.pictures("render_sheet", json!({"model": "lamp.bbmodel"}));
    drop(client);
    assert_eq!(listing(dir.path()), before);
}

#[test]
fn each_write_adds_a_line_to_the_journal_and_reads_and_dry_runs_add_none() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    client.ok("model_outline", json!({"model": "lamp.bbmodel"}));
    client.ok("model_edit", json!({"model": "lamp.bbmodel", "dry_run": true, "ops": [{"op": "set", "select": "base", "name": "x"}]}));
    assert!(!dir.path().join("journal.jsonl").exists());
    client.ok(
        "model_edit",
        json!({"model": "lamp.bbmodel", "ops": [{"op": "set", "select": "base", "name": "x"}]}),
    );
    client.ok("project_save", json!({"model": "lamp.bbmodel"}));
    // A render writes a picture under renders/, so it is a write too.
    client.pictures("render_view", json!({"model": "lamp.cubik"}));
    client.pictures("render_sheet", json!({"model": "lamp.cubik"}));
    let text = fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    let tools: Vec<_> = lines.iter().map(|line| line["tool"].as_str()).collect();
    assert_eq!(
        tools,
        [
            Some("model_edit"),
            Some("project_save"),
            Some("render_view"),
            Some("render_sheet")
        ]
    );
    assert_eq!(lines[0]["model"], "lamp.bbmodel");
    assert_eq!(lines[1]["model"], "lamp.cubik");
    assert_eq!(lines[2]["model"], "lamp.cubik");
    assert!(lines[0]["revision"].as_str().unwrap().contains('-'));
    assert!(lines[0]["time"].as_u64().unwrap() > 1_700_000_000);
    assert_eq!(lines[0]["client"], "unknown");
}

#[test]
fn every_tool_answers_with_a_result_that_fits_its_output_schema() {
    let dir = project();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(dir.path().join("refs/ref.bbmodel"), common::lamp()).unwrap();
    fs::write(
        dir.path().join("refs/tex.png"),
        cuboid_mcp::Picture::new(8, 8, [1, 2, 3, 255])
            .png()
            .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("art")).unwrap();
    fs::write(
        dir.path().join("art/in.png"),
        cuboid_mcp::Picture::new(4, 4, [9, 9, 9, 255])
            .png()
            .unwrap(),
    )
    .unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let label = client.ok("project_open", json!({"model": "lamp.bbmodel"}))["textures"][0]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let imported = client.ok("refs_import", json!({"folder": "refs"}));
    assert_eq!(imported["added"], 2);
    let brief = client.ok("brief_get", json!({}));
    let entries = brief["entries"].as_array().unwrap();
    let model_ref = entries.iter().find(|e| e["kind"] == "model").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let texture_ref = entries.iter().find(|e| e["kind"] == "texture").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let calls: Vec<(&str, Value)> = vec![
        ("brief_get", json!({})),
        ("refs_import", json!({"folder": "refs"})),
        ("refs_get", json!({"id": model_ref})),
        ("refs_get", json!({"id": texture_ref})),
        ("project_list", json!({})),
        (
            "project_open",
            json!({"model": "new.cubik", "create": "item"}),
        ),
        ("project_open", json!({"model": "lamp.bbmodel"})),
        ("model_outline", json!({"model": "lamp.bbmodel"})),
        (
            "model_get",
            json!({"model": "lamp.bbmodel", "select": ["base", "lamp_arm"]}),
        ),
        ("model_find", json!({"model": "lamp.bbmodel", "name": "*"})),
        ("model_stats", json!({"model": "lamp.bbmodel"})),
        (
            "model_edit",
            json!({"model": "lamp.bbmodel", "ops": [
                {"op": "add_cube", "name": "n", "from": [0, 0, 0], "to": [1, 1, 1]},
                {"op": "set_face", "select": "n", "texture": label.as_str()},
                {"op": "add_texture", "name": "t2", "size": 8}
            ]}),
        ),
        (
            "texture_paint",
            json!({"model": "lamp.bbmodel", "texture": "t2", "ops": [{"op": "fill_rect", "rect": [0, 0, 2, 2], "color": "#ff0000"}]}),
        ),
        (
            "texture_view",
            json!({"model": "lamp.bbmodel", "texture": label}),
        ),
        (
            "validate",
            json!({"model": "lamp.bbmodel", "self_test": true}),
        ),
        ("render_view", json!({"model": "lamp.bbmodel"})),
        ("render_view", json!({"ref": model_ref, "camera": "gui"})),
        ("render_sheet", json!({"model": "lamp.bbmodel"})),
        (
            "history",
            json!({"model": "lamp.bbmodel", "action": "list"}),
        ),
        (
            "history",
            json!({"model": "lamp.bbmodel", "action": "checkpoint", "name": "a"}),
        ),
        ("project_save", json!({"model": "lamp.bbmodel"})),
        (
            "project_export",
            json!({"model": "lamp.cubik", "format": "java_model", "version": "1.21.4"}),
        ),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (tool, arguments) in calls {
        let output = client.server.call(tool, arguments.clone());
        let output = match output {
            Ok(output) => output,
            // The export may be refused for a model that does not fit; the error has its own shape.
            Err(error) if tool == "project_export" => {
                assert_eq!(error.code, "E_EXPORT");
                seen.insert(tool);
                continue;
            }
            Err(error) => panic!("{tool} {arguments}: {error:?}"),
        };
        let schema = TOOLS
            .iter()
            .find(|t| t.name == tool)
            .map(|t| (t.output)())
            .unwrap();
        conforms(&schema, &output.value)
            .unwrap_or_else(|why| panic!("{tool}: {why}\n{}", output.value));
        seen.insert(tool);
    }
    assert_eq!(seen.len(), TOOLS.len(), "{seen:?}");
}
