use std::path::Path;

use super::*;
use crate::contract::FormatVisibility;

fn registry() -> Registry {
    Registry::new(&FormatVisibility {
        builtin_browser: true,
        ..FormatVisibility::default()
    })
}

/// The title and the text of a failed validation.
fn rejected(path: &str, mode: LookupMode, im: ImportMode) -> (String, String) {
    let message = validate(&registry(), Path::new(path), mode, im)
        .map(|_| ())
        .unwrap_err();
    assert_eq!(message.kind, MessageKind::Error);
    assert!(message.buttons.is_empty());
    (message.title, message.text)
}

// ---- interop 5.2 step 1 and 6.2 step 1 --------------------------------------------------

#[test]
fn a_path_without_extension_is_refused() {
    let expected = (
        "Missing extension".to_owned(),
        "The selected file doesn't have any extension.\n\nOperation aborted...".to_owned(),
    );
    assert_eq!(
        rejected("model", LookupMode::Open, ImportMode::Cubik29),
        expected
    );
    assert_eq!(
        rejected("model", LookupMode::Save, ImportMode::Cubik29),
        expected
    );
}

#[test]
fn an_unknown_type_names_the_extension_with_its_dot() {
    let expected = (
        "Unknown file type \".VOX\"".to_owned(),
        "The selected file can't be opened in {App}.\n\nOperation aborted...".to_owned(),
    );
    assert_eq!(
        rejected("m.VOX", LookupMode::Open, ImportMode::Cubik29),
        expected
    );
    // The export dialog words it the same way.
    assert_eq!(
        rejected("m.VOX", LookupMode::Save, ImportMode::Cubik29),
        expected
    );
}

#[test]
fn an_entry_that_cannot_read_or_write_says_so() {
    assert_eq!(
        rejected("m.js", LookupMode::Open, ImportMode::Cubik29),
        (
            "Can't read \".js\"".to_owned(),
            "{App} doesn't know how to read \".js\" files yet.\n\nHowever it knows how to write them!"
                .to_owned()
        )
    );
    assert_eq!(
        rejected("m.nbt", LookupMode::Save, ImportMode::Cubik29),
        (
            "Can't generate \".nbt\"".to_owned(),
            "{App} doesn't know how to write \".nbt\" files yet.\n\nHowever it knows how to read them!"
                .to_owned()
        )
    );
}

#[test]
fn a_usable_path_gives_its_entry() {
    let registry = registry();
    let label = |path, mode| {
        validate(&registry, Path::new(path), mode, ImportMode::Cubik29)
            .map(|entry| entry.label)
            .unwrap()
    };
    assert_eq!(label("m.png", LookupMode::Open), "Sprite");
    assert_eq!(label("m.png", LookupMode::Save), "Model Render");
    assert_eq!(label("m.cubik", LookupMode::Save), "Cubik model");
}

// ---- interop 5.2 step 3 -----------------------------------------------------------------

#[test]
fn sizes_are_divided_by_1024_up_to_gigabytes() {
    let cases = [
        (0, "0B"),
        (1023, "1023B"),
        (1024, "1KB"),
        (360_000, "351KB"),
        (1_048_575, "1023KB"),
        (5 * 1024 * 1024, "5MB"),
        (3 * 1024 * 1024 * 1024, "3GB"),
        // The unit stops at GB.
        (5000 * 1024 * 1024 * 1024, "5000GB"),
    ];
    for (bytes, text) in cases {
        assert_eq!(size_text(bytes), text, "{bytes}");
    }
}

#[test]
fn only_files_above_307200_bytes_show_the_loading_box() {
    assert_eq!(loading_box("m.cubik", 307_200), None);
    let message = loading_box("m.cubik", 307_201).unwrap();
    assert_eq!(message.kind, MessageKind::Wait);
    assert_eq!(message.title, "Loading...");
    assert_eq!(
        message.text,
        "{App} is loading \"m.cubik\" (300KB)\n\nPlease wait..."
    );
    assert!(message.buttons.is_empty());
}

// ---- interop 5.2 step 5, 6.2 steps 2 and 7 ----------------------------------------------

#[test]
fn status_lines_name_the_upper_cased_extension_and_the_path() {
    let path = Path::new("C:\\models\\m.vox");
    let cases = [
        (Phase::Imported, "Imported VOX: C:\\models\\m.vox"),
        (Phase::Loaded, "Loaded VOX: C:\\models\\m.vox"),
        (Phase::Importing, "Importing VOX: C:\\models\\m.vox"),
        (Phase::Loading, "Loading VOX: C:\\models\\m.vox"),
        (Phase::Exported, "Exported VOX: C:\\models\\m.vox"),
    ];
    for (phase, text) in cases {
        assert_eq!(status(phase, path), text);
    }
    assert_eq!(
        status(Phase::Loaded, Path::new("m.cubik")),
        "Loaded CUBIK: m.cubik"
    );
}

// ---- interop 5.4, 6.2 step 6 ------------------------------------------------------------

#[test]
fn a_failed_import_uses_the_original_title_and_lead_sentence() {
    let message = import_failed("MCPE entity", "no bones");
    assert_eq!(message.kind, MessageKind::Error);
    assert_eq!(message.title, "MCPE entity Import");
    assert_eq!(
        message.text,
        "An error occured while importing MCPE entity:\nno bones"
    );
    assert_eq!(message.buttons, ["Abort"]);
}

#[test]
fn an_unexpected_export_failure_uses_the_original_title_and_lead_sentence() {
    let message = export_failed("Minecraft PE Entity Model", "disk full");
    assert_eq!(message.kind, MessageKind::Error);
    assert_eq!(message.title, "Error while saving");
    assert_eq!(
        message.text,
        "An error occured while saving Minecraft PE Entity Model export:\ndisk full"
    );
    assert_eq!(message.buttons, ["Abort"]);
}

// ---- interop 6.3 ------------------------------------------------------------------------

#[test]
fn the_voxel_only_prompt_offers_continue_and_cancel() {
    let message = voxel_only_prompt("Magicavoxel VOX");
    assert_eq!(message.kind, MessageKind::Information);
    assert_eq!(message.title, "Magicavoxel VOX Export");
    assert_eq!(
        message.text,
        "Exporting to Magicavoxel VOX format only export voxels. Elements and Meshes are not exported.\n\nTo export elements or meshes, convert them first to voxels."
    );
    assert_eq!(message.buttons, ["Continue", "Cancel"]);
    assert_eq!(cancelled_status("vox"), "Cancelled VOX export...");
}

// ---- interop 6.4 ------------------------------------------------------------------------

#[test]
fn writer_helpers_report_files_and_textures() {
    let path = Path::new("out\\m.png");
    let cases = [
        (OutKind::Binary, true, "Successfuly saved file: out\\m.png"),
        (OutKind::Text, true, "Successfuly saved file: out\\m.png"),
        (OutKind::Png, true, "Successfuly saved texture: out\\m.png"),
        (OutKind::Binary, false, "Failed saving file: out\\m.png"),
        (OutKind::Text, false, "Failed saving file: out\\m.png"),
        (OutKind::Png, false, "Failed saving texture: out\\m.png"),
    ];
    for (kind, ok, text) in cases {
        assert_eq!(write_status(kind, ok, path), text, "{kind:?} {ok}");
    }
}

#[test]
fn a_failed_write_shows_the_helper_message_of_its_kind() {
    let cases = [
        (
            OutKind::Binary,
            "An error occured while saving file \"m.stl\".\n\naccess denied",
        ),
        (
            OutKind::Text,
            "An error occured while saving \"m.stl\".\n\naccess denied",
        ),
        (
            OutKind::Png,
            "An error occured while saving texture \"m.stl\".\n\naccess denied",
        ),
    ];
    for (kind, text) in cases {
        let message = write_failed(kind, "m.stl", "access denied");
        assert_eq!(message.kind, MessageKind::Error);
        assert_eq!(message.title, "Error while saving file");
        assert_eq!(message.text, text, "{kind:?}");
        assert!(message.buttons.is_empty());
    }
}

// ---- interop 6.5 ------------------------------------------------------------------------

#[test]
fn the_element_only_precheck_names_what_the_model_holds() {
    let name = "Optifine Model Part";
    let cases = [
        (
            NoElements::Voxels,
            "Voxel model",
            "Your model does not contain any element but voxels.\n\nTo export your model to Optifine Model Part format, you must convert voxels to elements first.",
        ),
        (
            NoElements::Meshes,
            "Mesh model",
            "Your model does not contain any element but meshes.\n\nTo export your model to Optifine Model Part format, you must convert meshes to elements first (via a voxelization).",
        ),
        (
            NoElements::Empty,
            "Empty model",
            "Your model is empty.\n\nTo export your model to Optifine Model Part format, you must create at least one element first.",
        ),
    ];
    for (what, title, text) in cases {
        let message = no_elements(name, what, false);
        assert_eq!(message.kind, MessageKind::Error);
        assert_eq!(
            (message.title.as_str(), message.text.as_str()),
            (title, text)
        );
    }
}

#[test]
fn the_minecraft_json_precheck_words_the_mesh_case_without_the_article() {
    let message = no_elements("Minecraft 3D Model", NoElements::Meshes, true);
    assert_eq!(
        message.text,
        "Your model does not contain any element but meshes.\n\nTo export your model to Minecraft 3D Model format, you must convert meshes to elements first (via voxelization)."
    );
    // The other two cases read the same in both wordings.
    assert_eq!(
        no_elements("X", NoElements::Empty, true),
        no_elements("X", NoElements::Empty, false)
    );
}
