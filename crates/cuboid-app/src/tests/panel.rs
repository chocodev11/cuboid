//! The edits of the properties panel, the texture panel and the palette as the UI sends them:
//! each reaches its core command, survives a save and shows in the Minecraft JSON.

use cuboid_core::{Command, SelectOp, Target};
use cuboid_fmt_minecraft::McVersion;
use cuboid_fmt_minecraft::json_export::{JsonOptions, export};
use cuboid_model::image::{Image, RgbaBuf, encode_png};
use cuboid_model::{Element, ElementFlags, FaceFlags, FaceName};
use cuboid_num::pack::to_byte;
use cuboid_ui::{Edit, ElementFlag, Face, FaceChange, TransformField, UiRequest};
use glam::Vec3;

use super::{Fixture, Intent, Level};

impl Fixture {
    /// What the UI sends for an edit, applied the way the app does.
    fn ui_edit(&mut self, edit: Edit) {
        let app = &mut self.app;
        app.front.apply(&mut app.state, UiRequest::Edit(edit));
        self.settle();
    }

    fn run(&mut self, command: Command) {
        self.state().inbox.push(Intent::Command(Box::new(command)));
        self.tick();
    }

    fn element(&mut self) -> Element {
        self.state().editor.doc().elements.items()[0]
            .as_ref()
            .clone()
    }

    fn not_available(&self) -> Vec<String> {
        let toasts = self.app.state.toasts.iter();
        toasts
            .filter(|toast| toast.text.contains("not available yet"))
            .map(|toast| toast.text.clone())
            .collect()
    }
}

/// A turned cube with every edit of the panel applied, selected.
fn edited() -> Fixture {
    let mut fx = Fixture::new();
    fx.edit();
    fx.run(Command::Select {
        targets: vec![Target::Element(0)],
        op: SelectOp::Replace,
    });
    let steps = fx.state().editor.core.history().undo_len();
    fx.ui_edit(Edit::Transform {
        field: TransformField::Rotation,
        value: Vec3::new(0.0, 22.5, 0.0),
        commit: true,
    });
    fx.ui_edit(Edit::Transform {
        field: TransformField::Pivot,
        value: Vec3::new(1.0, 2.0, 3.0),
        commit: true,
    });
    fx.ui_edit(Edit::SetColor {
        color: [0.4, 0.8, 1.0, 1.0],
        commit: true,
    });
    fx.ui_edit(Edit::SetFlag {
        flag: ElementFlag::Shade,
        on: false,
    });
    for change in [
        (Face::Up, FaceChange::Enabled(false)),
        (Face::North, FaceChange::Tint(2)),
        (Face::North, FaceChange::Cull(Some(Face::Down))),
    ] {
        let (face, change) = change;
        fx.ui_edit(Edit::Face { face, change });
    }
    let steps = fx.state().editor.core.history().undo_len() - steps;
    assert_eq!(steps, 7, "every edit is one undo step");
    fx
}

fn check_edits(element: &Element) {
    assert_eq!(element.base.o, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(element.color.map(to_byte), [102, 204, 255, 255]);
    assert!(!element.flags.contains(ElementFlags::SHADE));
    assert!(!element.faces[FaceName::Up as usize].active);
    assert!(element.faces[FaceName::Down as usize].active);
    let north = &element.faces[FaceName::North as usize];
    assert_eq!(north.tint, 2);
    assert_eq!(north.cull, FaceFlags::DOWN);
}

#[test]
fn the_panel_edits_are_commands_and_no_longer_say_not_available() {
    let mut fx = edited();
    check_edits(&fx.element());
    assert_eq!(fx.not_available(), Vec::<String>::new());
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .all(|text| !text.contains("available"))
    );

    // One undo takes the newest edit back.
    fx.run(Command::Undo);
    assert_eq!(
        fx.element().faces[FaceName::North as usize].cull,
        FaceFlags::empty()
    );
}

#[test]
fn a_drag_of_the_pivot_or_the_colour_is_one_undo_step() {
    let mut fx = Fixture::new();
    fx.edit();
    fx.run(Command::Select {
        targets: vec![Target::Element(0)],
        op: SelectOp::Replace,
    });
    let steps = fx.state().editor.core.history().undo_len();
    for x in [1.0, 2.0, 3.0] {
        fx.ui_edit(Edit::Transform {
            field: TransformField::Pivot,
            value: Vec3::new(x, 0.0, 0.0),
            commit: x > 2.5,
        });
    }
    for c in [0.3, 0.5, 0.7] {
        fx.ui_edit(Edit::SetColor {
            color: [c, c, c, 1.0],
            commit: c > 0.6,
        });
    }
    assert_eq!(fx.state().editor.core.history().undo_len(), steps + 2);
}

#[test]
fn the_edits_of_the_panel_are_kept_by_the_file() {
    let mut fx = edited();
    let path = fx.path("edits.cubik");
    fx.send(Intent::SaveAs(Some(path.clone())));
    assert!(
        fx.toasts(Level::Error).is_empty(),
        "{:?}",
        fx.toasts(Level::Error)
    );

    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    check_edits(&other.element());
}

#[test]
fn the_edits_of_the_panel_are_written_to_the_minecraft_json() {
    let mut fx = edited();
    let version = McVersion::parse("1.21.3").expect("a version");
    let out = export(fx.state().editor.doc(), &JsonOptions::new(version)).expect("fits");
    let model: serde_json::Value = serde_json::from_str(&out.json).expect("JSON");
    let element = &model["elements"][0];
    let origin = element["rotation"]["origin"].as_array().expect("an origin");
    let origin: Vec<f64> = origin
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    assert_eq!(origin, [1.0, 2.0, -3.0], "Minecraft space negates Z");
    assert_eq!(element["shade"], false);
    let faces = &element["faces"];
    assert!(
        faces.get("up").is_none(),
        "a face that is off is not written"
    );
    assert!(faces.get("down").is_some());
    assert_eq!(faces["north"]["tintindex"], 2);
    assert_eq!(faces["north"]["cullface"], "down");

    // The file gives the same model.
    let path = fx.path("edits.cubik");
    fx.send(Intent::SaveAs(Some(path.clone())));
    let mut other = Fixture::new();
    other.send(Intent::Open(Some(path)));
    let again = export(other.state().editor.doc(), &JsonOptions::new(version)).expect("fits");
    assert_eq!(again.json, out.json);
}

#[test]
fn a_swatch_leaves_the_palette_and_the_view_follows() {
    let mut fx = Fixture::new();
    fx.run(Command::SetupVoxels { resolution: 16 });
    for color in [[10, 20, 30, 255], [40, 50, 60, 255]] {
        fx.run(Command::PaletteAdd { color });
    }
    let refresh = |fx: &mut Fixture| {
        let app = &mut fx.app;
        let query = app.state.editor.core.query();
        app.front.model.update(&mut app.state, &query);
        app.front.model.palette.colors.len()
    };
    assert_eq!(refresh(&mut fx), 2);

    fx.ui_edit(Edit::RemovePaletteColor(0));
    assert_eq!(refresh(&mut fx), 1);
    let left = fx
        .state()
        .editor
        .doc()
        .voxels
        .as_ref()
        .map(|w| w.palette.values().to_vec());
    assert_eq!(left, Some(vec![0, 0xFF28_323C]));
    assert!(fx.not_available().is_empty());

    // A swatch the view does not have is ignored.
    fx.ui_edit(Edit::RemovePaletteColor(5));
    assert_eq!(refresh(&mut fx), 1);
}

/// A PNG of 2 by 2 texels in the folder of the fixture.
fn picture(fx: &Fixture, name: &str, texel: [u8; 4]) -> (std::path::PathBuf, usize) {
    let mut buf = RgbaBuf::new(2, 2);
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        buf.set(x, y, texel);
    }
    let bytes = encode_png(&Image::from_rgba(buf)).expect("a PNG");
    let path = fx.path(name);
    std::fs::write(&path, &bytes).expect("written");
    (path, bytes.len())
}

fn pixels(fx: &mut Fixture, label: &str) -> Vec<u8> {
    let doc = fx.state().editor.doc();
    let entry = doc.textures.entries.iter().find(|e| &*e.label == label);
    entry.expect("the texture").texture.image.rgba().to_vec()
}

#[test]
fn a_texture_is_read_again_from_its_file_as_one_undo_step() {
    let mut fx = Fixture::new();
    let (path, _) = picture(&fx, "tex.png", [1, 2, 3, 255]);
    // The document comes with a texture that names its file.
    let mut doc = fx.state().editor.doc().clone();
    let source = &mut doc.textures.entries[1].texture.source;
    source.path = Some(path.to_string_lossy().replace('\\', "/"));
    let base = doc.revision;
    fx.state()
        .editor
        .core
        .swap_document(doc, base, cuboid_core::SwapKind::Open)
        .expect("not stale");
    let old = pixels(&mut fx, "texture");
    let steps = fx.state().editor.core.history().undo_len();

    // The file changes on disk; the button reads it again.
    let (_, size) = picture(&fx, "tex.png", [200, 100, 50, 255]);
    fx.ui_edit(Edit::ReloadTexture(1));
    let new = pixels(&mut fx, "texture");
    assert_eq!(new, [200, 100, 50, 255].repeat(4));
    assert_eq!(fx.state().editor.core.history().undo_len(), steps + 1);
    let entry = &fx.state().editor.doc().textures.entries[1];
    assert_eq!((entry.texture.image.w, entry.texture.image.h), (2, 2));
    assert_eq!(entry.texture.source.file_size, size as u64);
    assert!(entry.texture.source.signature.is_some());
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .any(|text| text.starts_with("Reloaded"))
    );
    assert!(fx.not_available().is_empty());

    // Reading it again changes nothing.
    fx.ui_edit(Edit::ReloadTexture(1));
    assert_eq!(fx.state().editor.core.history().undo_len(), steps + 1);
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .any(|text| text.contains("up to date"))
    );

    fx.run(Command::Undo);
    assert_eq!(pixels(&mut fx, "texture"), old);
}

#[test]
fn a_texture_that_cannot_be_read_again_says_why_and_stays() {
    let mut fx = Fixture::new();
    // The default texture was never loaded from a file.
    fx.ui_edit(Edit::ReloadTexture(1));
    assert!(
        fx.toasts(Level::Info)
            .iter()
            .any(|text| text.contains("not loaded from a file"))
    );

    let (path, _) = picture(&fx, "gone.png", [1, 2, 3, 255]);
    let mut doc = fx.state().editor.doc().clone();
    doc.textures.entries[1].texture.source.path = Some(path.to_string_lossy().replace('\\', "/"));
    let base = doc.revision;
    let swapped = fx
        .state()
        .editor
        .core
        .swap_document(doc, base, cuboid_core::SwapKind::Open);
    assert!(swapped.is_ok());
    std::fs::remove_file(&path).expect("removed");
    let old = pixels(&mut fx, "texture");
    fx.ui_edit(Edit::ReloadTexture(1));
    assert!(
        fx.toasts(Level::Warn)
            .iter()
            .any(|text| text.contains("does not exist"))
    );
    assert_eq!(pixels(&mut fx, "texture"), old);

    // A number that is no texture is ignored.
    fx.ui_edit(Edit::ReloadTexture(9));
    assert!(fx.toasts(Level::Error).is_empty());
}
