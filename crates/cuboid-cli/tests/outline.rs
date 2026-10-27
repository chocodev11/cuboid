//! `cuboid-cli outline` on models made in the test.

mod common;

use std::fs;
use std::path::Path;

use common::{add_cube, add_texture, cli, code, cubik, names, path_arg, project, stderr, stdout};
use cuboid_library::{Brief, OUTLINE_PAGE, PROJECT_DIR, outline};
use serde_json::Value;

/// A model of `count` cubes, each with the 16 x 16 texture `stone`.
fn cubes(dir: &Path, name: &str, count: usize) -> std::path::PathBuf {
    cubik(dir, name, |doc| {
        add_texture(doc, "stone", "block/stone", 16, [200, 100, 50]);
        for n in 0..count {
            add_cube(doc, &format!("cube_{n}"), "stone", n as f32 * 2.0);
        }
    })
}

#[test]
fn json_is_the_outline_of_the_library_function() {
    let dir = tempfile::tempdir().unwrap();
    let model = cubes(dir.path(), "two.cubik", 2);
    let output = cli(&["outline", "--json", path_arg(&model)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    let opened = cuboid_library::doc::load(&model).expect("loads");
    let expected = serde_json::to_string(&outline(&opened.doc).page(0, OUTLINE_PAGE)).unwrap();
    assert_eq!(text.trim_end(), expected);

    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["format"], "cuboid-outline");
    assert_eq!(
        (json["total"].as_u64(), json["offset"].as_u64()),
        (Some(2), Some(0))
    );
    assert_eq!(json["next"], Value::Null);
    let first = &json["elements"][0];
    assert_eq!(
        (first["number"].as_u64(), first["path"].as_str()),
        (Some(1), Some("cube_0"))
    );
    assert_eq!(
        first["faces"]["north"],
        serde_json::json!(["stone", 0, 0, 16, 16])
    );
    assert_eq!(json["textures"][0]["name"], "stone");
    assert_eq!(json["textures"][0]["size"], serde_json::json!([16, 16]));
    assert_eq!(json["textures"][0]["path"], "block/stone");
}

#[test]
fn the_limit_cuts_the_elements_and_zero_lists_all() {
    let dir = tempfile::tempdir().unwrap();
    let model = cubes(dir.path(), "three.cubik", 3);
    let count = |args: &[&str]| -> (usize, Option<u64>) {
        let mut full = vec!["outline", "--json"];
        full.extend_from_slice(args);
        full.push(path_arg(&model));
        let output = cli(&full);
        assert_eq!(code(&output), 0, "{}", stderr(&output));
        let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
        (
            json["elements"].as_array().unwrap().len(),
            json["next"].as_u64(),
        )
    };
    assert_eq!(count(&[]), (3, None));
    assert_eq!(count(&["--limit", "2"]), (2, Some(2)));
    assert_eq!(count(&["--limit=1"]), (1, Some(1)));
    assert_eq!(count(&["--limit", "0"]), (3, None));

    let output = cli(&["outline", "--limit", "2", path_arg(&model)]);
    let text = stdout(&output);
    assert!(text.contains("#1 cube_0 from "), "{text}");
    assert!(
        text.contains("\n    down stone 0 0 16 16; up stone "),
        "{text}"
    );
    assert!(text.contains("texture stone 16x16 block/stone\n"), "{text}");
    assert!(
        text.ends_with("2 of 3 elements (more from 2; --limit 0 lists all)\n"),
        "{text}"
    );
    assert!(!text.contains("cube_2"), "{text}");
}

#[test]
fn one_page_is_24_elements() {
    let dir = tempfile::tempdir().unwrap();
    let model = cubes(dir.path(), "thirty.cubik", 30);
    let output = cli(&["outline", "--json", path_arg(&model)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        json["elements"].as_array().map(Vec::len),
        Some(OUTLINE_PAGE)
    );
    assert_eq!(
        (json["total"].as_u64(), json["next"].as_u64()),
        (Some(30), Some(24))
    );
    let text = stdout(&cli(&["outline", path_arg(&model)]));
    assert!(
        text.ends_with("24 of 30 elements (more from 24; --limit 0 lists all)\n"),
        "{text}"
    );
}

#[test]
fn an_element_without_faces_prints_no_empty_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bare.json");
    fs::write(
        &path,
        r#"{"elements": [{"from": [0, 0, 0], "to": [1, 1, 1]}]}"#,
    )
    .unwrap();
    let output = cli(&["outline", path_arg(&path)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("#1 "), "{text}");
    assert!(text.lines().all(|line| !line.trim().is_empty()), "{text:?}");
}

#[test]
fn a_stored_copy_of_the_library_is_read_like_any_model() {
    let dir = tempfile::tempdir().unwrap();
    let folder = project(dir.path(), Brief::default());
    let files = folder.join(PROJECT_DIR).join("files");
    let copy = names(&files)
        .into_iter()
        .find(|name| name.ends_with("cube.json"))
        .expect("the stored model");
    let output = cli(&["outline", "--json", path_arg(&files.join(copy))]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["elements"][0]["to"], serde_json::json!([16, 16, 16]));
}

#[test]
fn outline_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let model = cubes(dir.path(), "m.cubik", 1);
    let (before, bytes) = (names(dir.path()), fs::read(&model).unwrap());
    for args in [
        vec!["outline", path_arg(&model)],
        vec!["outline", "--json", path_arg(&model)],
    ] {
        assert_eq!(code(&cli(&args)), 0);
    }
    assert_eq!(names(dir.path()), before);
    assert_eq!(fs::read(&model).unwrap(), bytes);
}

#[test]
fn mistakes_are_usage_errors_and_missing_files_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let model = cubes(dir.path(), "m.cubik", 1);
    let path = path_arg(&model);
    for args in [
        vec!["outline"],
        vec!["outline", "--limit", "many", path],
        vec!["outline", "--limit"],
        vec!["outline", path, path],
        vec!["outline", "--nope", path],
    ] {
        assert_eq!(code(&cli(&args)), 1, "{args:?}");
    }
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "x").unwrap();
    let output = cli(&["outline", path_arg(&notes)]);
    assert_eq!(code(&output), 1, "unknown extension");
    assert!(
        stderr(&output).contains("can be read"),
        "{}",
        stderr(&output)
    );
    let missing = dir.path().join("none.cubik");
    assert_eq!(code(&cli(&["outline", path_arg(&missing)])), 2);
}
