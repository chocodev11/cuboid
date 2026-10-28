//! The preset of a document (roadmap_lean 6.1): Cubik, or Minecraft Java for a game version. A
//! preset sets the editing, rendering, validation and export defaults at once. The settings live
//! in the editor next to the document, are stored in the `cuboid.json` entry of a `.cubik` file
//! and are described to the UI each frame.

use std::sync::{Arc, OnceLock};

use cuboid_assets::mc_versions::{McVersion, mc_versions};
use cuboid_fmt_cubik::ProjectMeta;
use cuboid_library::brief::{minecraft_template, restrict_rotations};
use cuboid_library::{Brief, DisplayPreset};
use cuboid_model::{Document, EditEnv, GuiLight, ModelKind, check_invariants, mc};
use cuboid_render::{ItemLight, RenderSettings, ShadingMode};
use cuboid_ui::{PresetKind, ProjectChange, ProjectView, Shading, Template};
use cuboid_validate::Target;

use crate::editor::Editor;

/// The movement snap of a Minecraft document: 1/16.
const MINECRAFT_SNAP: f32 = 1.0 / 16.0;

/// Where a document gets its rules from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Cubik's rules, lighting and units.
    Cubik,
    /// Minecraft Java rules of one game version.
    Minecraft {
        /// The game version.
        version: McVersion,
    },
}

/// The game versions with a known pack format, newest first.
fn versions() -> &'static [McVersion] {
    static LIST: OnceLock<Vec<McVersion>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut list: Vec<McVersion> = mc_versions()
            .pack_formats
            .iter()
            .flat_map(|entry| [entry.from, entry.to])
            .collect();
        list.sort_unstable_by(|a, b| b.cmp(a));
        list.dedup();
        list
    })
}

/// The same versions as text, for the UI.
fn version_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| versions().iter().map(ToString::to_string).collect())
}

impl Preset {
    /// The Minecraft preset for the newest version with a known pack format.
    pub fn minecraft() -> Preset {
        let version = versions()
            .first()
            .copied()
            .unwrap_or(McVersion::new(1, 21, 11));
        Preset::Minecraft { version }
    }

    fn kind(self) -> PresetKind {
        match self {
            Preset::Cubik => PresetKind::Cubik,
            Preset::Minecraft { .. } => PresetKind::Minecraft,
        }
    }

    /// The rotation restriction the preset asks for; `None` keeps what the document says (Cubik
    /// rules). Minecraft follows the version: one axis up to the tiers with free rotations. A
    /// version the table does not know gets the strictest rule.
    fn restrict_rotations(self) -> Option<bool> {
        let Preset::Minecraft { version } = self else {
            return None;
        };
        Some(restrict_rotations(version))
    }

    fn allow_negative_size(self) -> bool {
        matches!(self, Preset::Minecraft { .. })
    }

    fn shading(self) -> ShadingMode {
        match self {
            Preset::Cubik => ShadingMode::Cubik,
            Preset::Minecraft { .. } => ShadingMode::MinecraftBlock,
        }
    }

    fn export_note(self) -> &'static str {
        match self {
            Preset::Cubik => "Saves .cubik files that Cubik opens.",
            Preset::Minecraft { .. } => {
                "Saves .cubik; exports Minecraft JSON or a resource pack in one click."
            }
        }
    }
}

/// The settings of the open document: the preset and what the user changed on top of it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSettings {
    pub preset: Preset,
    /// The document's own choice for negative sizes, when it differs from the preset.
    negative_size: Option<bool>,
    pub shading: ShadingMode,
    pub smooth_light: bool,
    pub voxel_ao: bool,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self::new(Preset::Cubik)
    }
}

impl ProjectSettings {
    /// The defaults of a preset.
    pub fn new(preset: Preset) -> Self {
        Self {
            preset,
            negative_size: None,
            shading: preset.shading(),
            smooth_light: false,
            voxel_ao: true,
        }
    }

    /// The preset of a file that was just read: `.bbmodel` and Minecraft JSON take Minecraft,
    /// `.cubik` takes what its `cuboid.json` names, everything else Cubik.
    pub fn for_file(extension: &str, meta: Option<&ProjectMeta>) -> Self {
        match extension {
            "bbmodel" | "json" => Self::new(Preset::minecraft()),
            "cubik" => meta.map_or_else(Self::default, Self::from_meta),
            _ => Self::default(),
        }
    }

    fn from_meta(meta: &ProjectMeta) -> Self {
        let preset = if meta.preset == "minecraft" {
            let version = meta.version.as_deref().and_then(McVersion::parse);
            version.map_or_else(Preset::minecraft, |version| Preset::Minecraft { version })
        } else {
            Preset::Cubik
        };
        Self {
            negative_size: meta
                .allow_negative_size
                .filter(|on| *on != preset.allow_negative_size()),
            ..Self::new(preset)
        }
    }

    /// What goes into the `cuboid.json` entry; `None` when the document is a plain Cubik one.
    pub fn meta(&self) -> Option<ProjectMeta> {
        if self.preset == Preset::Cubik && self.negative_size.is_none() {
            return None;
        }
        let (preset, version) = match self.preset {
            Preset::Cubik => ("cubik", None),
            Preset::Minecraft { version } => ("minecraft", Some(version.to_string())),
        };
        Some(ProjectMeta {
            preset: preset.to_owned(),
            version,
            allow_negative_size: self.negative_size,
        })
    }

    /// The settings of a new model made from a template, and the document brought to them: the
    /// rotation rule, the model kind and, for the Minecraft templates, the display settings the
    /// brief asks for. Block and Item take Minecraft, with the game version of the brief when it
    /// has one; the others take Cubik.
    pub fn for_template(template: Template, doc: &mut Document, defaults: &BriefDefaults) -> Self {
        let settings = match template {
            Template::Block | Template::Item => Self::new(match defaults.version {
                Some(version) => Preset::Minecraft { version },
                None => Preset::minecraft(),
            }),
            Template::CubikClassic | Template::Voxel { .. } | Template::Empty => Self::default(),
        };
        if let Preset::Minecraft { version } = settings.preset {
            let item = template == Template::Item;
            minecraft_template(doc, version, item, defaults.display);
        }
        settings
    }

    /// Whether sizes may cross zero.
    pub fn allow_negative_size(&self) -> bool {
        self.negative_size
            .unwrap_or_else(|| self.preset.allow_negative_size())
    }

    /// Puts the editing rules of the preset into the environment of the core.
    pub fn apply_env(&self, env: &mut EditEnv) {
        env.allow_negative_size = self.allow_negative_size();
    }

    /// The movement snap the preset asks for, if it asks for one.
    pub fn snap_grid(&self) -> Option<f32> {
        matches!(self.preset, Preset::Minecraft { .. }).then_some(MINECRAFT_SNAP)
    }

    /// The renderer settings with this document's lighting. The cutout follows the shading mode.
    pub fn render(&self, base: RenderSettings) -> RenderSettings {
        RenderSettings {
            shading: self.shading,
            smooth_block_light: self.smooth_light,
            voxel_ao: self.voxel_ao,
            ..base
        }
    }

    /// What the validator checks the document for: a Cubik file, or the Java block or item of
    /// the game version (the model kind of the document decides which).
    pub fn validation_target(&self, doc: &Document) -> Target {
        match self.preset {
            Preset::Cubik => Target::Cubik,
            Preset::Minecraft { version } if doc.props.minecraft.kind == ModelKind::Item => {
                Target::JavaItem { version }
            }
            Preset::Minecraft { version } => Target::JavaBlock { version },
        }
    }

    /// The description the UI shows for `doc`.
    pub fn view(&self, doc: &Document) -> ProjectView {
        let version = match self.preset {
            Preset::Cubik => String::new(),
            Preset::Minecraft { version } => version.to_string(),
        };
        ProjectView {
            preset: self.preset.kind(),
            version,
            versions: version_names(),
            restrict_rotations: doc.props.general.restrict_rotations,
            allow_negative_size: self.allow_negative_size(),
            shading: match self.shading {
                ShadingMode::Cubik => Shading::Cubik,
                ShadingMode::MinecraftBlock => Shading::MinecraftBlock,
                ShadingMode::MinecraftItem => Shading::MinecraftItem,
                ShadingMode::Flat => Shading::Flat,
            },
            smooth_light: self.smooth_light,
            voxel_ao: self.voxel_ao,
            validation: match self.validation_target(doc) {
                Target::JavaBlock { version } => format!("Java block {version}"),
                Target::JavaItem { version } => format!("Java item {version}"),
                _ => "a Cubik file".to_owned(),
            },
            export_note: self.preset.export_note(),
        }
    }
}

/// What the brief of a project gives a new model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BriefDefaults {
    /// The game version; `None` when the brief has none or it is not a version.
    pub version: Option<McVersion>,
    /// The parent model whose display settings a Minecraft model starts from.
    pub display: Option<DisplayPreset>,
}

impl BriefDefaults {
    /// The defaults of a brief.
    pub fn of(brief: &Brief) -> Self {
        Self {
            version: McVersion::parse(&brief.mc_version),
            display: brief.display_preset,
        }
    }
}

impl Editor {
    /// Brings the fresh document of File > New to the preset of a template.
    pub fn use_template(&mut self, template: Template, defaults: &BriefDefaults) {
        let mut doc = self.new_document();
        let project = ProjectSettings::for_template(template, &mut doc, defaults);
        self.replace_with(doc, None, false, project);
    }

    /// Applies a change of the project settings. Text comes back when the change could not be
    /// made fully.
    pub fn change_project(&mut self, change: ProjectChange) -> Option<String> {
        let mut refused = None;
        match change {
            ProjectChange::SetPreset(kind) => {
                let keep = match self.project.preset {
                    preset @ Preset::Minecraft { .. } => preset,
                    Preset::Cubik => Preset::minecraft(),
                };
                let preset = match kind {
                    PresetKind::Cubik => Preset::Cubik,
                    PresetKind::Minecraft => keep,
                };
                refused = self.use_preset(preset);
            }
            ProjectChange::SetVersion(text) => {
                if let Some(version) = McVersion::parse(&text) {
                    refused = self.use_preset(Preset::Minecraft { version });
                }
            }
            ProjectChange::SetRestrictRotations(on) => refused = self.set_restrict_rotations(on),
            ProjectChange::SetAllowNegativeSize(on) => {
                let project = &mut self.project;
                project.negative_size = (on != project.preset.allow_negative_size()).then_some(on);
                project.apply_env(self.core.env_mut());
            }
            ProjectChange::SetShading(mode) => {
                self.project.shading = match mode {
                    Shading::Cubik => ShadingMode::Cubik,
                    Shading::MinecraftBlock => ShadingMode::MinecraftBlock,
                    Shading::MinecraftItem => ShadingMode::MinecraftItem,
                    Shading::Flat => ShadingMode::Flat,
                };
            }
            ProjectChange::SetSmoothLight(on) => self.project.smooth_light = on,
            ProjectChange::SetVoxelAo(on) => self.project.voxel_ao = on,
        }
        refused
    }

    /// Takes the defaults of `preset`, including the rotation rule of its version.
    fn use_preset(&mut self, preset: Preset) -> Option<String> {
        self.project = ProjectSettings::new(preset);
        self.project.apply_env(self.core.env_mut());
        let restrict = preset.restrict_rotations()?;
        self.set_restrict_rotations(restrict)
    }

    /// Sets "Restrict rotations" of the document. Turning it on is refused while a rotation is
    /// off the grid, because the model would not hold its own rules.
    fn set_restrict_rotations(&mut self, on: bool) -> Option<String> {
        let doc = self.core.doc();
        if doc.props.general.restrict_rotations == on {
            return None;
        }
        let base = doc.revision;
        let mut next = doc.clone();
        Arc::make_mut(&mut next.props).general.restrict_rotations = on;
        if on && !(fits_the_rules(&next) && check_invariants(&next, self.core.selection()).is_ok())
        {
            return Some(
                "Some rotations are off the grid, so rotations stay unrestricted.".to_owned(),
            );
        }
        match self
            .core
            .swap_document(next, base, cuboid_core::SwapKind::Import)
        {
            Ok(()) => None,
            Err(_) => Some("The model changed meanwhile; try again.".to_owned()),
        }
    }
}

/// Every rotation follows the Minecraft rule (one axis, 22.5 degree steps; meshes on the 5 degree
/// grid), as the importers require before they switch "Restrict rotations" on.
fn fits_the_rules(doc: &Document) -> bool {
    let on_five = |e: glam::Vec3| e.to_array().iter().all(|c| (c / 5.0).fract() == 0.0);
    doc.elements
        .items()
        .iter()
        .all(|el| mc::rotation_fits_json(el.base.e))
        && doc.meshes.items().iter().all(|mesh| on_five(mesh.base.e))
}

/// How an item is lit in a display context: the inventory takes the model's `gui_light`, every
/// other context (hand, ground, frame, head) the lights of the world.
pub fn display_light(gui: bool, gui_light: GuiLight) -> ItemLight {
    match (gui, gui_light) {
        (false, _) => ItemLight::Level,
        (true, GuiLight::Side) => ItemLight::GuiSide,
        (true, GuiLight::Front) => ItemLight::GuiFront,
    }
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind};

    use super::*;
    use crate::editor::empty_document;

    fn editor() -> Editor {
        Editor::new(1252)
    }

    #[test]
    fn minecraft_defaults_to_the_newest_version_with_a_known_pack_format() {
        let newest = mc_versions()
            .pack_formats
            .iter()
            .map(|entry| entry.to)
            .max()
            .expect("a table");
        assert_eq!(Preset::minecraft(), Preset::Minecraft { version: newest });
        let names = version_names();
        assert_eq!(
            names.first().map(String::as_str),
            Some(newest.to_string().as_str())
        );
        assert!(names.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(names.iter().any(|name| name == "1.21.4"));
    }

    #[test]
    fn a_preset_sets_the_defaults_of_the_table() {
        let cubik = ProjectSettings::new(Preset::Cubik);
        let minecraft = ProjectSettings::new(Preset::minecraft());
        assert_eq!(cubik.shading, ShadingMode::Cubik);
        assert_eq!(minecraft.shading, ShadingMode::MinecraftBlock);
        assert!(!cubik.allow_negative_size() && minecraft.allow_negative_size());
        assert_eq!(cubik.snap_grid(), None);
        assert_eq!(minecraft.snap_grid(), Some(1.0 / 16.0));
        let mut env = editor().core.env_mut().clone();
        minecraft.apply_env(&mut env);
        assert!(env.allow_negative_size);
        cubik.apply_env(&mut env);
        assert!(!env.allow_negative_size);
        // The cutout follows the mode.
        let base = RenderSettings::default();
        assert!((cubik.render(base).shading.cutout() - 0.5).abs() < f32::EPSILON);
        assert!((minecraft.render(base).shading.cutout() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn the_rotation_rule_follows_the_version() {
        let rule = |text: &str| {
            Preset::Minecraft {
                version: McVersion::parse(text).expect("version"),
            }
            .restrict_rotations()
        };
        assert_eq!(Preset::Cubik.restrict_rotations(), None);
        assert_eq!(rule("1.20.4"), Some(true));
        assert_eq!(rule("1.21.7"), Some(true));
        assert_eq!(rule("1.21.11"), Some(false));
        assert_eq!(rule("26.3"), Some(false));
        // A version the table does not know gets the strictest rule.
        assert_eq!(rule("0.5"), Some(true));
    }

    #[test]
    fn the_open_rules_pick_the_preset() {
        let minecraft = ProjectMeta {
            preset: "minecraft".to_owned(),
            version: Some("1.21.4".to_owned()),
            allow_negative_size: None,
        };
        let from = |ext: &str, meta: Option<&ProjectMeta>| ProjectSettings::for_file(ext, meta);
        assert_eq!(from("cubik", None).preset, Preset::Cubik);
        assert_eq!(from("cub", None).preset, Preset::Cubik);
        assert_eq!(from("vox", None).preset, Preset::Cubik);
        assert_eq!(from("bbmodel", None).preset, Preset::minecraft());
        assert_eq!(from("json", None).preset, Preset::minecraft());
        assert_eq!(
            from("cubik", Some(&minecraft)).preset,
            Preset::Minecraft {
                version: McVersion::new(1, 21, 4)
            }
        );
        // Only a `.cubik` has the entry; it can also name Cubik, and a bad version falls back.
        assert_eq!(from("cub", Some(&minecraft)).preset, Preset::Cubik);
        let cubik = ProjectMeta {
            preset: "cubik".to_owned(),
            version: None,
            allow_negative_size: Some(true),
        };
        let kept = from("cubik", Some(&cubik));
        assert_eq!(kept.preset, Preset::Cubik);
        assert!(kept.allow_negative_size());
        let broken = ProjectMeta {
            version: Some("soon".to_owned()),
            ..minecraft
        };
        assert_eq!(from("cubik", Some(&broken)).preset, Preset::minecraft());
    }

    #[test]
    fn the_templates_take_their_preset() {
        let none = BriefDefaults::default();
        let mut doc = empty_document();
        let block = ProjectSettings::for_template(Template::Block, &mut doc, &none);
        assert_eq!(block.preset, Preset::minecraft());
        assert_eq!(doc.props.minecraft.kind, ModelKind::Block);
        assert!(!doc.props.minecraft.include_display);

        let mut doc = empty_document();
        let item = ProjectSettings::for_template(Template::Item, &mut doc, &none);
        assert_eq!(item.preset, Preset::minecraft());
        assert_eq!(doc.props.minecraft.kind, ModelKind::Item);
        assert!(matches!(
            item.validation_target(&doc),
            Target::JavaItem { .. }
        ));

        for template in [
            Template::CubikClassic,
            Template::Voxel { resolution: 16 },
            Template::Empty,
        ] {
            let mut doc = empty_document();
            let settings = ProjectSettings::for_template(template, &mut doc, &none);
            assert_eq!(settings.preset, Preset::Cubik, "{template:?}");
            assert_eq!(settings.validation_target(&doc), Target::Cubik);
        }
    }

    #[test]
    fn the_brief_gives_a_new_minecraft_model_its_version_and_display_settings() {
        let brief = Brief {
            mc_version: "1.20.1".to_owned(),
            display_preset: Some(DisplayPreset::ItemHandheld),
            ..Brief::default()
        };
        let defaults = BriefDefaults::of(&brief);
        let version = McVersion::new(1, 20, 1);
        assert_eq!(defaults.version, Some(version));
        let mut doc = empty_document();
        let item = ProjectSettings::for_template(Template::Item, &mut doc, &defaults);
        assert_eq!(item.preset, Preset::Minecraft { version });
        assert!(doc.props.minecraft.include_display);
        let held_slot = &doc.display[cuboid_model::DisplaySlotId::ThirdRight];
        assert!(held_slot.enabled);
        assert_eq!(held_slot.preset, "item/handheld");
        let held = cuboid_fmt_minecraft::builtin_display("item/handheld").expect("built in");
        let values = |slots: &cuboid_model::DisplaySlots| {
            slots
                .slots
                .iter()
                .map(|s| (s.enabled, s.rotation, s.translation, s.scale))
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&doc.display), values(&held));

        // A model the brief does not concern keeps the Cubik defaults.
        let mut doc = empty_document();
        let classic = ProjectSettings::for_template(Template::CubikClassic, &mut doc, &defaults);
        assert_eq!(classic.preset, Preset::Cubik);
        assert!(!doc.props.minecraft.include_display);
        assert!(!doc.display[cuboid_model::DisplaySlotId::ThirdRight].enabled);

        // Without a version or a preset the newest version and the empty display stay.
        let nothing = BriefDefaults::of(&Brief::default());
        assert_eq!(nothing, BriefDefaults::default());
        let mut doc = empty_document();
        let block = ProjectSettings::for_template(Template::Block, &mut doc, &nothing);
        assert_eq!(block.preset, Preset::minecraft());
    }

    #[test]
    fn the_validation_target_follows_the_preset_and_the_model_kind() {
        let doc = empty_document();
        let version = McVersion::new(1, 21, 4);
        let minecraft = ProjectSettings::new(Preset::Minecraft { version });
        assert_eq!(
            minecraft.validation_target(&doc),
            Target::JavaBlock { version }
        );
        assert_eq!(
            ProjectSettings::default().validation_target(&doc),
            Target::Cubik
        );
        assert_eq!(minecraft.view(&doc).validation, "Java block 1.21.4");
        assert_eq!(
            ProjectSettings::default().view(&doc).validation,
            "a Cubik file"
        );
    }

    #[test]
    fn the_meta_round_trips_and_a_plain_cubik_document_has_none() {
        assert_eq!(ProjectSettings::default().meta(), None);
        let mut settings = ProjectSettings::new(Preset::minecraft());
        settings.negative_size = Some(false);
        let meta = settings.meta().expect("a meta");
        assert_eq!(meta.preset, "minecraft");
        let back = ProjectSettings::for_file("cubik", Some(&meta));
        assert_eq!(back.preset, settings.preset);
        assert!(!back.allow_negative_size());
        // The override is only kept when it differs from the preset.
        let same = ProjectMeta {
            allow_negative_size: Some(true),
            ..meta
        };
        assert_eq!(
            ProjectSettings::for_file("cubik", Some(&same)).negative_size,
            None
        );
    }

    #[test]
    fn changing_the_preset_resets_the_overrides_and_sets_the_rules() {
        let mut editor = editor();
        assert_eq!(
            editor.change_project(ProjectChange::SetPreset(PresetKind::Minecraft)),
            None
        );
        assert_eq!(editor.project.preset, Preset::minecraft());
        assert!(editor.core.env_mut().allow_negative_size);
        assert_eq!(
            editor.change_project(ProjectChange::SetAllowNegativeSize(false)),
            None
        );
        assert!(!editor.core.env_mut().allow_negative_size);
        assert_eq!(editor.project.negative_size, Some(false));

        // An older version has the one-axis rule.
        assert_eq!(
            editor.change_project(ProjectChange::SetVersion("1.20.4".to_owned())),
            None
        );
        assert!(editor.doc().props.general.restrict_rotations);
        assert_eq!(
            editor.project.negative_size, None,
            "a version change takes the defaults"
        );

        editor.change_project(ProjectChange::SetPreset(PresetKind::Cubik));
        assert_eq!(editor.project.preset, Preset::Cubik);
        assert!(!editor.core.env_mut().allow_negative_size);
        // Cubik keeps the document's own rotation flag.
        assert!(editor.doc().props.general.restrict_rotations);
    }

    #[test]
    fn restricting_rotations_is_refused_while_a_rotation_is_off_the_grid() {
        let mut editor = editor();
        assert_eq!(
            editor.change_project(ProjectChange::SetRestrictRotations(false)),
            None
        );
        assert!(!editor.doc().props.general.restrict_rotations);
        let made = editor.core.execute(Command::AddElement(ElementKind::Cube));
        assert!(made.is_done());
        let turned = editor.core.execute(Command::SetRotation {
            targets: vec![],
            value: [Some(10.0), Some(10.0), None],
        });
        assert!(turned.is_done(), "{turned:?}");
        let refusal = editor.change_project(ProjectChange::SetRestrictRotations(true));
        assert!(refusal.is_some());
        assert!(!editor.doc().props.general.restrict_rotations);
        // On the grid it is allowed.
        assert!(
            editor
                .core
                .execute(Command::ResetRotation { targets: vec![] })
                .is_done()
        );
        assert_eq!(
            editor.change_project(ProjectChange::SetRestrictRotations(true)),
            None
        );
        assert!(editor.doc().props.general.restrict_rotations);
    }

    #[test]
    fn the_shading_menu_changes_the_render_settings() {
        let mut editor = editor();
        let base = RenderSettings::default();
        for (choice, mode) in [
            (Shading::Cubik, ShadingMode::Cubik),
            (Shading::MinecraftBlock, ShadingMode::MinecraftBlock),
            (Shading::MinecraftItem, ShadingMode::MinecraftItem),
            (Shading::Flat, ShadingMode::Flat),
        ] {
            editor.change_project(ProjectChange::SetShading(choice));
            let settings = editor.project.render(base);
            assert_eq!(settings.shading, mode);
            assert!((settings.shading.cutout() - mode.cutout()).abs() < f32::EPSILON);
        }
        editor.change_project(ProjectChange::SetSmoothLight(true));
        editor.change_project(ProjectChange::SetVoxelAo(false));
        let settings = editor.project.render(base);
        assert!(settings.smooth_block_light && !settings.voxel_ao);
        // The other settings stay as the renderer has them.
        assert_eq!(settings.msaa, base.msaa);
        // A preset change brings the lighting of the preset back.
        editor.change_project(ProjectChange::SetPreset(PresetKind::Minecraft));
        let settings = editor.project.render(base);
        assert_eq!(settings.shading, ShadingMode::MinecraftBlock);
        assert!(!settings.smooth_block_light && settings.voxel_ao);
    }

    #[test]
    fn display_contexts_light_the_inventory_from_the_gui_light() {
        assert_eq!(display_light(true, GuiLight::Side), ItemLight::GuiSide);
        assert_eq!(display_light(true, GuiLight::Front), ItemLight::GuiFront);
        assert_eq!(display_light(false, GuiLight::Front), ItemLight::Level);
    }
}

#[cfg(test)]
mod flow_tests {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use cuboid_settings::{Environment, Settings};
    use cuboid_ui::{FontSet, Prefs, UiRequest};

    use super::*;
    use crate::frontend::Frontend;
    use crate::io_thread::IoThread;
    use crate::platform::Dirs;
    use crate::state::{Intent, Level, State};

    fn setup() -> (tempfile::TempDir, State, Frontend) {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::first_run(&Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let state = State::new(
            Dirs::under(dir.path()),
            settings,
            IoThread::spawn().unwrap(),
            1252,
            Instant::now(),
        );
        let front = Frontend::new(
            Prefs::default(),
            FontSet::default(),
            dir.path().join("ui.json"),
        );
        (dir, state, front)
    }

    /// Runs frames until the open or save job is over.
    fn finish(state: &mut State) {
        let start = Instant::now();
        while state.doc_job.is_some() {
            state.step(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn open(state: &mut State, path: &Path) {
        state.dispatch(Intent::Open(Some(path.to_owned())));
        finish(state);
        let errors: Vec<_> = state
            .toasts
            .iter()
            .filter(|toast| toast.level == Level::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    const BBMODEL: &str = r#"{
        "meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
        "name": "Box", "resolution": {"width": 16, "height": 16},
        "elements": [{"name": "a", "from": [0, 0, 0], "to": [8, 8, 8], "type": "cube",
            "origin": [4, 4, 4], "rotation": [0, 22.5, 0],
            "faces": {"north": {"uv": [0, 0, 8, 8], "texture": null}}}],
        "textures": [{"name": "t.png", "id": "0"}]
    }"#;

    const MODEL: &str = r##"{
        "textures": { "all": "block/stone" },
        "elements": [{ "from": [0, 0, 0], "to": [16, 8, 16],
            "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#all" } } }]
    }"##;

    /// Runs frames until the condition holds.
    fn wait_until(state: &mut State, done: impl Fn(&State) -> bool) {
        let start = Instant::now();
        while !done(state) {
            state.step(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn new_documents_in_a_project_take_the_version_and_display_settings_of_its_brief() {
        let (dir, mut state, mut front) = setup();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let model = work.join("model.json");
        std::fs::write(&model, MODEL).unwrap();
        let mut library = cuboid_library::Library::new(
            work.join(cuboid_library::PROJECT_DIR),
            cuboid_library::Scope::Project,
        );
        library.set_brief(Brief {
            mc_version: "1.20.1".to_owned(),
            display_preset: Some(DisplayPreset::ItemHandheld),
            ..Brief::default()
        });
        library.save().unwrap();

        // An import has no folder; saving gives the document one.
        open(&mut state, &model);
        state.dispatch(Intent::SaveAs(Some(work.join("model.cubik"))));
        finish(&mut state);
        wait_until(&mut state, |s| s.library.project_brief().is_some());
        front.apply(&mut state, UiRequest::NewDocument(Template::Item));
        let version = McVersion::new(1, 20, 1);
        assert_eq!(state.editor.project.preset, Preset::Minecraft { version });
        assert!(state.editor.doc().props.minecraft.include_display);
        let held = &state.editor.doc().display[cuboid_model::DisplaySlotId::ThirdRight];
        assert!(held.enabled && held.preset == "item/handheld");
        assert!(!state.editor.is_dirty(), "a template is a starting point");

        // A document with no project library gets the newest version and no display settings.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        open(&mut state, &model);
        state.dispatch(Intent::SaveAs(Some(elsewhere.join("model.cubik"))));
        finish(&mut state);
        wait_until(&mut state, |s| !s.library.is_busy());
        front.apply(&mut state, UiRequest::NewDocument(Template::Item));
        assert_eq!(state.editor.project.preset, Preset::minecraft());
        assert!(!state.editor.doc().props.minecraft.include_display);
    }

    #[test]
    fn a_corrupt_project_index_is_left_alone_when_a_document_next_to_it_opens() {
        let (dir, mut state, _front) = setup();
        let work = dir.path().join("work");
        let root = work.join(cuboid_library::PROJECT_DIR);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(cuboid_library::INDEX_FILE), b"{ not json").unwrap();
        let model = work.join("model.json");
        std::fs::write(&model, MODEL).unwrap();
        open(&mut state, &model);
        state.dispatch(Intent::SaveAs(Some(work.join("model.cubik"))));
        finish(&mut state);
        // The brief is read without changing anything: nothing to find, nothing moved.
        let start = Instant::now();
        while state.library.is_busy() {
            state.step(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            thread::sleep(Duration::from_millis(2));
        }
        assert!(state.library.project_brief().is_none());
        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [cuboid_library::INDEX_FILE]);
    }

    #[test]
    fn new_documents_take_the_preset_of_their_template() {
        let (_dir, mut state, mut front) = setup();
        let new = |front: &mut Frontend, state: &mut State, template| {
            front.apply(state, UiRequest::NewDocument(template));
        };
        new(&mut front, &mut state, Template::Block);
        assert_eq!(state.editor.project.preset, Preset::minecraft());
        assert_eq!(state.editor.doc().elements.len(), 1);
        assert!(!state.editor.is_dirty());
        assert!(matches!(
            state.editor.project.validation_target(state.editor.doc()),
            Target::JavaBlock { .. }
        ));
        assert!(state.editor.core.env_mut().allow_negative_size);

        new(&mut front, &mut state, Template::Item);
        assert_eq!(state.editor.project.preset, Preset::minecraft());
        assert!(matches!(
            state.editor.project.validation_target(state.editor.doc()),
            Target::JavaItem { .. }
        ));

        new(&mut front, &mut state, Template::CubikClassic);
        assert_eq!(state.editor.project.preset, Preset::Cubik);
        assert_eq!(state.editor.doc().elements.len(), 1);
        assert!(!state.editor.core.env_mut().allow_negative_size);

        new(&mut front, &mut state, Template::Voxel { resolution: 16 });
        assert_eq!(state.editor.project.preset, Preset::Cubik);
        new(&mut front, &mut state, Template::Block);
        new(&mut front, &mut state, Template::Empty);
        assert_eq!(state.editor.project.preset, Preset::Cubik);
    }

    #[test]
    fn imports_take_minecraft_and_a_cubik_file_takes_what_it_names() {
        let (dir, mut state, _front) = setup();
        let bbmodel = dir.path().join("box.bbmodel");
        std::fs::write(&bbmodel, BBMODEL).unwrap();
        open(&mut state, &bbmodel);
        assert_eq!(state.editor.project.preset, Preset::minecraft());

        let json = dir.path().join("slab.json");
        std::fs::write(&json, MODEL).unwrap();
        state.editor.project = ProjectSettings::default();
        open(&mut state, &json);
        assert_eq!(state.editor.project.preset, Preset::minecraft());

        let cub = dir.path().join("block.cub");
        let plain = r#"{"textures_path":{},"elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#;
        std::fs::write(&cub, cuboid_fmt_cubik::cub::deobfuscate(plain.as_bytes())).unwrap();
        open(&mut state, &cub);
        assert_eq!(state.editor.project.preset, Preset::Cubik);

        // A Cubik file without the entry is Cubik; the entry brings Minecraft back after a save.
        let plain_cubik = dir.path().join("plain.cubik");
        state.dispatch(Intent::SaveAs(Some(plain_cubik.clone())));
        finish(&mut state);
        open(&mut state, &plain_cubik);
        assert_eq!(state.editor.project.preset, Preset::Cubik);
        let loaded = cuboid_fmt_cubik::load_path(&plain_cubik, &Default::default()).unwrap();
        assert_eq!(loaded.project, None, "a Cubik document writes no entry");

        let version = McVersion::new(1, 21, 4);
        state.editor.project = ProjectSettings::new(Preset::Minecraft { version });
        state
            .editor
            .change_project(ProjectChange::SetAllowNegativeSize(false));
        let minecraft = dir.path().join("pack.cubik");
        state.dispatch(Intent::SaveAs(Some(minecraft.clone())));
        finish(&mut state);
        state.editor.project = ProjectSettings::default();
        open(&mut state, &minecraft);
        assert_eq!(state.editor.project.preset, Preset::Minecraft { version });
        assert!(!state.editor.project.allow_negative_size());
        assert!(!state.editor.core.env_mut().allow_negative_size);
    }

    #[test]
    fn a_copy_for_cubik_carries_no_preset() {
        let (dir, mut state, _front) = setup();
        state.editor.project = ProjectSettings::new(Preset::minecraft());
        let copy = dir.path().join("copy.cubik");
        let input = state.editor.save_input(None);
        input.write_copy(&copy).unwrap();
        let loaded = cuboid_fmt_cubik::load_path(&copy, &Default::default()).unwrap();
        assert_eq!(loaded.project, None);
    }

    #[test]
    fn the_project_requests_change_the_settings_the_renderer_and_the_snap() {
        let (_dir, mut state, mut front) = setup();
        let base = RenderSettings::default();
        front.apply(
            &mut state,
            UiRequest::Project(ProjectChange::SetShading(Shading::Flat)),
        );
        assert_eq!(state.editor.project.render(base).shading, ShadingMode::Flat);

        let mut prefs = front.ui.prefs().clone();
        prefs.snap.grid = 0.5;
        front.ui.set_prefs(prefs);
        front.apply(
            &mut state,
            UiRequest::Project(ProjectChange::SetPreset(PresetKind::Minecraft)),
        );
        assert_eq!(state.editor.project.preset, Preset::minecraft());
        assert_eq!(
            state.editor.project.render(base).shading,
            ShadingMode::MinecraftBlock
        );
        assert!((front.ui.prefs().snap.grid - 1.0 / 16.0).abs() < f32::EPSILON);
        // The view the UI gets describes the same settings.
        let view = state.editor.project.view(state.editor.doc());
        assert_eq!(view.preset, PresetKind::Minecraft);
        assert_eq!(view.label(), format!("Minecraft {}", view.version));
        assert!(view.versions.contains(&view.version));
    }
}
