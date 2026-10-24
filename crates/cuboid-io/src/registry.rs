//! The format registry: the 23 entries of interop 4.1, extension lookup and dialog lists.

use std::path::Path;
use std::sync::Arc;

use crate::contract::{
    Caps, Exporter, FormatEntry, FormatVisibility, ImportMode, Importer, Lookup, LookupMode,
    MessageNames, Visibility,
};
use crate::names::extension;

/// What an entry needs to count as having an importer (open mode) or an exporter (save mode).
const READS: Caps = Caps::LOAD.union(Caps::IMPORT);
const WRITES: Caps = Caps::SAVE.union(Caps::EXPORT);

/// One row of the table in interop 4.1.
struct Row {
    key: &'static str,
    label: &'static str,
    filter: &'static str,
    caps: Caps,
    names: MessageNames,
    visibility: Visibility,
}

const fn names(import: Option<&'static str>, export: Option<&'static str>) -> MessageNames {
    MessageNames { import, export }
}

/// The rows in registry order; the order is semantic (interop 4.3, 4.4).
///
/// spec: interop 4.1
static ROWS: [Row; 23] = [
    Row {
        key: "cubik",
        label: "Cubik model",
        filter: "*.cubik",
        caps: Caps::LOAD.union(Caps::SAVE),
        names: names(Some("Cubik"), Some("Cubik")),
        visibility: Visibility::Always,
    },
    Row {
        key: "cub",
        label: "Cubik model alpha",
        filter: "*.cub",
        caps: Caps::LOAD,
        names: names(Some("CUB"), None),
        visibility: Visibility::Always,
    },
    Row {
        key: "json",
        label: "Minecraft model",
        filter: "*.json",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("JSON"), Some("Minecraft 3D Model")),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "mcpe",
        label: "Minecraft PE Geometries",
        filter: "*.mcpe",
        caps: Caps::IMPORT,
        names: names(Some("MCPE entity"), None),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "js",
        label: "Minecraft PE Entity Part model",
        filter: "*.js",
        caps: Caps::EXPORT,
        names: names(None, Some("Minecraft PE Entity Model")),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "jpm",
        label: "Optifine Entity part model",
        filter: "*.jpm",
        caps: Caps::EXPORT,
        names: names(None, Some("Optifine Model Part")),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "java",
        label: "Minecraft Mod Entity model",
        filter: "*.java",
        caps: Caps::EXPORT,
        names: names(None, Some("Forge Java model")),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "zip",
        label: "Minecraft ResourcePack",
        filter: "*.zip",
        caps: Caps::EXPORT,
        names: names(None, Some("Minecraft ResourcePack")),
        visibility: Visibility::Minecraft,
    },
    Row {
        key: "stonehearth",
        label: "Stonehearth model",
        filter: "stonehearth",
        caps: Caps::EXPORT,
        names: names(None, Some("Stonehearth data")),
        visibility: Visibility::Stonehearth,
    },
    Row {
        key: "staxel",
        label: "Staxel model",
        filter: "staxel",
        caps: Caps::EXPORT,
        names: names(None, Some("Staxel data")),
        visibility: Visibility::Staxel,
    },
    Row {
        key: "obj",
        label: "Wavefront obj model",
        filter: "*.obj",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("Wavefront OBJ"), Some("Wavefront OBJ")),
        visibility: Visibility::Mesh,
    },
    Row {
        key: "stl",
        label: "Stereolithography Model",
        filter: "*.stl",
        caps: Caps::EXPORT,
        names: names(None, Some("Stereolithography STL")),
        visibility: Visibility::Mesh,
    },
    Row {
        key: "png",
        label: "Model Render",
        filter: "*.png",
        caps: Caps::EXPORT,
        names: names(None, Some("PNG render")),
        visibility: Visibility::Always,
    },
    Row {
        key: "png",
        label: "Sprite",
        filter: "*.png",
        caps: Caps::IMPORT,
        names: names(Some("PNG"), None),
        visibility: Visibility::Always,
    },
    Row {
        key: "binvox",
        label: "Binvox voxels",
        filter: "*.binvox",
        caps: Caps::EXPORT,
        names: names(None, Some("Binvox")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "csv",
        label: "Sproxel voxels",
        filter: "*.csv",
        caps: Caps::EXPORT,
        names: names(None, Some("Sproxel CSV")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "qb",
        label: "Qubicle binary voxels",
        filter: "*.qb",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("Qubicle Binary QB"), Some("Qubicle Binary")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "qef",
        label: "Qubicle exchange voxels",
        filter: "*.qef",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("Qubicle QEF"), Some("Qubicle Exchange")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "schematic",
        label: "MCEdit voxels",
        filter: "*.schematic",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("MCEdit SCHEMATIC"), Some("MCEdit Schematic")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "nbt",
        label: "MC Structures voxels",
        filter: "*.nbt",
        caps: Caps::IMPORT,
        names: names(Some("MC Structure NBT"), None),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "vox",
        label: "Magicavoxel voxels",
        filter: "*.vox",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("MagicaVoxel VOX"), Some("Magicavoxel VOX")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "zox",
        label: "Zoxel voxels",
        filter: "*.zox",
        caps: Caps::IMPORT.union(Caps::EXPORT),
        names: names(Some("Zoxel ZOX"), Some("Zoxel ZOX")),
        visibility: Visibility::Voxel,
    },
    Row {
        key: "lnk",
        label: "Windows Shortcut",
        filter: "*.lnk",
        caps: Caps::IMPORT,
        names: names(Some("LNK"), None),
        visibility: Visibility::BuiltinBrowserOnly,
    },
];

impl FormatVisibility {
    /// Whether an entry with this rule is part of the registry at all. Rule H (Mesh) entries
    /// stay registered and are hidden from the dialog lists only.
    fn registers(&self, rule: Visibility) -> bool {
        match rule {
            Visibility::Always | Visibility::Mesh => true,
            Visibility::Minecraft => !self.hide_minecraft,
            Visibility::Stonehearth => !self.hide_stonehearth,
            Visibility::Staxel => !self.hide_staxel,
            Visibility::Voxel => !self.hide_voxel,
            Visibility::BuiltinBrowserOnly => self.builtin_browser,
        }
    }
}

impl FormatEntry {
    /// Whether the entry has an importer: it can load or import.
    pub fn can_read(&self) -> bool {
        self.caps.intersects(READS)
    }

    /// Whether the entry has an exporter: it can save or export.
    pub fn can_write(&self) -> bool {
        self.caps.intersects(WRITES)
    }

    /// The filter of a native dialog, `<label> (*.<key>)|*.<key>`.
    ///
    /// spec: interop 4.4
    pub fn native_filter(&self) -> String {
        format!("{} (*.{key})|*.{key}", self.label, key = self.key)
    }
}

/// The 23 entries of the format registry in interop 4.1 order. The order is semantic.
///
/// spec: interop 4.1-4.4, arch 6 (cuboid-io), arch 12.2
pub struct Registry {
    entries: Vec<FormatEntry>,
}

impl Registry {
    /// The registry for the given visibility preferences; entries hidden by rule M, SH, SX or V
    /// are absent, rule L entries exist only with the built-in browser.
    pub fn new(vis: &FormatVisibility) -> Registry {
        let entries = ROWS
            .iter()
            .filter(|row| vis.registers(row.visibility))
            .map(|row| FormatEntry {
                key: row.key,
                label: row.label,
                filter: row.filter,
                caps: row.caps,
                names: row.names,
                visibility: row.visibility,
                importer: None,
                exporter: None,
            })
            .collect();
        Registry { entries }
    }

    /// Attaches the importer of the first entry with this key that can import (for `png` that is
    /// Sprite). The format crates implement the codecs but sit above this crate, so the
    /// application composes them into the registry at start-up. A key of a hidden entry is
    /// ignored: the application attaches every codec whatever the visibility.
    pub fn attach_importer(&mut self, key: &str, importer: Arc<dyn Importer>) {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.key == key && entry.can_read());
        if let Some(entry) = entry {
            entry.importer = Some(importer);
        }
    }

    /// Attaches the exporter of the first entry with this key that can export (for `png` that is
    /// Model Render). A key of a hidden entry is ignored.
    pub fn attach_exporter(&mut self, key: &str, exporter: Arc<dyn Exporter>) {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.key == key && entry.can_write());
        if let Some(entry) = entry {
            entry.exporter = Some(exporter);
        }
    }

    /// Looks a path up: every `.` of the extension is removed, the keys are compared in registry
    /// order, case-sensitively in `ImportMode::Cubik29` and case-insensitively in `Modern`.
    /// `png` resolves to Sprite when opening and to Model Render when saving.
    ///
    /// The first matching entry that has an importer (open mode) or an exporter (save mode)
    /// wins, otherwise the last matching entry.
    ///
    /// spec: interop 4.3
    pub fn lookup(&self, path: &Path, mode: LookupMode, im: ImportMode) -> Lookup<'_> {
        let ext = extension(path);
        let key = ext.trim_start_matches('.');
        if key.is_empty() {
            return Lookup::NoExtension;
        }
        let matches = |entry: &&FormatEntry| match im {
            ImportMode::Cubik29 => entry.key == key,
            ImportMode::Modern => entry.key.eq_ignore_ascii_case(key),
        };
        let usable = |entry: &FormatEntry| match mode {
            LookupMode::Open => entry.can_read(),
            LookupMode::Save => entry.can_write(),
        };
        let mut last = None;
        for entry in self.entries.iter().filter(matches) {
            if usable(entry) {
                return Lookup::Entry(entry);
            }
            last = Some(entry);
        }
        last.map_or(Lookup::Unknown(ext), Lookup::Entry)
    }

    /// The entries a dialog for the capability set `caps` lists: every visible entry whose
    /// capabilities include all of `caps`, in registry order. The entries of rule H (Mesh) are
    /// left out while "Hide Mesh formats" is on.
    ///
    /// spec: interop 4.2, interop 4.4
    pub fn dialog_entries(&self, caps: Caps, vis: &FormatVisibility) -> Vec<&FormatEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.caps.contains(caps)
                    && !(vis.hide_mesh && entry.visibility == Visibility::Mesh)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
