//! Every path of a tool goes through one resolver that keeps it inside the project folder.

mod common;

use std::fs;
use std::path::Path;

use common::{Client, Fake, project};
use cuboid_mcp::Root;
use serde_json::json;

fn root() -> (tempfile::TempDir, Root) {
    let dir = project();
    let root = Root::new(dir.path()).expect("a project folder");
    (dir, root)
}

/// Makes `name` in `dir` a link to `target`: a junction on Windows, which needs no privilege, a
/// symbolic link elsewhere. False when the system would not.
fn link(dir: &Path, name: &str, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.join(name))
            .arg(target)
            .output()
            .is_ok_and(|out| out.status.success())
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dir.join(name)).is_ok()
    }
}

#[test]
fn a_path_with_dots_a_drive_a_root_or_a_special_form_is_refused() {
    let (_dir, root) = root();
    let long = "a".repeat(1025);
    let bad = [
        "..",
        "../x.cubik",
        "a/../../x.cubik",
        "a\\..\\..\\x.cubik",
        "a/..",
        "C:\\Windows\\x.cubik",
        "C:/x.cubik",
        "C:x.cubik",
        "/etc/passwd",
        "\\x.cubik",
        "\\\\host\\share\\x.cubik",
        "//host/share/x.cubik",
        "\\\\.\\COM1",
        "\\\\?\\C:\\x.cubik",
        "//?/C:/x.cubik",
        "\\\\?\\UNC\\host\\share\\x",
        "NUL",
        "nul.cubik",
        "dir/CON",
        "dir/COM1.png",
        "LPT9",
        "AUX.txt",
        "lamp.bbmodel:stream",
        "dir/name.",
        "dir/name ",
        "...",
        "",
        "x\0y",
        long.as_str(),
    ];
    for text in bad {
        let error = root.resolve(text).expect_err(text);
        assert_eq!(error.code, "E_PATH", "{text:?}");
    }
}

#[test]
fn ordinary_relative_paths_resolve_under_the_folder_even_when_they_do_not_exist_yet() {
    let (dir, root) = root();
    for (text, shown) in [
        ("lamp.bbmodel", "lamp.bbmodel"),
        ("./lamp.bbmodel", "lamp.bbmodel"),
        ("a//b\\c.cubik", "a/b/c.cubik"),
        ("new/deeper/model.cubik", "new/deeper/model.cubik"),
    ] {
        let path = root.resolve(text).expect(text);
        assert!(path.starts_with(root.path()), "{path:?}");
        assert_eq!(root.relative(&path), shown);
    }
    assert!(!dir.path().join("new").exists(), "resolving makes nothing");
    assert_eq!(root.resolve(".").expect("the folder"), root.path());
}

#[cfg(windows)]
#[test]
fn a_name_in_another_case_is_the_file_that_is_there() {
    let (_dir, root) = root();
    let path = root.resolve("LAMP.BBMODEL").expect("resolves");
    assert_eq!(root.relative(&path), "lamp.bbmodel");
}

#[test]
fn the_project_folder_is_absolute_exists_and_is_no_drive_root() {
    let dir = project();
    assert!(Root::new(Path::new("relative/folder")).is_err());
    assert!(Root::new(&dir.path().join("missing")).is_err());
    assert!(Root::new(&dir.path().join("lamp.bbmodel")).is_err());
    let top = dir.path().ancestors().last().unwrap().to_owned();
    assert!(Root::new(&top).expect_err("a root").contains("root"));
    assert!(Root::new(dir.path()).is_ok());
    let unc = Root::new(Path::new("\\\\host\\share\\folder"));
    assert!(unc.is_err());
}

#[test]
fn a_link_out_of_the_folder_is_refused_for_what_exists_and_for_what_is_new() {
    let (dir, root) = root();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.bbmodel"), common::lamp()).unwrap();
    if !link(dir.path(), "gate", outside.path()) {
        eprintln!("the system makes no link here; skipped");
        return;
    }
    for text in [
        "gate",
        "gate/secret.bbmodel",
        "gate/new.cubik",
        "gate/deep/new.cubik",
    ] {
        assert_eq!(root.resolve(text).expect_err(text).code, "E_PATH", "{text}");
    }
    // A link inside the folder is followed.
    fs::create_dir(dir.path().join("real")).unwrap();
    fs::write(dir.path().join("real/in.bbmodel"), common::lamp()).unwrap();
    if link(dir.path(), "alias", &dir.path().join("real")) {
        let path = root.resolve("alias/in.bbmodel").expect("inside");
        assert_eq!(root.relative(&path), "real/in.bbmodel");
    }
}

#[test]
fn the_tools_refuse_a_path_that_leaves_the_folder_and_write_nothing_outside() {
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.bbmodel"), common::lamp()).unwrap();
    fs::write(
        outside.path().join("secret.png"),
        cuboid_mcp::Picture::new(2, 2, [1, 2, 3, 255])
            .png()
            .unwrap(),
    )
    .unwrap();
    let gate = link(dir.path(), "gate", outside.path());
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let mut paths = vec![
        "../secret.bbmodel".to_owned(),
        "C:\\secret.bbmodel".to_owned(),
        "\\\\?\\C:\\secret.bbmodel".to_owned(),
    ];
    if gate {
        paths.extend([
            "gate/secret.bbmodel".to_owned(),
            "gate/new.cubik".to_owned(),
        ]);
    }
    for path in &paths {
        let tools = [
            ("project_open", json!({"model": path})),
            ("model_outline", json!({"model": path})),
            (
                "project_save",
                json!({"model": "lamp.bbmodel", "path": path}),
            ),
            (
                "project_export",
                json!({"model": "lamp.bbmodel", "format": "java_model", "folder": path}),
            ),
            ("refs_import", json!({"folder": path})),
            (
                "texture_paint",
                json!({"model": "lamp.bbmodel", "ops": [{"op": "import_png", "texture": "x", "path": path}]}),
            ),
            ("project_open", json!({"model": path, "create": "item"})),
        ];
        for (tool, arguments) in tools {
            assert_eq!(client.code(tool, arguments), "E_PATH", "{tool} {path}");
        }
    }
    drop(client);
    let names: Vec<_> = fs::read_dir(outside.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(!dir.path().parent().unwrap().join("secret.bbmodel").exists());
}

#[test]
fn the_library_index_cannot_send_a_tool_outside_the_folder() {
    let dir = project();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(dir.path().join("refs/r.bbmodel"), common::lamp()).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let imported = client.ok("refs_import", json!({"folder": "refs"}));
    let id = imported["ids"][0].as_str().unwrap().to_owned();
    client.ok("refs_get", json!({"id": id}));
    // An index that names a stored copy by a path is not followed.
    let index = dir.path().join("cuboid-library/library.json");
    let text = fs::read_to_string(&index).unwrap();
    let stored = fs::read_dir(dir.path().join("cuboid-library/files"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .file_name();
    fs::write(
        &index,
        text.replace(&*stored.to_string_lossy(), "../../lamp.bbmodel"),
    )
    .unwrap();
    assert_eq!(client.code("refs_get", json!({"id": id})), "E_IO");
    assert_eq!(client.code("render_view", json!({"ref": id})), "E_IO");
}

#[test]
fn a_stored_copy_behind_a_link_out_of_the_folder_is_not_read() {
    let dir = project();
    fs::create_dir_all(dir.path().join("refs")).unwrap();
    fs::write(dir.path().join("refs/r.bbmodel"), common::lamp()).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let imported = client.ok("refs_import", json!({"folder": "refs"}));
    let id = imported["ids"][0].as_str().unwrap().to_owned();
    client.ok("refs_get", json!({"id": id}));
    client.ok("render_view", json!({"ref": id}));
    // The stored copies move to a folder outside, and the folder of copies becomes a link to it.
    let outside = tempfile::tempdir().unwrap();
    let files = dir.path().join("cuboid-library/files");
    for entry in fs::read_dir(&files).unwrap().flatten() {
        fs::copy(entry.path(), outside.path().join(entry.file_name())).unwrap();
    }
    fs::remove_dir_all(&files).unwrap();
    if !link(&dir.path().join("cuboid-library"), "files", outside.path()) {
        eprintln!("the system makes no link here; skipped");
        return;
    }
    assert_eq!(client.code("refs_get", json!({"id": id})), "E_PATH");
    assert_eq!(client.code("render_view", json!({"ref": id})), "E_PATH");
    assert_eq!(client.code("render_sheet", json!({"ref": id})), "E_PATH");
}
