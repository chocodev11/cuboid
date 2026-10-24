use std::fs;

use tempfile::TempDir;

use super::*;
use crate::locations::{Location, LocationKind};

fn env() -> Environment {
    Environment {
        desktop_dir: "/home/me/Desktop".into(),
        minecraft_dir: String::new(),
    }
}

fn folder() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    (dir, path)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_saved_file_loads_back_equal() {
    let (_dir, path) = folder();
    let mut settings = Settings::first_run(&env());
    settings.backup_every_minutes = 25;
    settings.minecraft_skin = Some("Notch".into());
    settings.path_export = None;
    settings.recent_files.add("C:\\models\\a.cubik");
    settings.recent_files.add("C:\\models\\b.cubik");
    settings.cuboid.managers_on_left = true;
    settings.cuboid.scale_override = Some(1.5);
    settings.cuboid.locations.add_recent(Location {
        path: "C:\\models".into(),
        hidden: false,
        kind: LocationKind::Folder,
        time: 1_792_315_815,
    });
    settings
        .extra
        .insert("fromTheFuture".into(), serde_json::json!([1, 2]));
    settings.save(&path).unwrap();

    let loaded = Settings::load(&path, &env()).unwrap();
    assert!(matches!(loaded.origin, Origin::File));
    assert_eq!(loaded.settings, settings);
}

#[test]
fn the_file_is_pretty_json_with_the_version_first_and_a_final_newline() {
    let (_dir, path) = folder();
    Settings::first_run(&env()).save(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("{\n  \"version\": 1,\n  \"signatureTemplate\": "),
        "{text}"
    );
    assert!(text.contains("\n  \"minecraftSkin\": null,\n"));
    assert!(text.contains("\n  \"recentFiles\": {\n    \"max\": 10,\n    \"list\": []\n  },\n"));
    assert!(text.ends_with("\n}\n"));
    assert!(!text.starts_with('\u{feff}') && !text.contains('\r'));
}

#[test]
fn saving_creates_the_folder_and_replaces_the_file_without_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("config").join("cuboid");
    let path = folder.join(FILE_NAME);
    let mut settings = Settings::first_run(&env());
    settings.save(&path).unwrap();
    settings.show_hidden_files = true;
    settings.save(&path).unwrap();

    assert_eq!(names(&folder), [FILE_NAME]);
    let loaded = Settings::load(&path, &env()).unwrap();
    assert!(loaded.settings.show_hidden_files);
}

#[test]
fn a_failed_write_reports_the_path_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    // A folder cannot be replaced by a file.
    let path = dir.path().join(FILE_NAME);
    fs::create_dir(&path).unwrap();
    let err = Settings::first_run(&env()).save(&path).unwrap_err();
    assert!(
        matches!(&err, SettingsError::Write { path: at, .. } if *at == path),
        "{err}"
    );
    assert_eq!(names(dir.path()), [FILE_NAME]);
}

#[test]
fn a_missing_file_is_a_fresh_start() {
    let (_dir, path) = folder();
    let loaded = Settings::load(&path, &env()).unwrap();
    assert!(matches!(loaded.origin, Origin::Fresh));
    assert_eq!(loaded.settings, Settings::first_run(&env()));
}

#[test]
fn an_empty_or_null_file_is_a_fresh_start_and_stays_in_place() {
    let (dir, path) = folder();
    for content in ["", "  \n", "null"] {
        fs::write(&path, content).unwrap();
        let loaded = Settings::load(&path, &env()).unwrap();
        assert!(matches!(loaded.origin, Origin::Fresh), "{content:?}");
        assert_eq!(loaded.settings, Settings::first_run(&env()));
        assert_eq!(names(dir.path()), [FILE_NAME]);
    }
}

#[test]
fn a_fresh_start_logs_the_line_of_the_original() {
    let (_dir, path) = folder();
    crate::test_log::capture();
    Settings::load(&path, &env()).unwrap();
    assert_eq!(crate::test_log::lines(), ["CubikData: Fresh data created"]);

    fs::write(&path, "null").unwrap();
    Settings::load(&path, &env()).unwrap();
    assert_eq!(crate::test_log::lines(), ["CubikData: Fresh data created"]);

    fs::write(&path, "{}").unwrap();
    Settings::load(&path, &env()).unwrap();
    assert!(crate::test_log::lines().is_empty());
}

#[test]
fn debug_output_hides_the_token_and_the_raw_file() {
    let mut settings = Settings::first_run(&env());
    settings.sketchfab_token = Some("secret123token".into());
    let shown = format!("{settings:?}");
    assert!(!shown.contains("secret123token") && shown.contains("<redacted>"));

    let legacy = Legacy {
        data: Some(br#"{"sketchfabToken":"secret123token"}"#),
        ..Legacy::default()
    };
    let shown = format!("{legacy:?}");
    assert!(!shown.contains("secret123token"));
}

#[test]
fn a_file_that_is_not_json_is_kept_aside_and_the_first_run_settings_are_used() {
    let (dir, path) = folder();
    fs::write(&path, "{\"useNativeFileBrowser\": tru").unwrap();
    let loaded = Settings::load(&path, &env()).unwrap();
    let bad = dir.path().join("settings.json.bad");
    match loaded.origin {
        Origin::Rejected { error, kept_as } => {
            assert!(matches!(error, ParseError::Json(_)), "{error}");
            assert_eq!(kept_as.as_deref(), Some(bad.as_path()));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(loaded.settings, Settings::first_run(&env()));
    assert_eq!(names(dir.path()), ["settings.json.bad"]);
    assert_eq!(
        fs::read_to_string(&bad).unwrap(),
        "{\"useNativeFileBrowser\": tru"
    );

    // The next start finds no file; a second broken file replaces the older copy.
    assert!(matches!(
        Settings::load(&path, &env()).unwrap().origin,
        Origin::Fresh
    ));
    fs::write(&path, "[1]").unwrap();
    let loaded = Settings::load(&path, &env()).unwrap();
    assert!(matches!(
        loaded.origin,
        Origin::Rejected {
            error: ParseError::NotAnObject,
            ..
        }
    ));
    assert_eq!(fs::read_to_string(&bad).unwrap(), "[1]");
}

#[test]
fn an_unreadable_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = Settings::load(dir.path(), &env()).unwrap_err();
    assert!(matches!(err, SettingsError::Read { .. }), "{err}");
}

#[test]
fn a_file_with_one_unusable_key_keeps_the_others() {
    let (_dir, path) = folder();
    fs::write(
        &path,
        r#"{"backupEveryMinutes": null, "windowX": 100, "windowY": 50}"#,
    )
    .unwrap();
    let loaded = Settings::load(&path, &env()).unwrap();
    assert!(matches!(loaded.origin, Origin::File));
    assert_eq!(loaded.settings.backup_every_minutes, 10);
    assert_eq!(loaded.settings.saved_window_position(), Some((100, 50)));
}

#[test]
fn values_of_the_original_are_imported() {
    let data = br#"{"signatureTemplate":"By me","backupEveryMinutes":15,"windowX":40,"windowY":60,"minecraftSkin":null,"pathLoadSave":"D:\\Models","recentFiles":{"max":10,"list":[{"name":"a.cubik","path":"D:\\Models\\a.cubik"}]}}"#;
    let locations = "SavedLocations {\nSavedLocationEntry {\nRecent,\nD:\\Models,\nFalse,\nFolder,\n10/18/2026 09:30:15\n}\n}\n";
    let legacy = Legacy {
        data: Some(data),
        locations: Some(locations),
        favourites_marker: Some(1),
        managers_width: Some(250),
        helpers_width: Some(9000),
    };
    let settings = Settings::import_legacy(&legacy, &env());

    assert_eq!(settings.signature_template, "By me");
    assert_eq!(settings.backup_every_minutes, 15);
    assert_eq!(settings.saved_window_position(), Some((40, 60)));
    assert_eq!(settings.path_load_save.as_deref(), Some("D:\\Models"));
    assert_eq!(settings.recent_files.list.len(), 1);
    // Keys the file lacks take the field defaults, as for any existing file.
    assert!(settings.render_hidden_elements);
    assert_eq!(settings.cuboid.locations.recent.len(), 1);
    assert!(settings.cuboid.locations.defaults_added);
    assert_eq!(settings.cuboid.managers_width, 250);
    assert_eq!(settings.cuboid.helpers_width, HELPERS_WIDTH_MAX);
}

#[test]
fn nothing_to_import_is_the_first_run_and_an_unusable_file_is_ignored() {
    let first = Settings::first_run(&env());
    assert_eq!(Settings::import_legacy(&Legacy::default(), &env()), first);

    let broken = Legacy {
        data: Some(b"{ nope"),
        favourites_marker: Some(0),
        ..Legacy::default()
    };
    assert_eq!(Settings::import_legacy(&broken, &env()), first);
}
