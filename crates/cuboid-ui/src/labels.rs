//! Display names for registry actions and window commands, for the palette and the menus.
//!
//! The registry stores the original's menu text; the labels here are generated from the action
//! names and the few cryptic ones are written out.

use cuboid_actions::{ACTIONS, ActionId, ActionKind, MENUS, MenuId, RowTarget};

use crate::keymap::{Command, UiCommand};
use crate::request::ViewPreset;

/// Labels that the generated text would make hard to read.
const WRITTEN: &[(ActionId, &str)] = &[
    (ActionId::NewFile, "New model"),
    (ActionId::Exit, "Quit"),
    (ActionId::Open, "Open file"),
    (ActionId::Merge, "Merge file into model"),
    (ActionId::SaveAs, "Save as"),
    (ActionId::DeleteConfirm, "Delete selection (ask first)"),
    (ActionId::SelectName, "Select by name"),
    (ActionId::SelectAllElements, "Select all"),
    (ActionId::SelectNone, "Deselect all"),
    (ActionId::ModelProperties, "Model properties"),
    (ActionId::ModelOrigin, "Set model origin"),
    (ActionId::ModelBlueprint, "Set model blueprint"),
    (ActionId::Next, "Select next object"),
    (ActionId::Previous, "Select previous object"),
    (ActionId::ElementProperties, "Element properties"),
    (ActionId::ElementFaces, "Element faces"),
    (ActionId::ElementUv, "Element UV mapping"),
    (ActionId::Clone, "Duplicate selection"),
    (ActionId::ResetRotation, "Reset origin and rotation"),
    (ActionId::ShowAll, "Show all objects"),
    (ActionId::UnlockAll, "Unlock all objects"),
    (ActionId::ResetBlueprint, "Reset blueprint"),
    (ActionId::ResetCamera, "Reset views"),
    (ActionId::GetBackFarElements, "Bring back far-away elements"),
    (ActionId::ZoomPlus, "Zoom in"),
    (ActionId::ZoomMinus, "Zoom out"),
    (ActionId::LoadBgUp, "Load background image: top view"),
    (ActionId::LoadBgWest, "Load background image: side view"),
    (ActionId::LoadBgSouth, "Load background image: front view"),
    (ActionId::LoadBgAll, "Load background image: all views"),
    (ActionId::ToggleBgSize, "Toggle background size"),
    (ActionId::DeleteBgAll, "Remove background images"),
    (ActionId::ElementTools, "Show model tools"),
    (ActionId::VoxelTools, "Show voxel tools"),
    (ActionId::PaintTools, "Show paint tools"),
    (ActionId::ColorTools, "Show colour tools"),
    (ActionId::ColorPalette, "Show colour palette"),
    (ActionId::ElementsList, "Show outliner"),
    (ActionId::ElementsMaterial, "Show textures"),
    (ActionId::GizmosTools, "Show transform tools"),
    (ActionId::ShareSketchfab, "Share on Sketchfab"),
    (ActionId::BrowseAppFolder, "Open application folder"),
    (ActionId::BrowseDataFolder, "Open data folder"),
    (ActionId::BrowseLastExportFolder, "Open last export folder"),
    (ActionId::MoveManagersOnLeft, "Move side panels to the left"),
    (
        ActionId::MoveManagersOnRight,
        "Move side panels to the right",
    ),
    (ActionId::HideManagers, "Hide the outliner"),
    (ActionId::HideHelpers, "Hide the properties panel"),
    (ActionId::HideAll, "Hide all panels"),
    (ActionId::ProjectSite, "Open the project website"),
    (ActionId::SelectMeshTint, "Set mesh colour"),
    (ActionId::ApplyMeshTint, "Apply mesh colour"),
    (ActionId::StartVoxelization, "Convert mesh to voxels"),
    (ActionId::ElementColor, "Set element colour"),
    (ActionId::ElementColorRandom, "Give element a random colour"),
    (ActionId::TexDefault, "Texture layout: single texture"),
    (ActionId::TexSud, "Texture layout: side, up and down"),
    (ActionId::TexSc, "Texture layout: side and cap"),
    (ActionId::TexFsc, "Texture layout: front, side and cap"),
    (ActionId::TexDunswe, "Texture layout: one per face"),
    (ActionId::TexCustom, "Texture layout: custom"),
    (ActionId::UvSnapEnable, "Enable UV snap"),
    (ActionId::UvSnapDisable, "Disable UV snap"),
    (ActionId::UvSnapMirrorEnable, "Enable UV symmetry"),
    (ActionId::UvSnapMirrorDisable, "Disable UV symmetry"),
    (ActionId::ShadeEnable, "Enable shading"),
    (ActionId::ShadeDisable, "Disable shading"),
    (ActionId::InvertFalse, "Use outer normals"),
    (ActionId::InvertTrue, "Use inner normals"),
    (ActionId::SetAsShortcut, "Pin texture"),
    (ActionId::SetTextureLabel, "Edit texture label"),
    (ActionId::NewMatAuto, "Add missing textures"),
    (ActionId::NewMatOptim, "Remove unused textures"),
    (ActionId::NewMatEmpty, "Delete all textures"),
    (ActionId::NewMatTexture, "Replace textures: single texture"),
    (ActionId::NewMatSud, "Replace textures: side, up and down"),
    (ActionId::NewMatSc, "Replace textures: side and cap"),
    (ActionId::NewMatFsc, "Replace textures: front, side and cap"),
    (ActionId::NewMatDunswe, "Replace textures: one per face"),
    (
        ActionId::CreateAtlasBlank,
        "Create atlas with blank textures",
    ),
    (
        ActionId::CreateAtlasTextures,
        "Create atlas keeping textures",
    ),
    (ActionId::ConvertToElementsCubes, "Convert voxels to cubes"),
    (
        ActionId::ConvertToElements,
        "Convert voxels to elements by colour",
    ),
    (
        ActionId::ConvertToElementsAtlas,
        "Convert voxels to elements by surface",
    ),
    (ActionId::VoxelResetColor, "Colour all voxels"),
    (
        ActionId::VoxelCropOutside,
        "Crop voxels outside the model box",
    ),
    (ActionId::FillBlueprint, "Fill blueprint with voxels"),
    (ActionId::ClearBlueprint, "Clear voxels in blueprint"),
    (ActionId::CopyInBlueprint, "Copy voxels in blueprint"),
    (ActionId::PasteAtBlueprint, "Paste voxels at blueprint"),
    (ActionId::FlipXBlueprint, "Flip blueprint voxels on X"),
    (ActionId::FlipYBlueprint, "Flip blueprint voxels on Y"),
    (ActionId::FlipZBlueprint, "Flip blueprint voxels on Z"),
    (ActionId::DefaultPalette, "Palette: RGB"),
    (ActionId::HslPalette, "Palette: HSL"),
    (ActionId::JmpPalette, "Palette: JMP"),
    (ActionId::Generic16Palette, "Palette: generic 16"),
    (ActionId::Pico8Palette, "Palette: Pico-8"),
    (ActionId::Edg32Palette, "Palette: EDG32"),
    (ActionId::Edg16Palette, "Palette: EDG16"),
    (ActionId::LoadPalette, "Load palette"),
    (ActionId::SavePalette, "Save palette"),
    (ActionId::CreateElementCube, "Add cube"),
    (ActionId::CreateElementPlaneX, "Add plane facing X"),
    (ActionId::CreateElementPlaneY, "Add plane facing Y"),
    (ActionId::CreateElementPlaneZ, "Add plane facing Z"),
    (ActionId::CreateMeshCube, "Add cube primitive"),
    (ActionId::CreateMeshSphere, "Add sphere primitive"),
    (ActionId::CreateMeshCylinder, "Add cylinder primitive"),
    (ActionId::CreateMeshCone, "Add cone primitive"),
    (ActionId::CreateMeshPyramid, "Add pyramid primitive"),
    (ActionId::CreateMeshSlope, "Add slope primitive"),
    (ActionId::CreateMeshCornerIn, "Add inner corner primitive"),
    (ActionId::CreateMeshCornerOut, "Add outer corner primitive"),
    (ActionId::CreateMeshTorus, "Add torus primitive"),
];

/// Words that are acronyms or fixed spellings in generated labels.
const WORDS: &[(&str, &str)] = &[
    ("uv", "UV"),
    ("png", "PNG"),
    ("obj", "OBJ"),
    ("rgb", "RGB"),
    ("hsl", "HSL"),
    ("x", "X"),
    ("y", "Y"),
    ("z", "Z"),
    ("bg", "background"),
    ("tex", "texture"),
    ("mesh", "mesh"),
];

/// The label of a registry action.
pub fn action_label(id: ActionId) -> String {
    if let Some((_, text)) = WRITTEN.iter().find(|(a, _)| *a == id) {
        return (*text).to_owned();
    }
    let name = format!("{id:?}");
    // FacesEnableUp -> "Enable face: up".
    for (prefix, verb) in [("FacesEnable", "Enable"), ("FacesDisable", "Disable")] {
        if let Some(face) = name.strip_prefix(prefix) {
            return format!("{verb} face: {}", humanize(face).to_lowercase());
        }
    }
    humanize(&name)
}

/// Splits `CamelCase` into a sentence: "Select inverse".
pub fn humanize(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut words: Vec<String> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        let starts_word = i == 0
            || (c.is_uppercase()
                && (chars[i - 1].is_lowercase()
                    || chars[i - 1].is_ascii_digit()
                    || chars.get(i + 1).is_some_and(|n| n.is_lowercase())))
            || (c.is_ascii_digit() && !chars[i - 1].is_ascii_digit());
        if starts_word {
            words.push(String::new());
        }
        if let Some(word) = words.last_mut() {
            word.extend(c.to_lowercase());
        }
    }
    let words: Vec<String> = words
        .into_iter()
        .map(|w| {
            WORDS
                .iter()
                .find(|(from, _)| *from == w)
                .map_or(w, |(_, to)| (*to).to_owned())
        })
        .collect();
    let mut text = words.join(" ");
    if let Some(first) = text.chars().next() {
        let upper: String = first.to_uppercase().collect();
        text.replace_range(..first.len_utf8(), &upper);
    }
    text
}

/// The group an action is listed under, from the menu that holds it.
pub fn action_category(id: ActionId) -> &'static str {
    let menu = MENUS.iter().find(|menu| {
        menu.rows
            .iter()
            .any(|row| matches!(row.target, RowTarget::Action(a) if a == id))
    });
    match menu.map(|m| m.id) {
        Some(MenuId::File | MenuId::RecentFiles) => "File",
        Some(MenuId::Edit) => "Edit",
        Some(MenuId::Objects) => "Object",
        Some(MenuId::Recover) => "Recover",
        Some(MenuId::Views) => "View",
        Some(MenuId::Tools) => "Tools",
        Some(MenuId::Window) => "Window",
        Some(MenuId::Help) => "Help",
        Some(
            MenuId::MeshesPopup
            | MenuId::Mesh
            | MenuId::MeshTransform
            | MenuId::MeshTexture
            | MenuId::MeshCopy
            | MenuId::MeshPaste,
        ) => "Mesh",
        Some(
            MenuId::ElementsPopup
            | MenuId::Element
            | MenuId::ElementTransform
            | MenuId::Faces
            | MenuId::FacesEnable
            | MenuId::FacesDisable
            | MenuId::Textures
            | MenuId::UvMapping
            | MenuId::Tint
            | MenuId::Culling
            | MenuId::Render
            | MenuId::ElementCopy
            | MenuId::ElementPaste,
        ) => "Element",
        Some(
            MenuId::TexturePopup
            | MenuId::MaterialPopup
            | MenuId::NewTexturesList
            | MenuId::TextureLabelList,
        ) => "Texture",
        Some(
            MenuId::VoxelPopup
            | MenuId::VoxelConvert
            | MenuId::VoxelTransform
            | MenuId::VoxelBlueprint,
        ) => "Voxel",
        Some(MenuId::PalettePopup) => "Palette",
        _ if format!("{id:?}").starts_with("Create") => "Create",
        _ => "Other",
    }
}

/// The label of any command.
pub fn command_label(command: Command) -> String {
    match command {
        Command::Action(id) => action_label(id),
        Command::Ui(c) => ui_label(c).0,
    }
}

/// Label and group of a window command.
pub fn ui_label(command: UiCommand) -> (String, &'static str) {
    match command {
        UiCommand::Palette => ("Open command palette".into(), "Window"),
        UiCommand::ShowStart => ("Show start screen".into(), "File"),
        UiCommand::Preferences => ("Open preferences".into(), "Window"),
        UiCommand::KeyboardShortcuts => ("Edit keyboard shortcuts".into(), "Window"),
        UiCommand::TogglePanel(panel) => (format!("Toggle {} panel", panel.title()), "Window"),
        UiCommand::ResetLayout => ("Reset panel layout".into(), "Window"),
        UiCommand::SetViewLayout(layout) => (layout.name().into(), "View"),
        UiCommand::SetMode(mode) => (format!("Switch to {} mode", mode.name()), "Mode"),
        UiCommand::NextMode => ("Switch to next mode".into(), "Mode"),
        UiCommand::SetTool(tool) => (tool.name().into(), "Tool"),
        UiCommand::View(preset) => (preset_label(preset).into(), "View"),
        UiCommand::FrameSelected => ("Frame selection".into(), "View"),
        UiCommand::ToggleSnap => ("Toggle snapping".into(), "Tool"),
        UiCommand::GroupSelection => ("Group selection".into(), "Object"),
        UiCommand::Ungroup => ("Ungroup".into(), "Object"),
        UiCommand::ToggleTheme => ("Toggle dark and light theme".into(), "Window"),
    }
}

fn preset_label(preset: ViewPreset) -> &'static str {
    match preset {
        ViewPreset::Front => "View from front",
        ViewPreset::Back => "View from back",
        ViewPreset::Right => "View from right",
        ViewPreset::Left => "View from left",
        ViewPreset::Top => "View from top",
        ViewPreset::Bottom => "View from bottom",
        ViewPreset::ToggleProjection => "Toggle perspective and orthographic",
    }
}

/// One line of the command palette.
#[derive(Clone, Debug)]
pub struct Entry {
    /// What runs.
    pub command: Command,
    /// Name.
    pub label: String,
    /// Group.
    pub category: &'static str,
    /// Lower-case text the search looks through.
    haystack: String,
}

impl Entry {
    fn new(command: Command, label: String, category: &'static str) -> Self {
        let haystack = format!("{label} {category}").to_lowercase();
        Self {
            command,
            label,
            category,
            haystack,
        }
    }
}

/// Every command: all live registry actions and all window commands.
pub fn entries() -> &'static [Entry] {
    static ENTRIES: std::sync::OnceLock<Vec<Entry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        let actions = ACTIONS
            .iter()
            .filter(|def| def.kind == ActionKind::Live)
            .map(|def| {
                Entry::new(
                    Command::Action(def.id),
                    action_label(def.id),
                    action_category(def.id),
                )
            });
        let windows = crate::keymap::all_ui_commands().into_iter().map(|c| {
            let (label, category) = ui_label(c);
            Entry::new(Command::Ui(c), label, category)
        });
        actions.chain(windows).collect()
    })
}

/// Score of one query word in `text`, higher is better. The word must appear as a subsequence;
/// matches at the start of a word and consecutive letters score more.
fn word_score(word: &[char], text: &str) -> Option<i32> {
    let mut score = 0;
    let mut qi = 0;
    let mut prev_matched = false;
    let mut prev: Option<char> = None;
    let mut first_hit: Option<usize> = None;
    for (i, c) in text.chars().enumerate() {
        let at_word_start = prev.is_none_or(|p| !p.is_alphanumeric());
        if qi < word.len() && c == word[qi] {
            score += 10;
            if at_word_start {
                score += 15;
            }
            if prev_matched {
                score += 10;
            }
            first_hit.get_or_insert(i);
            qi += 1;
            prev_matched = true;
        } else {
            prev_matched = false;
        }
        prev = Some(c);
    }
    if qi < word.len() {
        return None;
    }
    let lead = i32::try_from(first_hit.unwrap_or(0))
        .unwrap_or(i32::MAX)
        .min(20);
    Some(score - lead)
}

/// Indices into `entries` that match `query`, best first. Every word of the query must match;
/// an empty query matches everything in the given order. `recent` command ids rank first for an
/// empty query and get a small bonus otherwise.
pub fn search(entries: &[Entry], query: &str, recent: &[String]) -> Vec<usize> {
    let words: Vec<Vec<char>> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.chars().collect())
        .collect();
    let recent_rank = |entry: &Entry| recent.iter().position(|id| *id == entry.command.id());
    if words.is_empty() {
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|&i| recent_rank(&entries[i]).unwrap_or(usize::MAX));
        return order;
    }
    let mut scored: Vec<(i32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let mut total = 0;
            for word in &words {
                total += word_score(word, &entry.haystack)?;
            }
            let joined: String = words
                .iter()
                .map(|w| w.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join(" ");
            let label = entry.label.to_lowercase();
            if label.starts_with(&joined) {
                total += 60;
            } else if label.contains(&joined) {
                total += 30;
            }
            if recent_rank(entry).is_some() {
                total += 8;
            }
            let length_penalty = i32::try_from(entry.label.len() / 4).unwrap_or(0);
            Some((total - length_penalty, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(query: &str) -> Vec<String> {
        let all = entries();
        search(all, query, &[])
            .into_iter()
            .map(|i| all[i].label.clone())
            .collect()
    }

    #[test]
    fn humanize_splits_words_and_keeps_acronyms() {
        assert_eq!(humanize("SelectInverse"), "Select inverse");
        assert_eq!(humanize("UvSnapEnable"), "UV snap enable");
        assert_eq!(humanize("Edg32Palette"), "Edg 32 palette");
        assert_eq!(humanize("Undo"), "Undo");
        assert_eq!(humanize("LoadBgUp"), "Load background up");
    }

    #[test]
    fn every_live_action_has_an_entry_with_a_nonempty_label() {
        let live = ACTIONS
            .iter()
            .filter(|d| d.kind == ActionKind::Live)
            .count();
        let ui = crate::keymap::all_ui_commands().len();
        assert_eq!(entries().len(), live + ui);
        assert!(entries().iter().all(|e| !e.label.trim().is_empty()));
    }

    #[test]
    fn dead_actions_are_left_out() {
        assert!(
            !entries()
                .iter()
                .any(|e| e.command == Command::Action(ActionId::UvApplyDefault))
        );
        assert!(
            !entries()
                .iter()
                .any(|e| e.command == Command::Action(ActionId::PasteTint))
        );
    }

    #[test]
    fn the_exact_start_of_a_label_ranks_first() {
        let result = labels("undo");
        assert_eq!(result.first().map(String::as_str), Some("Undo"));
        let result = labels("save");
        assert_eq!(result.first().map(String::as_str), Some("Save"));
    }

    #[test]
    fn words_may_come_in_any_order_and_match_the_category() {
        let result = labels("all select");
        assert!(result.iter().any(|l| l == "Select all"), "{result:?}");
        let result = labels("view front");
        assert_eq!(result.first().map(String::as_str), Some("View from front"));
    }

    #[test]
    fn abbreviations_match_as_subsequences() {
        let result = labels("prefs");
        assert!(
            result.iter().take(3).any(|l| l == "Open preferences"),
            "{result:?}"
        );
        let result = labels("tgsnap");
        assert!(result.iter().any(|l| l == "Toggle snapping"), "{result:?}");
    }

    #[test]
    fn nonsense_matches_nothing_and_empty_matches_everything() {
        assert!(labels("qzxjv").is_empty());
        assert_eq!(labels("").len(), entries().len());
        assert_eq!(labels("   ").len(), entries().len());
    }

    #[test]
    fn recent_commands_come_first_for_an_empty_query() {
        let all = entries();
        let wanted = Command::Action(ActionId::Export).id();
        let order = search(all, "", std::slice::from_ref(&wanted));
        assert_eq!(all[order[0]].command.id(), wanted);
    }

    #[test]
    fn categories_come_from_the_menus() {
        assert_eq!(action_category(ActionId::Save), "File");
        assert_eq!(action_category(ActionId::Undo), "Edit");
        assert_eq!(action_category(ActionId::CreateMeshTorus), "Create");
        assert_eq!(action_category(ActionId::VoxelFlipX), "Voxel");
    }
}
