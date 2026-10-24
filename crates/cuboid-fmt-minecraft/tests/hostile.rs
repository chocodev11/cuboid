//! Damaged and hostile input: no panic, a message or a notice.

mod common;

use std::path::Path;

use common::{Tree, import_at, open, png};
use cuboid_fmt_minecraft::json_export::{JsonOptions, export};
use cuboid_fmt_minecraft::{
    Limits, McError, McVersion, Notice, PackOptions, Refusal, TextureFiles, export_pack, import,
    import_with,
};

/// Files that are all refused, as network and device paths are.
struct Refusing;

impl TextureFiles for Refusing {
    fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        Err(Refusal::Network)
    }
}

const MODEL: &str = r##"{
    "textures": { "all": "block/stone" },
    "elements": [
        { "from": [0, 0, 0], "to": [16, 8, 16],
          "rotation": { "origin": [8, 0, 8], "axis": "y", "angle": 22.5 },
          "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "up" } } }
    ],
    "display": { "gui": { "rotation": [30, 225, 0] } },
    "overrides": [ { "predicate": { "pulling": 1 }, "model": "item/b" } ]
}"##;

fn v(text: &str) -> McVersion {
    McVersion::parse(text).expect("version")
}

#[test]
fn refused_texture_files_are_notices() {
    let imported = import(MODEL.as_bytes(), Path::new("m/x.json"), &Refusing).expect("imports");
    assert!(
        imported.notices.iter().any(|n| matches!(
            n,
            Notice::TextureRefused {
                reason: Refusal::Network,
                ..
            }
        )),
        "{:?}",
        imported.notices
    );
}

#[test]
fn a_texture_that_is_no_picture_is_a_notice() {
    let tree = Tree::default().with(
        "m/assets/minecraft/textures/block/stone.png",
        b"not a png".to_vec(),
    );
    let imported = import_at(MODEL, "m/x.json", &tree);
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::TextureMissing { .. }))
    );
}

#[test]
fn a_parent_that_is_no_model_is_a_notice() {
    let tree = Tree::default().with("m/base.json", b"{ broken".to_vec());
    let imported = import_at(r#"{ "parent": "base" }"#, "m/x.json", &tree);
    assert_eq!(
        imported.notices,
        [Notice::ParentUnreadable {
            parent: "base".to_owned()
        }]
    );
}

#[test]
fn parents_together_stay_within_the_text_limit() {
    let big = format!(r#"{{ "elements": [], "credit": "{}" }}"#, "x".repeat(200));
    let tree = Tree::default().with("m/base.json", big);
    let limits = Limits {
        max_text: 300,
        ..Limits::default()
    };
    let imported = import_with(
        br#"{ "parent": "base" }"#,
        Path::new("m/x.json"),
        &tree,
        &limits,
    )
    .expect("imports");
    assert!(imported.notices.is_empty());
    let twice = Tree::default()
        .with(
            "m/a.json",
            format!(r#"{{ "parent": "b", "credit": "{}" }}"#, "x".repeat(200)),
        )
        .with(
            "m/b.json",
            format!(r#"{{ "credit": "{}" }}"#, "x".repeat(200)),
        );
    let imported = import_with(
        br#"{ "parent": "a" }"#,
        Path::new("m/x.json"),
        &twice,
        &limits,
    )
    .expect("imports");
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::ParentUnreadable { .. }))
    );
}

#[test]
fn a_large_sprite_becomes_one_slab() {
    let tree = Tree::default().with(
        "m/assets/minecraft/textures/item/big.png",
        png(64, 64, [1, 2, 3, 255]),
    );
    let model = r#"{ "parent": "item/generated", "textures": { "layer0": "item/big" } }"#;
    let imported = import_at(model, "m/assets/minecraft/models/item/big.json", &tree);
    assert!(
        imported.notices.contains(&Notice::SpriteSlab),
        "{:?}",
        imported.notices
    );
    assert_eq!(open(imported).elements.len(), 1);
}

#[test]
fn the_texel_budget_is_enforced() {
    let tree = Tree::default().with(
        "m/assets/minecraft/textures/block/stone.png",
        png(16, 16, [1, 2, 3, 255]),
    );
    let limits = Limits {
        max_texels: 100,
        ..Limits::default()
    };
    let imported = import_with(
        MODEL.as_bytes(),
        Path::new("m/assets/minecraft/models/block/x.json"),
        &tree,
        &limits,
    )
    .expect("imports");
    assert!(
        imported.notices.contains(&Notice::TexelBudget),
        "{:?}",
        imported.notices
    );
}

#[test]
fn a_truncated_or_flipped_model_never_panics() {
    let tree = Tree::default().with(
        "m/assets/minecraft/textures/block/stone.png",
        png(4, 4, [1, 2, 3, 255]),
    );
    let bytes = MODEL.as_bytes();
    for end in 0..bytes.len() {
        let _ = import(
            &bytes[..end],
            Path::new("m/assets/minecraft/models/block/x.json"),
            &tree,
        );
    }
    for at in 0..bytes.len() {
        for flip in [0x01u8, 0x20, 0x80, 0xFF] {
            let mut copy = bytes.to_vec();
            copy[at] ^= flip;
            let _ = import(
                &copy,
                Path::new("m/assets/minecraft/models/block/x.json"),
                &tree,
            );
        }
    }
    // A model with an astronomically deep or wide structure is refused, not built.
    let wide = format!("{{ \"elements\": [{}] }}", "{},".repeat(70_000));
    let limits = Limits {
        max_values: 1000,
        ..Limits::default()
    };
    assert_eq!(
        import_with(
            wide.as_bytes(),
            Path::new("m/x.json"),
            &Tree::default(),
            &limits
        )
        .err(),
        Some(McError::Limit("values"))
    );
}

#[test]
fn non_finite_models_are_refused_on_export() {
    let mut doc = open(import_at(
        MODEL,
        "m/assets/minecraft/models/block/x.json",
        &Tree::default(),
    ));
    let id = doc.elements.ids()[0];
    let mut journal = cuboid_model::ChangeJournal::default();
    doc.elements
        .edit(id, &mut journal)
        .expect("element")
        .base
        .s
        .x = f32::NAN;
    assert!(matches!(
        export(&doc, &JsonOptions::new(v("1.21.3"))),
        Err(McError::DoesNotFit { .. })
    ));
    assert!(matches!(
        export_pack(&doc, &PackOptions::new(v("1.21.3"))),
        Err(McError::DoesNotFit { .. })
    ));
}
