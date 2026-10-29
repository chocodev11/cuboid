use std::fs;
use std::time::SystemTime;

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

fn aged(path: &Path, age: Duration) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

#[test]
fn the_temporary_file_carries_the_prefix_and_lives_next_to_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let temp = temp_in(dir.path()).unwrap();
    assert_eq!(temp.path().parent(), Some(dir.path()));
    let name = temp.path().file_name().unwrap().to_string_lossy();
    assert!(name.starts_with(TEMP_PREFIX) && name.len() > TEMP_PREFIX.len());
}

#[test]
fn a_write_removes_old_leftovers_of_cut_off_writes_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        ".cuboid-tmp-old",
        ".cuboid-tmp-fresh",
        "user.cubik",
        ".tmpOther",
    ] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    for name in [".cuboid-tmp-old", "user.cubik", ".tmpOther"] {
        aged(&dir.path().join(name), Duration::from_secs(3 * 60 * 60));
    }
    write_atomic(&dir.path().join("m.cubik"), b"data").unwrap();
    assert_eq!(
        names(dir.path()),
        [".cuboid-tmp-fresh", ".tmpOther", "m.cubik", "user.cubik"]
    );
}

#[test]
fn only_the_first_write_into_a_folder_looks_for_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    write_atomic(&dir.path().join("a.cubik"), b"1").unwrap();
    let late = dir.path().join(".cuboid-tmp-late");
    fs::write(&late, b"x").unwrap();
    aged(&late, Duration::from_secs(3 * 60 * 60));
    write_atomic(&dir.path().join("b.cubik"), b"2").unwrap();
    assert!(late.exists(), "the folder was cleaned at the first write");
}

#[test]
fn the_age_is_measured_from_the_time_of_the_own_file_and_the_own_file_stays() {
    let dir = tempfile::tempdir().unwrap();
    let temp = temp_in(dir.path()).unwrap();
    // A file server whose clock is two hours behind stamps the new file two hours in the past.
    temp.as_file()
        .set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
        .unwrap();
    let running = dir.path().join(".cuboid-tmp-running");
    let killed = dir.path().join(".cuboid-tmp-killed");
    fs::write(&running, b"x").unwrap();
    fs::write(&killed, b"x").unwrap();
    aged(&running, Duration::from_secs(2 * 60 * 60));
    aged(&killed, Duration::from_secs(4 * 60 * 60));

    let reference = temp.as_file().metadata().unwrap().modified().unwrap();
    remove_stale(dir.path(), temp.path().file_name().unwrap(), reference);
    assert!(temp.path().exists(), "the own file is never removed");
    assert!(
        running.exists(),
        "within the hour of the file system's clock"
    );
    assert!(!killed.exists());
}
