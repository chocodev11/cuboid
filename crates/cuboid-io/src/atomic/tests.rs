use std::fs;

use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_new_file_gets_the_bytes_and_leaves_nothing_else_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.cubik");
    write_atomic(&path, b"first").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"first");
    assert_eq!(names(dir.path()), ["m.cubik"]);
}

#[test]
fn an_existing_file_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.cubik");
    fs::write(&path, b"old and longer").unwrap();
    write_atomic(&path, b"new").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(names(dir.path()), ["m.cubik"]);
}

#[test]
fn an_empty_text_is_an_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.json");
    write_atomic(&path, b"").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"");
}

#[test]
fn a_missing_folder_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing").join("m.cubik");
    assert!(write_atomic(&path, b"x").is_err());
    assert!(names(dir.path()).is_empty());
}

#[test]
fn a_failed_replacement_leaves_no_temporary_file() {
    // A folder cannot be replaced by a file, so the final step fails.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target");
    fs::create_dir(&path).unwrap();
    assert!(write_atomic(&path, b"x").is_err());
    assert_eq!(names(dir.path()), ["target"]);
    assert!(path.is_dir());
}
