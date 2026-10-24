use std::path::Path;
use std::sync::Arc;

use cuboid_model::DocSnapshot;

use super::*;
use crate::contract::{
    Answer, Artifacts, BuildStep, ExportCx, ExportSession, ImportCx, ImportSession, IoError,
    ProgressSink, Prompt, Source, Step,
};

/// Every entry of the registry, rule L satisfied.
fn full() -> Registry {
    Registry::new(&FormatVisibility {
        builtin_browser: true,
        ..FormatVisibility::default()
    })
}

fn keys(registry: &Registry) -> Vec<&'static str> {
    registry.entries.iter().map(|entry| entry.key).collect()
}

/// A lookup result as `key:label`, `?.ext` for an unknown type and `-` for no extension.
fn found(registry: &Registry, path: &str, mode: LookupMode, im: ImportMode) -> String {
    match registry.lookup(Path::new(path), mode, im) {
        Lookup::NoExtension => "-".to_owned(),
        Lookup::Unknown(ext) => format!("?{ext}"),
        Lookup::Entry(entry) => format!("{}:{}", entry.key, entry.label),
    }
}

/// A dialog list as keys; the two `png` entries are told apart by their label.
fn listed(registry: &Registry, caps: Caps, vis: &FormatVisibility) -> Vec<String> {
    registry
        .dialog_entries(caps, vis)
        .iter()
        .map(|entry| match entry.key {
            "png" => format!("png {}", entry.label),
            key => key.to_owned(),
        })
        .collect()
}

fn browser() -> FormatVisibility {
    FormatVisibility {
        builtin_browser: true,
        ..FormatVisibility::default()
    }
}

// ---- interop 4.1 ------------------------------------------------------------------------

#[test]
fn registry_follows_the_table_of_interop_4_1() {
    // Key, label, filter, capabilities (Load 1, Save 2, Import 4, Export 8), import name and
    // export name.
    type Row = (
        &'static str,
        &'static str,
        &'static str,
        u8,
        Option<&'static str>,
        Option<&'static str>,
    );
    #[rustfmt::skip]
    let table: [Row; 23] = [
        ("cubik", "Cubik model", "*.cubik", 3, Some("Cubik"), Some("Cubik")),
        ("cub", "Cubik model alpha", "*.cub", 1, Some("CUB"), None),
        ("json", "Minecraft model", "*.json", 12, Some("JSON"), Some("Minecraft 3D Model")),
        ("mcpe", "Minecraft PE Geometries", "*.mcpe", 4, Some("MCPE entity"), None),
        ("js", "Minecraft PE Entity Part model", "*.js", 8, None, Some("Minecraft PE Entity Model")),
        ("jpm", "Optifine Entity part model", "*.jpm", 8, None, Some("Optifine Model Part")),
        ("java", "Minecraft Mod Entity model", "*.java", 8, None, Some("Forge Java model")),
        ("zip", "Minecraft ResourcePack", "*.zip", 8, None, Some("Minecraft ResourcePack")),
        ("stonehearth", "Stonehearth model", "stonehearth", 8, None, Some("Stonehearth data")),
        ("staxel", "Staxel model", "staxel", 8, None, Some("Staxel data")),
        ("obj", "Wavefront obj model", "*.obj", 12, Some("Wavefront OBJ"), Some("Wavefront OBJ")),
        ("stl", "Stereolithography Model", "*.stl", 8, None, Some("Stereolithography STL")),
        ("png", "Model Render", "*.png", 8, None, Some("PNG render")),
        ("png", "Sprite", "*.png", 4, Some("PNG"), None),
        ("binvox", "Binvox voxels", "*.binvox", 8, None, Some("Binvox")),
        ("csv", "Sproxel voxels", "*.csv", 8, None, Some("Sproxel CSV")),
        ("qb", "Qubicle binary voxels", "*.qb", 12, Some("Qubicle Binary QB"), Some("Qubicle Binary")),
        ("qef", "Qubicle exchange voxels", "*.qef", 12, Some("Qubicle QEF"), Some("Qubicle Exchange")),
        ("schematic", "MCEdit voxels", "*.schematic", 12, Some("MCEdit SCHEMATIC"), Some("MCEdit Schematic")),
        ("nbt", "MC Structures voxels", "*.nbt", 4, Some("MC Structure NBT"), None),
        ("vox", "Magicavoxel voxels", "*.vox", 12, Some("MagicaVoxel VOX"), Some("Magicavoxel VOX")),
        ("zox", "Zoxel voxels", "*.zox", 12, Some("Zoxel ZOX"), Some("Zoxel ZOX")),
        ("lnk", "Windows Shortcut", "*.lnk", 4, Some("LNK"), None),
    ];
    let registry = full();
    assert_eq!(registry.entries.len(), table.len());
    for (entry, (key, label, filter, caps, import, export)) in registry.entries.iter().zip(table) {
        assert_eq!(
            (entry.key, entry.label, entry.filter, entry.caps.bits()),
            (key, label, filter, caps),
            "{key}"
        );
        assert_eq!(
            (entry.names.import, entry.names.export),
            (import, export),
            "{key}"
        );
    }
}

#[test]
fn a_name_exists_exactly_for_what_the_entry_can_do() {
    for entry in &full().entries {
        assert_eq!(
            entry.names.import.is_some(),
            entry.can_read(),
            "{}",
            entry.label
        );
        assert_eq!(
            entry.names.export.is_some(),
            entry.can_write(),
            "{}",
            entry.label
        );
    }
}

// ---- interop 4.2 ------------------------------------------------------------------------

#[test]
fn visibility_rules_remove_or_keep_entries() {
    let removed = |vis: FormatVisibility| {
        let kept = keys(&Registry::new(&FormatVisibility {
            builtin_browser: true,
            ..vis
        }));
        keys(&full())
            .into_iter()
            .filter(|key| !kept.contains(key))
            .collect::<Vec<_>>()
    };
    let none = FormatVisibility::default();
    assert_eq!(
        removed(FormatVisibility {
            hide_minecraft: true,
            ..none
        }),
        ["json", "mcpe", "js", "jpm", "java", "zip"]
    );
    assert_eq!(
        removed(FormatVisibility {
            hide_stonehearth: true,
            ..none
        }),
        ["stonehearth"]
    );
    assert_eq!(
        removed(FormatVisibility {
            hide_staxel: true,
            ..none
        }),
        ["staxel"]
    );
    assert_eq!(
        removed(FormatVisibility {
            hide_voxel: true,
            ..none
        }),
        [
            "binvox",
            "csv",
            "qb",
            "qef",
            "schematic",
            "nbt",
            "vox",
            "zox"
        ]
    );
    // Rule H keeps the entries registered.
    assert!(
        removed(FormatVisibility {
            hide_mesh: true,
            ..none
        })
        .is_empty()
    );
    // Rule L: the shortcut entry exists only with the built-in browser.
    assert_eq!(keys(&Registry::new(&none)).len(), 22);
    assert!(!keys(&Registry::new(&none)).contains(&"lnk"));
}

#[test]
fn a_hidden_entry_is_an_unknown_file_type() {
    let vox = |vis: FormatVisibility| {
        found(
            &Registry::new(&vis),
            "a.vox",
            LookupMode::Open,
            ImportMode::Cubik29,
        )
    };
    assert_eq!(vox(FormatVisibility::default()), "vox:Magicavoxel voxels");
    let hidden = FormatVisibility {
        hide_voxel: true,
        ..FormatVisibility::default()
    };
    assert_eq!(vox(hidden), "?.vox");
}

// ---- interop 4.3 ------------------------------------------------------------------------

#[test]
fn lookup_follows_the_rules_of_interop_4_3() {
    use ImportMode::{Cubik29, Modern};
    use LookupMode::{Open, Save};
    let registry = full();
    let cases = [
        // The extension is the text from the last dot of the file name.
        ("C:\\models\\m.cubik", Open, Cubik29, "cubik:Cubik model"),
        ("/models/m.cubik", Save, Cubik29, "cubik:Cubik model"),
        ("a.b.vox", Open, Cubik29, "vox:Magicavoxel voxels"),
        ("model.tar.gz", Open, Cubik29, "?.gz"),
        (".hidden", Open, Cubik29, "?.hidden"),
        ("noext", Open, Cubik29, "-"),
        ("trailing.", Open, Cubik29, "-"),
        ("dir.d/noext", Open, Cubik29, "-"),
        ("C:\\dir.d\\noext", Open, Cubik29, "-"),
        ("D:file", Open, Cubik29, "-"),
        // Case-sensitive in cubik29; the unknown type keeps its dot and its case.
        ("m.CUBIK", Open, Cubik29, "?.CUBIK"),
        ("m.Vox", Save, Cubik29, "?.Vox"),
        ("m.CUBIK", Open, Modern, "cubik:Cubik model"),
        ("m.Vox", Save, Modern, "vox:Magicavoxel voxels"),
        // png: Sprite when opening, Model Render when saving.
        ("x.png", Open, Cubik29, "png:Sprite"),
        ("x.png", Save, Cubik29, "png:Model Render"),
        ("x.PNG", Save, Modern, "png:Model Render"),
        // The first entry with the needed capability wins ...
        ("x.json", Open, Cubik29, "json:Minecraft model"),
        ("x.obj", Save, Cubik29, "obj:Wavefront obj model"),
        // ... otherwise the last matching entry, which lacks the capability.
        ("x.cub", Save, Cubik29, "cub:Cubik model alpha"),
        ("x.mcpe", Save, Cubik29, "mcpe:Minecraft PE Geometries"),
        ("x.js", Open, Cubik29, "js:Minecraft PE Entity Part model"),
        ("x.lnk", Save, Cubik29, "lnk:Windows Shortcut"),
        (
            "x.stonehearth",
            Save,
            Cubik29,
            "stonehearth:Stonehearth model",
        ),
    ];
    for (path, mode, im, expected) in cases {
        assert_eq!(
            found(&registry, path, mode, im),
            expected,
            "{path} {mode:?} {im:?}"
        );
    }
}

#[test]
fn the_shortcut_entry_needs_the_built_in_browser() {
    let lookup = |builtin_browser| {
        let vis = FormatVisibility {
            builtin_browser,
            ..FormatVisibility::default()
        };
        found(
            &Registry::new(&vis),
            "a.lnk",
            LookupMode::Open,
            ImportMode::Cubik29,
        )
    };
    assert_eq!(lookup(true), "lnk:Windows Shortcut");
    assert_eq!(lookup(false), "?.lnk");
}

// ---- interop 4.4 ------------------------------------------------------------------------

#[test]
fn dialog_lists_follow_the_table_of_interop_4_4() {
    let registry = full();
    let vis = browser();
    // Open and Merge.
    assert_eq!(listed(&registry, Caps::LOAD, &vis), ["cubik", "cub"]);
    // Save as.
    assert_eq!(listed(&registry, Caps::SAVE, &vis), ["cubik"]);
    assert_eq!(
        listed(&registry, Caps::IMPORT, &vis),
        [
            "json",
            "mcpe",
            "obj",
            "png Sprite",
            "qb",
            "qef",
            "schematic",
            "nbt",
            "vox",
            "zox",
            "lnk"
        ]
    );
    assert_eq!(
        listed(&registry, Caps::EXPORT, &vis),
        [
            "json",
            "js",
            "jpm",
            "java",
            "zip",
            "stonehearth",
            "staxel",
            "obj",
            "stl",
            "png Model Render",
            "binvox",
            "csv",
            "qb",
            "qef",
            "schematic",
            "vox",
            "zox"
        ]
    );
}

#[test]
fn the_import_list_has_no_shortcut_entry_without_the_built_in_browser() {
    let vis = FormatVisibility::default();
    let registry = Registry::new(&vis);
    let import = listed(&registry, Caps::IMPORT, &vis);
    assert_eq!(import.len(), 10);
    assert!(!import.contains(&"lnk".to_owned()));
}

#[test]
fn dialog_lists_hide_mesh_formats_but_a_typed_path_still_resolves() {
    let vis = FormatVisibility {
        hide_mesh: true,
        ..browser()
    };
    let registry = Registry::new(&vis);
    assert!(!listed(&registry, Caps::IMPORT, &vis).contains(&"obj".to_owned()));
    let export = listed(&registry, Caps::EXPORT, &vis);
    assert!(!export.contains(&"obj".to_owned()) && !export.contains(&"stl".to_owned()));
    assert_eq!(export.len(), 15);
    assert_eq!(
        found(
            &registry,
            "typed.stl",
            LookupMode::Save,
            ImportMode::Cubik29
        ),
        "stl:Stereolithography Model"
    );
}

#[test]
fn a_dialog_needs_every_requested_capability() {
    let vis = browser();
    assert_eq!(listed(&full(), Caps::LOAD | Caps::SAVE, &vis), ["cubik"]);
    assert!(listed(&full(), Caps::LOAD | Caps::EXPORT, &vis).is_empty());
}

#[test]
fn native_dialogs_render_label_and_key() {
    let registry = full();
    let filters: Vec<String> = registry
        .dialog_entries(Caps::LOAD, &FormatVisibility::default())
        .iter()
        .map(|entry| entry.native_filter())
        .collect();
    assert_eq!(
        filters,
        [
            "Cubik model (*.cubik)|*.cubik",
            "Cubik model alpha (*.cub)|*.cub"
        ]
    );
}

// ---- composition ------------------------------------------------------------------------

struct Codec;

impl Importer for Codec {
    fn begin(&self, _: Source, _: &ImportCx) -> Result<Box<dyn ImportSession>, IoError> {
        Err(IoError::Invalid("test codec".to_owned()))
    }
}

struct Idle;

impl ExportSession for Idle {
    fn prompt(&mut self) -> Option<Prompt> {
        None
    }
    fn answer(&mut self, _: Answer) -> Step {
        Step::Continue
    }
    fn build(&mut self, _: &mut dyn ProgressSink) -> Result<BuildStep, IoError> {
        Ok(BuildStep::Done(Artifacts::default()))
    }
    fn post_build_prompt(&mut self) -> Option<Prompt> {
        None
    }
}

impl Exporter for Codec {
    fn begin(&self, _: Arc<DocSnapshot>, _: &ExportCx) -> Box<dyn ExportSession> {
        Box::new(Idle)
    }
}

#[test]
fn codecs_attach_to_the_entry_that_can_do_the_job() {
    let mut registry = full();
    registry.attach_importer("png", Arc::new(Codec));
    registry.attach_exporter("png", Arc::new(Codec));
    registry.attach_importer("js", Arc::new(Codec));
    let attached: Vec<_> = registry
        .entries
        .iter()
        .filter(|entry| entry.importer.is_some() || entry.exporter.is_some())
        .map(|entry| {
            (
                entry.label,
                entry.importer.is_some(),
                entry.exporter.is_some(),
            )
        })
        .collect();
    // Sprite imports and Model Render exports; js has nothing to import.
    assert_eq!(
        attached,
        [("Model Render", false, true), ("Sprite", true, false)]
    );
}

#[test]
fn a_codec_for_a_hidden_format_is_ignored() {
    let vis = FormatVisibility {
        hide_voxel: true,
        ..FormatVisibility::default()
    };
    let mut registry = Registry::new(&vis);
    registry.attach_importer("vox", Arc::new(Codec));
    assert!(
        registry
            .entries
            .iter()
            .all(|entry| entry.importer.is_none())
    );
}

#[test]
fn the_registry_is_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Registry>();
}
