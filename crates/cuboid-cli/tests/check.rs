//! `cuboid-cli check` on models made in the test.

mod common;

use std::fs;
use std::path::Path;

use common::{add_cube, add_texture, cli, code, cubik, names, path_arg, project, stderr, stdout};
use cuboid_fmt_cubik::cub;
use cuboid_library::{Brief, NameCase, Naming, PROJECT_DIR};
use serde_json::Value;

/// The issues of a run as JSON.
fn check_json(args: &[&str]) -> (i32, Value, String) {
    let mut full = vec!["check", "--json"];
    full.extend_from_slice(args);
    let output = cli(&full);
    let text = stdout(&output);
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (code(&output), json, stderr(&output))
}

fn codes(json: &Value) -> Vec<&str> {
    json["issues"]
        .as_array()
        .expect("issues")
        .iter()
        .filter_map(|issue| issue["code"].as_str())
        .collect()
}

/// A cube named `cube` with an orange 16 x 16 texture.
fn clean_model(dir: &Path, name: &str) -> std::path::PathBuf {
    cubik(dir, name, |doc| {
        add_texture(doc, "stone", "block/stone", 16, [200, 100, 50]);
        add_cube(doc, "cube", "stone", 0.0);
    })
}

#[test]
fn a_clean_model_exits_0_and_a_broken_one_exits_4() {
    let dir = tempfile::tempdir().unwrap();
    let clean = clean_model(dir.path(), "clean.cubik");
    let (status, json, error) = check_json(&["--target", "block", path_arg(&clean)]);
    assert_eq!(status, 0, "{json}\n{error}");
    assert_eq!(json["format"], "cuboid-check");
    assert_eq!(json["errors"], 0);
    assert!(
        json["target"]
            .as_str()
            .is_some_and(|t| t.starts_with("Java block "))
    );

    let broken = cubik(dir.path(), "broken.cubik", |doc| {
        add_texture(doc, "stone", "block/stone", 16, [200, 100, 50]);
        add_cube(doc, "wall", "stone", 100.0);
    });
    let (status, json, _) = check_json(&["--target", "block", path_arg(&broken)]);
    assert_eq!(status, 4, "{json}");
    let issues = json["issues"].as_array().unwrap();
    let bounds = issues
        .iter()
        .find(|i| i["code"] == "GEO_BOUNDS")
        .expect("the box is outside the range");
    assert_eq!(bounds["severity"], "error");
    assert_eq!(bounds["fix"], true);
    assert_eq!(bounds["objects"][0]["name"], "wall");
    assert_eq!(bounds["objects"][0]["number"], 1);
    assert!(json["errors"].as_u64().unwrap() >= 1);
    // The keys of an issue follow the documented order.
    let text = stdout(&cli(&[
        "check",
        "--json",
        "--target",
        "block",
        path_arg(&broken),
    ]));
    let issue = &text[text.find("\"issues\"").unwrap()..];
    let positions: Vec<usize> = ["code", "severity", "objects", "message", "hint", "fix"]
        .iter()
        .map(|key| issue.find(&format!("\"{key}\"")).unwrap())
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}"
    );
}

#[test]
fn the_text_output_has_one_line_per_issue_and_a_summary() {
    let dir = tempfile::tempdir().unwrap();
    let broken = cubik(dir.path(), "broken.cubik", |doc| {
        add_texture(doc, "stone", "block/stone", 16, [200, 100, 50]);
        add_cube(doc, "wall", "stone", 100.0);
    });
    let output = cli(&["check", "--target", "block", path_arg(&broken)]);
    assert_eq!(code(&output), 4, "{}", stderr(&output));
    let text = stdout(&output);
    let line = text
        .lines()
        .find(|line| line.starts_with("error GEO_BOUNDS [wall] "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(line.ends_with("(fix available)"), "{line}");
    assert!(
        text.lines().last().is_some_and(|l| l.contains("error")),
        "{text}"
    );
}

#[test]
fn the_default_target_follows_the_file_as_the_app_does() {
    let dir = tempfile::tempdir().unwrap();
    // A plain .cubik is checked as a Cubik file, which has nothing against a wide cube.
    let wide = cubik(dir.path(), "wide.cubik", |doc| {
        add_texture(doc, "stone", "block/stone", 16, [200, 100, 50]);
        add_cube(doc, "wall", "stone", 100.0);
    });
    let (status, json, _) = check_json(&[path_arg(&wide)]);
    assert_eq!(status, 0, "{json}");
    assert_eq!(json["target"], "a Cubik file");
    // A Minecraft model JSON is a Java block.
    let model = dir.path().join("wall.json");
    fs::write(
        &model,
        r##"{"textures": {"t": "block/stone"},
            "elements": [{"from": [0, 0, 0], "to": [16, 16, 100],
                "faces": {"north": {"uv": [0, 0, 16, 16], "texture": "#t"}}}]}"##,
    )
    .unwrap();
    let (status, json, error) = check_json(&[path_arg(&model)]);
    assert_eq!(status, 4, "{json}\n{error}");
    assert!(json["target"].as_str().unwrap().starts_with("Java block "));
    assert!(codes(&json).contains(&"GEO_BOUNDS"), "{json}");
    // And an item with --target item.
    let (_, json, _) = check_json(&["--target", "item", path_arg(&model)]);
    assert!(json["target"].as_str().unwrap().starts_with("Java item "));
    let (_, json, _) = check_json(&["--target", "pack", path_arg(&model)]);
    assert!(
        json["target"]
            .as_str()
            .unwrap()
            .starts_with("a resource pack for ")
    );
}

#[test]
fn bbmodel_and_cub_files_are_opened_with_the_importers_of_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let bb = dir.path().join("wall.bbmodel");
    fs::write(
        &bb,
        r#"{"meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
            "name": "wall", "resolution": {"width": 16, "height": 16},
            "elements": [{"name": "wall", "from": [0, 0, 0], "to": [16, 16, 100],
                "faces": {"north": {"uv": [0, 0, 16, 16], "texture": null}}, "uuid": "e0"}],
            "textures": []}"#,
    )
    .unwrap();
    let (status, json, error) = check_json(&[path_arg(&bb)]);
    assert_eq!(status, 4, "{json}\n{error}");
    assert!(codes(&json).contains(&"GEO_BOUNDS"), "{json}");

    let old = dir.path().join("box.cub");
    let text = br#"{"textures_path":{},"elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#;
    fs::write(&old, cub::deobfuscate(text).into_bytes()).unwrap();
    let (status, json, error) = check_json(&[path_arg(&old)]);
    assert_eq!(status, 0, "{json}\n{error}");
    assert_eq!(json["target"], "a Cubik file");
}

#[test]
fn the_project_rules_come_from_the_brief_next_to_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let brief = Brief {
        namespace: "splat".to_owned(),
        texture_size: Some(16),
        palette: vec![[0, 0, 255]],
        palette_strict: true,
        max_elements: Some(1),
        naming: Naming {
            case: NameCase::Snake,
            prefix: "ink_".to_owned(),
        },
        ..Brief::default()
    };
    let folder = project(dir.path(), brief);
    // A 32 x 32 orange texture in another namespace, two elements, a name that is not snake case.
    let model = cubik(&folder, "Ink Gun.cubik", |doc| {
        add_texture(doc, "stone", "block/stone", 32, [200, 100, 50]);
        add_cube(doc, "Big Cube", "stone", 0.0);
        add_cube(doc, "second", "stone", 3.0);
    });
    let (status, json, error) = check_json(&["--target", "block", path_arg(&model)]);
    assert_eq!(status, 4, "{json}\n{error}");
    let found = codes(&json);
    for code in [
        "PRJ_PALETTE",
        "PRJ_TEXTURE_SIZE",
        "PRJ_NAMESPACE",
        "PRJ_NAME",
        "BUDGET_ELEMENTS",
    ] {
        assert!(found.contains(&code), "{code} in {found:?}");
    }
    let palette = json["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "PRJ_PALETTE")
        .unwrap();
    assert_eq!(palette["severity"], "error", "strict palette");
    let rename = json["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "PRJ_NAME" && i["objects"][0]["name"] == "Big Cube")
        .expect("the element name");
    assert_eq!(rename["fix"], true);
    let file = json["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| {
            i["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("the model file name"))
        })
        .expect("the file name");
    assert!(file["message"].as_str().unwrap().contains("Ink Gun"));
    assert!(json["project"].as_str().unwrap().ends_with(PROJECT_DIR));

    // The same model in another folder has no project, unless --brief names one.
    let elsewhere = tempfile::tempdir().unwrap();
    let copy = elsewhere.path().join("Ink Gun.cubik");
    fs::copy(&model, &copy).unwrap();
    let (status, json, _) = check_json(&["--target", "block", path_arg(&copy)]);
    assert_eq!(status, 0, "{json}");
    assert_eq!(json["project"], Value::Null);
    assert!(codes(&json).iter().all(|c| !c.starts_with("PRJ_")));
    let (status, json, _) = check_json(&[
        "--brief",
        path_arg(&folder),
        "--target",
        "block",
        path_arg(&copy),
    ]);
    assert_eq!(status, 4, "{json}");
    assert!(codes(&json).contains(&"PRJ_PALETTE"));
}

#[test]
fn check_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let folder = project(dir.path(), Brief::default());
    let model = clean_model(&folder, "m.cubik");
    let library = folder.join(PROJECT_DIR);
    let (before_folder, before_library) = (names(&folder), names(&library));
    let bytes = fs::read(&model).unwrap();
    for args in [
        vec!["check", path_arg(&model)],
        vec!["check", "--json", "--target", "item", path_arg(&model)],
    ] {
        let output = cli(&args);
        assert!(matches!(code(&output), 0 | 4), "{}", stderr(&output));
    }
    assert_eq!(
        (names(&folder), names(&library)),
        (before_folder, before_library)
    );
    assert_eq!(fs::read(&model).unwrap(), bytes);
}

#[test]
fn mistakes_are_usage_errors_and_missing_files_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let model = clean_model(dir.path(), "m.cubik");
    let path = path_arg(&model);
    for args in [
        vec!["check"],
        vec!["check", "--target", "wall", path],
        vec!["check", "--target"],
        vec!["check", path, path],
        vec!["check", "--nope", path],
    ] {
        assert_eq!(code(&cli(&args)), 1, "{args:?}");
    }
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "x").unwrap();
    assert_eq!(
        code(&cli(&["check", path_arg(&notes)])),
        1,
        "unknown extension"
    );
    let missing = dir.path().join("none.cubik");
    assert_eq!(code(&cli(&["check", path_arg(&missing)])), 2);
    assert_eq!(
        code(&cli(&["check", "--brief", path_arg(dir.path()), path])),
        2
    );
}
