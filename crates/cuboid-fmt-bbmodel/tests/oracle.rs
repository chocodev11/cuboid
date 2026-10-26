//! Private projects of the owner. `CUBOID_BBMODEL_REFS` lists their paths, separated by `;`.
//! The test prints the notices and counts, never names or content:
//! `cargo test -p cuboid-fmt-bbmodel --test oracle -- --include-ignored --nocapture`.

mod common;

use std::path::PathBuf;

use common::open;
use cuboid_fmt_bbmodel::{DiskFiles, export, import};

#[test]
#[ignore = "oracle: CUBOID_BBMODEL_REFS"]
fn private_projects_import_and_export() {
    let Some(refs) = std::env::var_os("CUBOID_BBMODEL_REFS") else {
        eprintln!("SKIPPED (oracle absent): CUBOID_BBMODEL_REFS");
        return;
    };
    let refs = refs.to_string_lossy().into_owned();
    let paths: Vec<PathBuf> = refs
        .split(';')
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect();
    assert!(!paths.is_empty(), "the variable names no file");
    for (n, path) in paths.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("sample {n}: {e}"));
        let imported =
            import(&bytes, path, &DiskFiles).unwrap_or_else(|e| panic!("sample {n}: {e}"));
        for notice in &imported.notices {
            eprintln!("sample {n}: notice: {notice}");
        }
        let doc = open(imported);
        let animated = |doc: &cuboid_model::Document| {
            doc.textures
                .entries
                .iter()
                .filter(|entry| entry.animation.is_some())
                .count()
        };
        eprintln!(
            "sample {n}: {} elements, {} meshes, {} textures, {} animated",
            doc.elements.len(),
            doc.meshes.len(),
            doc.textures.entries.len(),
            animated(&doc)
        );
        let out = export(&doc).unwrap_or_else(|e| panic!("sample {n}: {e}"));
        for notice in &out.notices {
            eprintln!("sample {n}: export notice: {notice}");
        }
        let again =
            import(&out.bytes, path, &DiskFiles).unwrap_or_else(|e| panic!("sample {n}: {e}"));
        let back = open(again);
        assert_eq!(doc.elements.len(), back.elements.len(), "sample {n}");
        assert_eq!(doc.meshes.len(), back.meshes.len(), "sample {n}");
        assert_eq!(
            doc.textures.entries.len(),
            back.textures.entries.len(),
            "sample {n}"
        );
        // The strips come back as strips, with the same number of frames.
        assert_eq!(animated(&doc), animated(&back), "sample {n}");
        let frames = |doc: &cuboid_model::Document| -> Vec<u32> {
            doc.textures
                .entries
                .iter()
                .filter_map(|e| Some(e.animation.as_ref()?.tiles(e.texture.image.h)))
                .collect()
        };
        assert_eq!(frames(&doc), frames(&back), "sample {n}");
        eprintln!(
            "sample {n}: frames per animated texture: {:?}",
            frames(&doc)
        );
        eprintln!("sample {n}: round trip ok");
    }
}
