//! The render tools on a fake renderer, the contact sheet, and the connection snippets and
//! self-check.

mod common;

use std::path::Path;

use common::{Client, Fake, project};
use cuboid_mcp::{Camera, Picture, RenderError, self_check, snippets};
use serde_json::json;

#[test]
fn a_view_comes_back_as_a_picture_a_file_and_numbers_about_the_picture() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let (view, images) = client.pictures(
        "render_view",
        json!({"model": "lamp.bbmodel", "camera": "front", "size": 64}),
    );
    assert_eq!(view["size"], json!([64, 64]));
    assert_eq!(view["bounds"], json!([16, 16, 48, 48]));
    assert_eq!(view["coverage"], 0.25);
    assert_eq!(view["emulated"], false);
    let path = view["path"].as_str().unwrap();
    assert!(
        path.starts_with("renders/lamp-front-r") && path.ends_with(".png"),
        "{path}"
    );
    let on_disk = Picture::from_png(&std::fs::read(dir.path().join(path)).unwrap()).unwrap();
    assert_eq!(Picture::from_png(&images[0]).unwrap(), on_disk);
    assert_eq!(on_disk.get(0, 0), [127, 127, 127, 255]);
    assert_eq!(on_disk.get(32, 32), [40, 100, 200, 255]);
    let (_, clear) = client.pictures(
        "render_view",
        json!({"model": "lamp.bbmodel", "size": 32, "background": "transparent"}),
    );
    assert_eq!(Picture::from_png(&clear[0]).unwrap().get(0, 0)[3], 0);
    let (_, red) = client.pictures(
        "render_view",
        json!({"model": "lamp.bbmodel", "size": 32, "background": "#ff0000"}),
    );
    assert_eq!(
        Picture::from_png(&red[0]).unwrap().get(0, 0),
        [255, 0, 0, 255]
    );
    let (gui, _) = client.pictures(
        "render_view",
        json!({"model": "lamp.bbmodel", "camera": "gui", "size": 32}),
    );
    assert_eq!(gui["emulated"], true);
    drop(client);
    assert_eq!(fake.calls[0], (Camera::Front, 64));
    assert_eq!(fake.calls[1], (Camera::Iso, 32));
    assert_eq!(fake.calls[3], (Camera::Gui, 32));
}

#[test]
fn a_large_view_is_saved_whole_and_sent_at_most_1024_px_wide() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let (view, images) = client.pictures(
        "render_view",
        json!({"model": "lamp.bbmodel", "size": 2048}),
    );
    let sent = Picture::from_png(&images[0]).unwrap();
    assert_eq!((sent.width, sent.height), (1024, 1024));
    let saved =
        Picture::from_png(&std::fs::read(dir.path().join(view["path"].as_str().unwrap())).unwrap())
            .unwrap();
    assert_eq!((saved.width, saved.height), (2048, 2048));
    assert_eq!(
        client.code(
            "render_view",
            json!({"model": "lamp.bbmodel", "size": 2049})
        ),
        "E_LIMIT"
    );
    let small = client.ok("render_view", json!({"model": "lamp.bbmodel", "size": 1}));
    assert_eq!(small["size"], json!([16, 16]));
}

#[test]
fn a_view_needs_a_model_or_a_reference_and_a_camera_that_exists() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    assert_eq!(client.code("render_view", json!({})), "E_INPUT");
    assert_eq!(
        client.code(
            "render_view",
            json!({"model": "lamp.bbmodel", "ref": "abc"})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "render_view",
            json!({"model": "lamp.bbmodel", "camera": "sideways"})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code(
            "render_view",
            json!({"model": "lamp.bbmodel", "background": "red"})
        ),
        "E_INPUT"
    );
    assert_eq!(
        client.code("render_view", json!({"ref": "0123456789abcdef"})),
        "E_NO_MATCH"
    );
    assert_eq!(
        client.code("render_view", json!({"model": "missing.cubik"})),
        "E_NO_MATCH"
    );
}

#[test]
fn the_schemas_say_which_face_each_camera_shows() {
    let list = cuboid_mcp::tools::definitions();
    for (tool, property) in [("render_view", "camera"), ("render_sheet", "views")] {
        let tool_schema = list.iter().find(|entry| entry["name"] == tool).unwrap();
        let text = tool_schema["inputSchema"]["properties"][property]["description"]
            .as_str()
            .unwrap();
        for words in [
            "front looks at the north face",
            "back at the south face",
            "left at the west face",
            "right at the east face",
            "iso from above the north-east corner",
        ] {
            assert!(text.contains(words), "{tool}: {text}");
        }
    }
}

#[test]
fn a_reference_is_drawn_like_a_model() {
    let dir = project();
    std::fs::create_dir_all(dir.path().join("refs")).unwrap();
    std::fs::write(dir.path().join("refs/r.bbmodel"), common::lamp()).unwrap();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let id = client.ok("refs_import", json!({"folder": "refs"}))["ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let (view, images) = client.pictures(
        "render_view",
        json!({"ref": id, "camera": "gui", "size": 32}),
    );
    assert_eq!(view["ref"], id.as_str());
    assert!(view.get("model").is_none());
    assert_eq!(view["path"], format!("renders/ref-{id}-gui-ref.png"));
    assert_eq!(Picture::from_png(&images[0]).unwrap().width, 32);
    let (sheet, _) = client.pictures(
        "render_sheet",
        json!({"ref": id, "views": ["front", "iso"], "size": 32}),
    );
    assert_eq!(sheet["ref"], id.as_str());
}

#[test]
fn a_sheet_lays_the_views_out_in_a_grid_in_the_order_asked_for() {
    let dir = project();
    let mut fake = Fake::default();
    let mut client = Client::new(dir.path(), false, &mut fake);
    let (sheet, images) = client.pictures("render_sheet", json!({"model": "lamp.bbmodel"}));
    assert_eq!(sheet["views"], json!(["front", "right", "top", "iso"]));
    assert_eq!(
        (
            sheet["columns"].as_u64(),
            sheet["rows"].as_u64(),
            sheet["cell"].as_u64()
        ),
        (Some(2), Some(2), Some(256))
    );
    assert_eq!(sheet["size"], json!([512, 512]));
    let picture = Picture::from_png(&images[0]).unwrap();
    // The square of each view sits in the middle of its cell, in the colour of its camera.
    let centre = |column: u32, row: u32| picture.get(column * 256 + 128, row * 256 + 128);
    let colour = |camera: Camera| {
        let at = Camera::ALL.iter().position(|c| *c == camera).unwrap() as u8;
        [40 + 25 * at, 100, 200, 255]
    };
    assert_eq!(centre(0, 0), colour(Camera::Front));
    assert_eq!(centre(1, 0), colour(Camera::Right));
    assert_eq!(centre(0, 1), colour(Camera::Top));
    assert_eq!(centre(1, 1), colour(Camera::Iso));
    assert_eq!(picture.get(0, 0), [127, 127, 127, 255]);
    let path = sheet["path"].as_str().unwrap();
    assert!(path.starts_with("renders/lamp-sheet-r"), "{path}");
    // Nine views make a grid of three; a sheet is at most 1024 px.
    let all: Vec<&str> = vec![
        "front", "back", "left", "right", "top", "bottom", "iso", "gui", "front",
    ];
    let (nine, images) = client.pictures(
        "render_sheet",
        json!({"model": "lamp.bbmodel", "views": all, "size": 500}),
    );
    assert_eq!(
        (
            nine["columns"].as_u64(),
            nine["rows"].as_u64(),
            nine["cell"].as_u64()
        ),
        (Some(3), Some(3), Some(341))
    );
    assert_eq!(nine["emulated"], true);
    assert!(Picture::from_png(&images[0]).unwrap().width <= 1024);
    let (one, _) = client.pictures(
        "render_sheet",
        json!({"model": "lamp.bbmodel", "views": ["top"], "size": 4000}),
    );
    assert_eq!(
        (one["columns"].as_u64(), one["cell"].as_u64()),
        (Some(1), Some(1024))
    );
    let ten: Vec<&str> = vec!["front"; 10];
    assert_eq!(
        client.code(
            "render_sheet",
            json!({"model": "lamp.bbmodel", "views": ten})
        ),
        "E_LIMIT"
    );
    assert_eq!(
        client.code(
            "render_sheet",
            json!({"model": "lamp.bbmodel", "views": []})
        ),
        "E_LIMIT"
    );
    assert_eq!(
        client.code(
            "render_sheet",
            json!({"model": "lamp.bbmodel", "views": ["up"]})
        ),
        "E_INPUT"
    );
    drop(client);
    let asked: Vec<Camera> = fake
        .calls
        .iter()
        .take(4)
        .map(|(camera, _)| *camera)
        .collect();
    assert_eq!(
        asked,
        [Camera::Front, Camera::Right, Camera::Top, Camera::Iso]
    );
}

#[test]
fn without_an_adapter_the_render_tools_say_so_and_the_rest_works() {
    let dir = project();
    let mut fake = Fake {
        fail: Some(RenderError::NoGpu),
        ..Fake::default()
    };
    let mut client = Client::new(dir.path(), false, &mut fake);
    assert_eq!(
        client.code("render_view", json!({"model": "lamp.bbmodel"})),
        "E_NO_GPU"
    );
    assert_eq!(
        client.code("render_sheet", json!({"model": "lamp.bbmodel"})),
        "E_NO_GPU"
    );
    client.ok("model_outline", json!({"model": "lamp.bbmodel"}));
    drop(client);
    let mut broken = Fake {
        fail: Some(RenderError::Failed("device lost".to_owned())),
        ..Fake::default()
    };
    let mut client = Client::new(dir.path(), false, &mut broken);
    assert_eq!(
        client.code("render_view", json!({"model": "lamp.bbmodel"})),
        "E_IO"
    );
    assert!(!dir.path().join("renders").exists());
}

#[test]
fn a_size_over_the_adapter_limit_is_a_limit_not_an_io_error() {
    let dir = project();
    let mut small = Fake {
        fail: Some(RenderError::TooLarge(1024)),
        ..Fake::default()
    };
    let mut client = Client::new(dir.path(), false, &mut small);
    for tool in ["render_view", "render_sheet"] {
        let error = client
            .server
            .call(tool, json!({"model": "lamp.bbmodel", "size": 2048}))
            .expect_err("too large");
        assert_eq!(error.code, "E_LIMIT", "{tool}");
        assert!(error.message.contains("1024"), "{}", error.message);
        assert!(error.hint.contains("smaller"), "{}", error.hint);
    }
    assert!(!dir.path().join("renders").exists());
}

#[test]
fn the_snippets_are_the_commands_and_entries_of_the_spec_for_a_program_and_a_folder() {
    let snippets = snippets(
        Path::new(r"C:\Program Files\cuboid\cuboid.exe"),
        Path::new(r"D:\models\lamp"),
    );
    assert_eq!(
        snippets.claude_code,
        r#"claude mcp add --transport stdio cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp""#
    );
    assert_eq!(
        snippets.claude_desktop,
        r#""cuboid-lamp": {"command": "C:\\Program Files\\cuboid\\cuboid.exe", "args": ["mcp", "--root", "D:\\models\\lamp"]}"#
    );
    assert_eq!(
        snippets.codex_command,
        r#"codex mcp add cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp""#
    );
    assert_eq!(
        snippets.codex_toml,
        "[mcp_servers.cuboid]\ncommand = 'C:\\Program Files\\cuboid\\cuboid.exe'\nargs = ['mcp', '--root', 'D:\\models\\lamp']\ntool_timeout_sec = 120  # the default is 60\n"
    );
    // PowerShell reads a line that starts with a quoted string as text, so on Windows the check
    // starts with the call operator.
    let call = if cfg!(windows) { "& " } else { "" };
    assert_eq!(
        snippets.check,
        format!(
            r#"{call}"C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp" --check"#
        )
    );
    // A name with a quote cannot be a literal TOML string.
    let odd = cuboid_mcp::snippets(Path::new("/opt/it's/cuboid"), Path::new("/work/My Models"));
    assert!(
        odd.codex_toml.contains(r#"command = "/opt/it's/cuboid""#),
        "{}",
        odd.codex_toml
    );
    assert!(
        odd.claude_desktop.starts_with(r#""cuboid-my-models": "#),
        "{}",
        odd.claude_desktop
    );
}

#[test]
fn the_self_check_reads_the_brief_validates_one_cube_and_draws_it() {
    let dir = project();
    let mut fake = Fake::default();
    let report = self_check(dir.path(), &mut fake);
    let names: Vec<&str> = report.steps.iter().map(|step| step.name).collect();
    assert_eq!(names, ["brief", "validate", "render"]);
    assert!(report.ok(), "{}", report.text());
    assert!(
        report.text().lines().all(|line| line.starts_with("ok ")),
        "{}",
        report.text()
    );
    assert_eq!(fake.calls.len(), 1);
    // Nothing is written.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let mut none = Fake {
        fail: Some(RenderError::NoGpu),
        ..Fake::default()
    };
    let report = self_check(dir.path(), &mut none);
    assert!(!report.ok());
    assert!(
        report
            .text()
            .contains("FAIL render: there is no graphics adapter"),
        "{}",
        report.text()
    );
    let report = self_check(&dir.path().join("missing"), &mut none);
    assert_eq!(report.steps.len(), 1);
    assert!(!report.ok());
}
