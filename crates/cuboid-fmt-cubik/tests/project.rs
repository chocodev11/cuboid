//! The preset in the `cuboid.json` entry: it survives a save, a file without it reads as none,
//! and a compatible save leaves it out.

mod common;

use cuboid_fmt_cubik::{Limits, ProjectMeta, SaveOptions, load_bytes, save_bytes};

use common::*;

fn minecraft() -> ProjectMeta {
    ProjectMeta {
        preset: "minecraft".to_owned(),
        version: Some("1.21.11".to_owned()),
        allow_negative_size: Some(true),
    }
}

#[test]
fn the_preset_round_trips_through_a_saved_file() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let snapshot = snapshot(&doc, &sel);
    let options = SaveOptions {
        project: Some(minecraft()),
        ..SaveOptions::default()
    };
    let bytes = save_bytes(&snapshot, doc.retained.as_deref(), &[], &options).unwrap();
    let loaded = load_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(loaded.project, Some(minecraft()));
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
}

#[test]
fn a_file_without_the_entry_has_no_preset() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let snapshot = snapshot(&doc, &sel);
    let bytes = save_bytes(
        &snapshot,
        doc.retained.as_deref(),
        &[],
        &SaveOptions::default(),
    )
    .unwrap();
    assert_eq!(
        load_bytes(&bytes, &Limits::default()).unwrap().project,
        None
    );
}

#[test]
fn a_compatible_save_leaves_the_preset_out_and_the_payload_alone() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let snapshot = snapshot(&doc, &sel);
    let with = SaveOptions {
        project: Some(minecraft()),
        ..SaveOptions::default()
    };
    let compatible = SaveOptions {
        extension: false,
        ..with.clone()
    };
    let plain = save_bytes(&snapshot, doc.retained.as_deref(), &[], &compatible).unwrap();
    assert_eq!(
        load_bytes(&plain, &Limits::default()).unwrap().project,
        None
    );
    // The entry sits next to the payload: both files hold the same payload.
    let payload = |options: &SaveOptions| {
        let bytes = save_bytes(&snapshot, doc.retained.as_deref(), &[], options).unwrap();
        cuboid_fmt_cubik::container::read(std::io::Cursor::new(bytes), &Limits::default())
            .unwrap()
            .payload
    };
    assert_eq!(payload(&with), payload(&compatible));
}

/// A file whose `cuboid.json` entry is `entry`.
fn file_with_entry(entry: &[u8]) -> Vec<u8> {
    cuboid_fmt_cubik::container::write(&full_payload(), Some(entry)).unwrap()
}

fn entry_of(file: Vec<u8>) -> Option<Vec<u8>> {
    cuboid_fmt_cubik::container::read(std::io::Cursor::new(file), &Limits::default())
        .unwrap()
        .extension
}

#[test]
fn an_entry_of_a_newer_version_is_ignored_with_a_notice_and_kept_on_save() {
    let entry = br#"{"version":2,"overrides":"another shape","future":{"a":[1,2]}}"#;
    // The second run is above the retention cap: the entry is kept without the payload.
    for retain_cap in [Limits::default().retain_cap, 0] {
        let limits = Limits {
            retain_cap,
            ..Limits::default()
        };
        let loaded = load_bytes(&file_with_entry(entry), &limits).unwrap();
        assert_eq!(loaded.project, None);
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("newer version 2"));
        let (doc, sel) = open(loaded);
        let snapshot = snapshot(&doc, &sel);
        let options = SaveOptions {
            project: Some(minecraft()),
            ..SaveOptions::default()
        };
        let saved = save_bytes(&snapshot, doc.retained.as_deref(), &[], &options).unwrap();
        assert_eq!(entry_of(saved).as_deref(), Some(&entry[..]));

        // A compatible save leaves the entry out, as it does for every entry.
        let compatible = SaveOptions {
            extension: false,
            ..SaveOptions::default()
        };
        let saved = save_bytes(&snapshot, doc.retained.as_deref(), &[], &compatible).unwrap();
        assert_eq!(entry_of(saved), None);
    }
}

#[test]
fn an_unreadable_entry_is_kept_on_save_and_a_damaged_preset_is_reported() {
    let entry = b"{ this is not json";
    let loaded = load_bytes(&file_with_entry(entry), &Limits::default()).unwrap();
    assert!(loaded.warnings[0].contains("cannot be read"));
    let (doc, sel) = open(loaded);
    let saved = save_bytes(
        &snapshot(&doc, &sel),
        doc.retained.as_deref(),
        &[],
        &SaveOptions::default(),
    )
    .unwrap();
    assert_eq!(entry_of(saved).as_deref(), Some(&entry[..]));

    let damaged = br#"{"version":1,"project":{"preset":7}}"#;
    let loaded = load_bytes(&file_with_entry(damaged), &Limits::default()).unwrap();
    assert_eq!(loaded.project, None);
    assert!(loaded.warnings[0].contains("project settings"));
}
