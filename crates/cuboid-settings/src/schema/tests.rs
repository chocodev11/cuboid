use serde_json::json;

use super::*;
use crate::store::ParseError;

/// A file the original writes before the first Preferences OK, with this port's default
/// signature and the folders of `env()`.
const FRESH: &str = r#"{"signatureTemplate":"Designed by %author with %cubik","useNativeFileBrowser":false,"showHiddenFiles":false,"readdDefaultFavourites":false,"backupEveryMinutes":10,"windowX":0,"windowY":0,"openElementCreate":true,"openElementTools":false,"openElementsList":false,"openElementsMaterial":false,"openVoxelTools":false,"openGizmosTools":false,"openMeshCreate":false,"openMeshTools":false,"openMeshesList":false,"openPaintTools":false,"openColorTools":false,"openPalette":false,"renderHiddenElements":false,"renderHiddenElementsMainView":false,"maximize3dViewOnPreview":true,"showClassicTooltips":true,"minecraftSkin":null,"appSkin":"Light","pathLoadSave":"/home/me/Desktop","pathImport":"/home/me/Desktop","pathExport":"/home/me/Desktop","pathTextures":"/home/me/Desktop","pathMeshes":"/home/me/Desktop","pathMinecraft":"/home/me/.minecraft","pathTextureEditor":"","minecraftExportTextures":false,"objCenterOnImport":true,"sketchfabToken":"","formatHideMinecraft":false,"formatHideStonehearth":false,"formatHideStaxel":false,"formatHideVoxel":false,"formatHideMesh":false,"advAtlasPadding":false,"advAtlasSquare":true,"advKeysRotate":"Alpha1, Keypad1","advKeysScale":"Alpha2, Keypad2","advKeysClone":"Alpha3, Keypad3, LeftAlt, RightAlt","advKeysPan":"Mouse2, Space","advKeysFocus":"F","mainCamOrtho":false,"useElementsColor":true,"emulateElementTintColor":true,"centerPoint":false,"renderShadows":false,"renderAntialiasing":false,"recentFiles":{"max":10,"list":[]}}"#;

fn env() -> Environment {
    Environment {
        desktop_dir: "/home/me/Desktop".into(),
        minecraft_dir: "/home/me/.minecraft".into(),
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn fresh() -> Map<String, Value> {
    object(serde_json::from_str(FRESH).unwrap())
}

fn parse(value: &Value) -> Settings {
    let bytes = serde_json::to_vec(value).unwrap();
    Settings::parse(&bytes, &env()).unwrap().unwrap()
}

fn written(settings: &Settings) -> Map<String, Value> {
    object(serde_json::to_value(settings).unwrap())
}

/// `FRESH` with every key changed away from its first-run value.
fn changed() -> Map<String, Value> {
    let mut map = fresh();
    for (key, value) in &mut map {
        match value {
            Value::Bool(flag) => *flag = !*flag,
            Value::Number(number) => *value = json!(number.as_i64().unwrap() + 7),
            Value::String(_) | Value::Null => *value = json!(format!("changed {key}")),
            _ => {}
        }
    }
    map.insert("appSkin".into(), json!("Dark"));
    map
}

/// The keys with a boolean or integer value in `FRESH`.
fn flag_and_int_keys() -> Vec<String> {
    fresh()
        .into_iter()
        .filter(|(_, value)| value.is_boolean() || value.is_number())
        .map(|(key, _)| key)
        .collect()
}

/// The string keys that may be `null`.
fn nullable_keys() -> Vec<String> {
    fresh()
        .into_iter()
        .filter(|(key, value)| {
            (value.is_string() || value.is_null()) && key != "signatureTemplate" && key != "appSkin"
        })
        .map(|(key, _)| key)
        .collect()
}

#[test]
fn the_first_run_file_has_the_keys_values_and_order_of_the_original() {
    let first = written(&Settings::first_run(&env()));
    let expected = fresh();
    let keys: Vec<_> = first.keys().map(String::as_str).collect();
    let original: Vec<_> = expected.keys().map(String::as_str).collect();
    assert_eq!(original.len(), 54);
    assert_eq!(keys[..original.len()], original);
    assert_eq!(keys[original.len()..], ["cuboid"]);
    for (key, value) in &expected {
        assert_eq!(first[key], *value, "{key}");
    }
}

#[test]
fn a_fresh_file_of_the_original_reads_as_the_first_run_settings() {
    let settings = parse(&Value::Object(fresh()));
    assert_eq!(settings, Settings::first_run(&env()));
}

#[test]
fn every_key_round_trips() {
    let changed = changed();
    let settings = parse(&Value::Object(changed.clone()));
    let again = written(&settings);
    for (key, value) in &changed {
        assert_eq!(again[key], *value, "{key}");
    }
    assert_ne!(settings, Settings::first_run(&env()));
}

#[test]
fn keys_a_file_lacks_take_the_field_defaults() {
    let first = Settings::first_run(&env());
    let expected = Settings {
        render_hidden_elements: true,
        obj_center_on_import: false,
        adv_atlas_square: false,
        adv_keys_rotate: None,
        adv_keys_scale: None,
        adv_keys_clone: None,
        adv_keys_pan: None,
        adv_keys_focus: None,
        ..first
    };
    assert_eq!(parse(&json!({})), expected);
}

#[test]
fn keys_a_file_has_win_over_the_field_defaults() {
    let settings = parse(&json!({
        "renderHiddenElements": false,
        "objCenterOnImport": true,
        "advAtlasSquare": true,
        "advKeysFocus": "G",
    }));
    assert!(!settings.render_hidden_elements);
    assert!(settings.obj_center_on_import);
    assert!(settings.adv_atlas_square);
    assert_eq!(settings.adv_keys_focus.as_deref(), Some("G"));
    assert_eq!(settings.adv_keys_pan, None);
}

#[test]
fn there_are_37_flag_and_integer_keys() {
    assert_eq!(flag_and_int_keys().len(), 37);
}

#[test]
fn null_in_a_flag_or_integer_key_costs_only_that_key() {
    let first = written(&Settings::first_run(&env()));
    let changed = changed();
    for key in flag_and_int_keys() {
        let mut file = changed.clone();
        file.insert(key.clone(), Value::Null);
        let again = written(&parse(&Value::Object(file)));
        for (other, value) in &changed {
            let want = if *other == key { &first[other] } else { value };
            assert_eq!(&again[other], want, "null in {key}, key {other}");
        }
    }
}

#[test]
fn null_in_a_string_key_is_accepted() {
    let keys = nullable_keys();
    assert_eq!(keys.len(), 14);
    for key in keys {
        let mut file = changed();
        file.insert(key.clone(), Value::Null);
        let again = written(&parse(&Value::Object(file)));
        assert_eq!(again[&key], Value::Null, "{key}");
    }
}

#[test]
fn a_null_signature_template_or_theme_takes_the_default() {
    let settings = parse(&json!({"signatureTemplate": null, "appSkin": null}));
    assert_eq!(settings.signature_template, DEFAULT_SIGNATURE);
    assert_eq!(settings.app_skin, "Light");
}

#[test]
fn only_the_known_theme_names_are_kept() {
    for (stored, kept) in [
        ("Light", "Light"),
        ("Dark", "Dark"),
        ("Rainbow", "Rainbow"),
        ("dark", "Light"),
        ("Solar", "Light"),
        ("", "Light"),
    ] {
        let settings = parse(&json!({"appSkin": stored}));
        assert_eq!(settings.app_skin, kept, "{stored:?}");
    }
}

#[test]
fn a_wrong_type_takes_the_first_run_value_of_its_key() {
    let settings = parse(&json!({
        "showHiddenFiles": 3,
        "pathImport": 5,
        "advKeysPan": ["Mouse2"],
        "signatureTemplate": false,
        "mainCamOrtho": true,
    }));
    assert!(!settings.show_hidden_files);
    assert_eq!(settings.path_import.as_deref(), Some("/home/me/Desktop"));
    assert_eq!(settings.adv_keys_pan.as_deref(), Some("Mouse2, Space"));
    assert_eq!(settings.signature_template, DEFAULT_SIGNATURE);
    assert!(settings.main_cam_ortho);
}

#[test]
fn numbers_written_as_text_are_accepted() {
    let settings = parse(&json!({
        "backupEveryMinutes": "15",
        "windowX": " -20 ",
        "windowY": 30,
        "centerPoint": "True",
        "renderShadows": "false",
    }));
    assert_eq!(settings.backup_every_minutes, 15);
    assert_eq!(settings.window_x, -20);
    assert_eq!(settings.window_y, 30);
    assert!(settings.center_point);
    assert!(!settings.render_shadows);
}

#[test]
fn integers_that_are_words_fractions_or_too_large_fall_back() {
    for stored in [
        json!("abc"),
        json!(""),
        json!(10.5),
        json!(5_000_000_000_i64),
        json!([1]),
    ] {
        let settings = parse(&json!({"backupEveryMinutes": stored}));
        assert_eq!(settings.backup_every_minutes, 10);
    }
}

#[test]
fn loading_does_not_clamp_the_backup_delay() {
    for minutes in [0, 1, 4, 999_999] {
        let settings = parse(&json!({"backupEveryMinutes": minutes}));
        assert_eq!(settings.backup_every_minutes, minutes);
    }
}

#[test]
fn unknown_keys_are_kept_in_file_order_after_the_known_ones() {
    let file = json!({"zeta": 1, "useNativeFileBrowser": true, "alpha": {"x": 2}});
    let again = written(&parse(&file));
    let keys: Vec<_> = again.keys().rev().take(3).map(String::as_str).collect();
    assert_eq!(keys, ["alpha", "zeta", "cuboid"]);
    assert_eq!(again["alpha"], json!({"x": 2}));
}

#[test]
fn the_version_key_is_not_kept() {
    assert!(!written(&parse(&json!({"version": 7}))).contains_key("version"));
}

#[test]
fn recent_files_and_the_cuboid_section_are_read_with_the_other_keys() {
    let settings = parse(&json!({
        "recentFiles": {"max": 3, "list": [{"name": "a.cubik", "path": "/a.cubik"}]},
        "cuboid": {"managersWidth": 250},
    }));
    assert_eq!(settings.recent_files.max, 3);
    assert_eq!(settings.recent_files.list.len(), 1);
    assert_eq!(settings.cuboid.managers_width, 250);

    let unusable = parse(&json!({"recentFiles": null, "cuboid": 5}));
    assert_eq!(unusable.recent_files, RecentFiles::default());
    assert_eq!(unusable.cuboid, Cuboid::default());
}

#[test]
fn text_is_empty_null_an_object_or_rejected() {
    let read = |bytes: &[u8]| Settings::parse(bytes, &env());
    for empty in [
        &b""[..],
        b" \r\n\t",
        b"null",
        b"\xEF\xBB\xBFnull",
        b"\xEF\xBB\xBF",
    ] {
        assert!(matches!(read(empty), Ok(None)), "{empty:?}");
    }
    assert!(matches!(read(b"\xEF\xBB\xBF{}"), Ok(Some(_))));
    for not_an_object in [&b"[]"[..], b"42", b"\"text\"", b"true"] {
        assert!(matches!(read(not_an_object), Err(ParseError::NotAnObject)));
    }
    for broken in [&b"{"[..], b"{\"a\":}", b"nul", b"{} {}", b"\xFF\xFE"] {
        assert!(
            matches!(read(broken), Err(ParseError::Json(_))),
            "{broken:?}"
        );
    }
}

#[test]
fn the_saved_window_position_is_used_only_when_it_looks_valid() {
    let at = |x, y| Settings {
        window_x: x,
        window_y: y,
        ..Settings::first_run(&env())
    };
    assert_eq!(at(120, 80).saved_window_position(), Some((120, 80)));
    assert_eq!(at(-1500, 40).saved_window_position(), Some((-1500, 40)));
    assert_eq!(
        at(29_999, -29_999).saved_window_position(),
        Some((29_999, -29_999))
    );
    for (x, y) in [
        (0, 0),
        (0, 80),
        (120, 0),
        (30_000, 80),
        (120, -30_000),
        (i32::MIN, 1),
    ] {
        assert_eq!(at(x, y).saved_window_position(), None, "({x}, {y})");
    }
}
