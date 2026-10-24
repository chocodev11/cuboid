use std::fs;

use super::*;

const LINK_INFO_HEADER: usize = 0x1C;

/// The LinkInfo structure around a region that starts at the local base path. The region is
/// followed by the two NULs that end the base path and the empty path suffix; the target text
/// is everything before them.
fn link_info(region: &[u8]) -> Vec<u8> {
    let size = u32::try_from(LINK_INFO_HEADER + region.len() + 2).unwrap();
    let mut info = Vec::new();
    info.extend(size.to_le_bytes());
    info.extend(u32::try_from(LINK_INFO_HEADER).unwrap().to_le_bytes());
    info.extend(1u32.to_le_bytes()); // flags: a local base path
    info.extend(0u32.to_le_bytes()); // volume ID offset
    info.extend(u32::try_from(LINK_INFO_HEADER).unwrap().to_le_bytes()); // local base path
    info.resize(LINK_INFO_HEADER, 0);
    info.extend(region);
    info.extend([0, 0]);
    info
}

/// A shortcut with a link target ID list of `id_list` bytes, as shell-created ones have.
fn shortcut(id_list: usize, region: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; ID_LIST_SIZE_AT];
    bytes[0] = 0x4C;
    bytes[FLAGS_AT..FLAGS_AT + 4].copy_from_slice(&HAS_ID_LIST.to_le_bytes());
    bytes.extend(u16::try_from(id_list).unwrap().to_le_bytes());
    bytes.extend(vec![0xAB; id_list]);
    bytes.extend(link_info(region));
    bytes
}

fn resolve(bytes: &[u8]) -> Option<String> {
    target(Path::new("link.lnk"), bytes)
}

// ---- interop 28 step 1 ------------------------------------------------------------------

#[test]
fn the_target_is_the_local_base_path() {
    let region = br"C:\Users\me\Desktop\model.cubik";
    for id_list in [0, 1, 40] {
        assert_eq!(
            resolve(&shortcut(id_list, region)).as_deref(),
            Some(r"C:\Users\me\Desktop\model.cubik"),
            "ID list of {id_list} bytes"
        );
    }
}

#[test]
fn the_shortcut_must_have_the_lnk_extension_in_any_case() {
    let bytes = shortcut(4, br"C:\a");
    let target_of = |name: &str| target(Path::new(name), &bytes);
    assert_eq!(target_of("x.LNK").as_deref(), Some(r"C:\a"));
    assert_eq!(target_of("x.Lnk").as_deref(), Some(r"C:\a"));
    assert_eq!(target_of("x.txt"), None);
    assert_eq!(target_of("lnk"), None);
}

#[test]
fn characters_invalid_in_paths_and_hash_signs_are_removed() {
    let region = b"C:\\a#b\"c<d>e|f\x01g.txt";
    assert_eq!(
        resolve(&shortcut(0, region)).as_deref(),
        Some(r"C:\abcdefg.txt")
    );
}

#[test]
fn a_text_with_a_double_nul_keeps_the_head_and_the_tail_after_the_unc_part() {
    // Head, double NUL, a part that is dropped up to `\\`, and what follows the next NUL.
    let region = b"C:\\a\0\0junk\\\\server\\share\0b.txt";
    assert_eq!(resolve(&shortcut(0, region)).as_deref(), Some(r"C:\ab.txt"));
    // Nothing to append without the `\\`.
    assert_eq!(
        resolve(&shortcut(0, b"C:\\a\0\0rest")).as_deref(),
        Some(r"C:\a")
    );
}

#[test]
fn a_shortcut_without_a_path_gives_an_empty_target() {
    assert_eq!(resolve(&shortcut(0, b"")).as_deref(), Some(""));
}

#[test]
fn the_text_is_read_as_utf8_characters() {
    assert_eq!(
        resolve(&shortcut(0, "C:\\dir\\é.txt".as_bytes())).as_deref(),
        Some("C:\\dir\\é.txt")
    );
}

#[test]
fn without_an_id_list_the_link_info_follows_the_flags() {
    // The position stays behind the flags that were just read (0x18).
    let mut bytes = vec![0; FLAGS_AT + 4];
    bytes.extend(link_info(br"C:\b"));
    assert_eq!(resolve(&bytes).as_deref(), Some(r"C:\b"));
}

#[test]
fn damaged_shortcuts_give_no_path() {
    let good = shortcut(4, br"C:\dir\file.txt");
    // Every cut before the end of the base path loses a field or the text.
    let needed = ID_LIST_SIZE_AT + 2 + 4 + LINK_INFO_HEADER;
    for len in 0..needed {
        assert_eq!(resolve(&good[..len]), None, "cut at {len}");
    }
    // A base path offset behind the end of the structure.
    let mut bad = good.clone();
    let at = ID_LIST_SIZE_AT + 2 + 4 + 16;
    bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(resolve(&bad), None);
    // An ID list that runs past the file.
    let mut bad = good;
    bad[ID_LIST_SIZE_AT..ID_LIST_SIZE_AT + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(resolve(&bad), None);
}

#[test]
fn a_cut_text_gives_what_is_there() {
    let mut bytes = shortcut(0, br"C:\dir\file.txt");
    bytes.truncate(bytes.len() - 8);
    // The two NULs and "le.txt" are gone, but the size field still promises them.
    assert_eq!(resolve(&bytes).as_deref(), Some(r"C:\dir\fi"));
}

// ---- interop 28 step 2 and T-LNK-1 ------------------------------------------------------

#[test]
fn a_shortcut_to_a_local_file_opens_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("model.cubik");
    fs::write(&file, b"x").unwrap();
    let bytes = shortcut(8, file.to_str().unwrap().as_bytes());
    let found = resolve(&bytes);
    assert_eq!(
        follow(found.as_deref()),
        Follow::Browse(dir.path().to_owned())
    );
}

#[test]
fn a_shortcut_to_a_folder_opens_that_folder() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = shortcut(8, dir.path().to_str().unwrap().as_bytes());
    let found = resolve(&bytes);
    assert_eq!(
        follow(found.as_deref()),
        Follow::Browse(dir.path().to_owned())
    );
}

#[test]
fn a_shortcut_to_a_missing_target_shows_the_path_not_found_box() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.cubik");
    let text = missing.to_str().unwrap();
    let found = resolve(&shortcut(8, text.as_bytes()));
    let Follow::NotFound(message) = follow(found.as_deref()) else {
        panic!("a missing file must not be followed");
    };
    assert_eq!(message.kind, MessageKind::Error);
    assert_eq!(message.title, "Path not found");
    assert_eq!(
        message.text,
        format!("Sorry but shortcut can't be followed:\n\"{text}\"")
    );
    assert_eq!(message.buttons, ["Continue", "Cancel"]);
}

#[test]
fn an_unreadable_shortcut_quotes_an_empty_path() {
    let Follow::NotFound(message) = follow(None) else {
        panic!("no target must not be followed");
    };
    assert_eq!(message.text, "Sorry but shortcut can't be followed:\n\"\"");
    assert_eq!(follow(Some("")), follow(None));
}

// ---- robustness -------------------------------------------------------------------------

#[test]
fn mutated_shortcuts_never_panic() {
    let seed = shortcut(12, br"C:\Users\me\Desktop\model.cubik");
    assert!(resolve(&seed).is_some());
    for bytes in crate::testgen::mutants(&seed, 5000) {
        let _ = resolve(&bytes);
    }
}
