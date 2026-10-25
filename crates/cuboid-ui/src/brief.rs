//! The project brief of the start screen: a read-only card and the dialog that edits it.

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    Button, Icon, Modal, ModalButton, PropertyGrid, Section, Type, color_swatch, dropdown,
    rgb_color, segmented, text, text_area, text_field, toggle,
};
use egui::{Context, Label, Ui, Vec2, vec2};

use crate::dialogs::fixed_body;
use crate::export_dialog::VersionChoice;
use crate::library::{BriefCardView, BriefInputs, LibraryRequest, split_tags};
use crate::request::DisplayPreset;

/// Side of a palette swatch.
const SWATCH: f32 = 16.0;
/// Most palette colours the card lists.
const SWATCHES: usize = 12;
/// Most palette colours the dialog previews next to its button.
const PREVIEW_SWATCHES: usize = 10;
/// The choice that leaves a setting unset.
const NOT_SET: &str = "Not set";
/// Characters of the goal the card shows.
const GOAL_ON_CARD: usize = 160;
/// Colours that "Use observed values" copies.
const OBSERVED_COLORS: usize = 16;
/// Height of the dialog body.
const BODY: f32 = height::ROW * 15.0;
/// Rows of the goal and the rules boxes.
const GOAL_ROWS: usize = 3;
const RULES_ROWS: usize = 5;

/// Where the models go: the id the library uses and the name the UI shows.
const PLATFORMS: [(&str, &str); 6] = [
    ("vanilla", "Vanilla"),
    ("model_engine", "ModelEngine"),
    ("items_adder", "ItemsAdder"),
    ("oraxen", "Oraxen"),
    ("geckolib", "GeckoLib"),
    ("other", "Other"),
];
/// How names are cased: the id the library uses and the name the UI shows.
const CASES: [(&str, &str); 3] = [
    ("any", "Any"),
    ("snake", "snake_case"),
    ("kebab", "kebab-case"),
];
/// The texture sides a brief can allow.
const TEXTURE_SIZES: [u32; 6] = [16, 32, 64, 128, 256, 512];

fn platform_name(id: &str) -> &'static str {
    PLATFORMS
        .iter()
        .find(|(known, _)| *known == id)
        .map_or("Other", |(_, name)| name)
}

/// `#rrggbb`.
fn hex(rgb: &[u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// The colours a text lists as `#rrggbb` codes, separated by spaces or commas; other words are
/// skipped.
fn parse_colors(text: &str) -> Vec<[u8; 3]> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|word| {
            let digits = word.strip_prefix('#').unwrap_or(word);
            if digits.len() != 6 {
                return None;
            }
            let [_, r, g, b] = u32::from_str_radix(digits, 16).ok()?.to_be_bytes();
            Some([r, g, b])
        })
        .collect()
}

/// The text under the project name: the game version and the style words.
pub(crate) fn summary(brief: &BriefCardView) -> String {
    let inputs = &brief.inputs;
    let mut parts = Vec::new();
    if !inputs.mc_version.is_empty() {
        parts.push(format!("Java {}", inputs.mc_version));
    }
    if !inputs.style_tags.is_empty() {
        parts.push(inputs.style_tags.join(", "));
    }
    if parts.is_empty() {
        "No version or style set".to_owned()
    } else {
        parts.join(" \u{b7} ")
    }
}

/// The platform and the namespace.
fn platform_line(inputs: &BriefInputs) -> String {
    let platform = platform_name(&inputs.platform);
    if inputs.namespace.is_empty() {
        platform.to_owned()
    } else {
        format!("{platform} \u{b7} {}", inputs.namespace)
    }
}

/// The targets that are set, or `None` when none is.
fn targets_line(inputs: &BriefInputs) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(density) = inputs.texel_density {
        parts.push(format!("{density} texels per block"));
    }
    if let Some(size) = inputs.texture_size {
        parts.push(format!("textures up to {size} px"));
    }
    if !inputs.palette.is_empty() {
        let strict = if inputs.palette_strict {
            ", strict"
        } else {
            ""
        };
        parts.push(format!("{} palette colours{strict}", inputs.palette.len()));
    }
    if let Some(budget) = inputs.max_elements {
        parts.push(format!("up to {budget} elements"));
    }
    if let Some((_, case)) = CASES
        .iter()
        .find(|(id, _)| *id == inputs.name_case && *id != "any")
    {
        parts.push(format!("{case} names"));
    }
    if !inputs.name_prefix.is_empty() {
        parts.push(format!("prefix {}", inputs.name_prefix));
    }
    if let Some(preset) = inputs.display_preset {
        parts.push(format!("{} display", preset.id()));
    }
    (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
}

/// The goal cut to what the card shows.
fn goal_on_card(goal: &str) -> String {
    let flat = goal.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= GOAL_ON_CARD {
        return flat;
    }
    let cut: String = flat.chars().take(GOAL_ON_CARD).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The text under the palette: the texel density and the entry counts.
fn counts(brief: &BriefCardView) -> String {
    let density = brief.observed_density.map_or_else(
        || "no texel density yet".to_owned(),
        |d| format!("{d:.0} texels per block"),
    );
    format!(
        "{density} \u{b7} {} models, {} textures",
        brief.models, brief.textures
    )
}

fn caption(ui: &mut Ui, line: impl Into<String>) {
    ui.add(
        Label::new(text(Type::Caption, line))
            .wrap()
            .selectable(false),
    );
}

/// The read-only card. Returns whether Edit was clicked.
pub(crate) fn card(ui: &mut Ui, brief: &BriefCardView, busy: bool) -> bool {
    let inputs = &brief.inputs;
    let mut edit = false;
    ui.horizontal(|ui| {
        ui.label(text(Type::Title, "Project brief"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = Button::new("Edit").ghost().icon(Icon::Pencil);
            edit = ui.add_enabled(!busy, button).clicked();
        });
    });
    ui.add_space(space::XS);
    if inputs.project_name.is_empty() {
        ui.add(Label::new(text(Type::Label, "Untitled project")).selectable(false));
    } else {
        ui.add(
            Label::new(text(Type::Strong, &inputs.project_name))
                .truncate()
                .selectable(false),
        );
    }
    caption(ui, summary(brief));
    if !inputs.goal.is_empty() {
        ui.add_space(space::XS);
        ui.add(
            Label::new(text(Type::Body, goal_on_card(&inputs.goal)))
                .wrap()
                .selectable(false),
        );
    }
    ui.add_space(space::XS);
    caption(ui, platform_line(inputs));
    caption(
        ui,
        targets_line(inputs).unwrap_or_else(|| "No targets set".to_owned()),
    );
    ui.add_space(space::SM);
    // The palette of the brief when it has one, else what the textures share.
    let palette = if inputs.palette.is_empty() {
        &brief.observed_palette
    } else {
        &inputs.palette
    };
    if palette.is_empty() {
        ui.add(Label::new(text(Type::Caption, "No shared palette yet")).selectable(false));
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(space::XS);
            for rgb in palette.iter().take(SWATCHES) {
                color_swatch(ui, rgb_color(*rgb), vec2(SWATCH, SWATCH), false)
                    .on_hover_text(hex(rgb));
            }
        });
    }
    ui.add_space(space::SM);
    caption(ui, counts(brief));
    edit
}

/// The dialog's state: the inputs as the fields hold them, and what the library observed.
pub(crate) struct BriefDialog {
    name: String,
    goal: String,
    /// Index into `PLATFORMS`.
    platform: usize,
    namespace: String,
    /// Index into `versions`; 0 is "Not set".
    version: usize,
    versions: Vec<String>,
    tags: String,
    /// The table is empty: the version is typed.
    typed: String,
    /// The version is chosen from the table.
    from_table: bool,
    density: String,
    /// 0 is "Not set", then `TEXTURE_SIZES`.
    texture_size: usize,
    /// `#rrggbb` codes.
    palette: String,
    strict: bool,
    budget: String,
    /// 0 is "Not set", then `DisplayPreset::ALL`.
    display: usize,
    /// Index into `CASES`.
    case: usize,
    prefix: String,
    rules: String,
    observed_palette: Vec<[u8; 3]>,
    observed_density: Option<f32>,
}

/// How the dialog ended.
pub(crate) enum BriefAnswer {
    /// Send the brief.
    Save(LibraryRequest),
    /// Close without sending.
    Cancel,
}

impl BriefDialog {
    /// Opens the dialog on the current brief. A version that is not in the table stays choosable.
    pub fn open(brief: &BriefCardView, table: &[VersionChoice]) -> Self {
        let inputs = &brief.inputs;
        let mut versions = vec![NOT_SET.to_owned()];
        versions.extend(table.iter().map(|v| v.id.clone()));
        let version = match versions.iter().position(|v| *v == inputs.mc_version) {
            Some(at) => at,
            None if inputs.mc_version.is_empty() || table.is_empty() => 0,
            None => {
                versions.insert(1, inputs.mc_version.clone());
                1
            }
        };
        let colors: Vec<String> = inputs.palette.iter().map(hex).collect();
        Self {
            name: inputs.project_name.clone(),
            goal: inputs.goal.clone(),
            platform: PLATFORMS
                .iter()
                .position(|(id, _)| *id == inputs.platform)
                .unwrap_or(0),
            namespace: inputs.namespace.clone(),
            version,
            versions,
            tags: inputs.style_tags.join(", "),
            typed: inputs.mc_version.clone(),
            from_table: !table.is_empty(),
            density: inputs
                .texel_density
                .map_or_else(String::new, |d| d.to_string()),
            texture_size: inputs
                .texture_size
                .and_then(|size| TEXTURE_SIZES.iter().position(|s| *s == size))
                .map_or(0, |at| at + 1),
            palette: colors.join(" "),
            strict: inputs.palette_strict,
            budget: inputs
                .max_elements
                .map_or_else(String::new, |n| n.to_string()),
            display: inputs
                .display_preset
                .and_then(|preset| DisplayPreset::ALL.iter().position(|p| *p == preset))
                .map_or(0, |at| at + 1),
            case: CASES
                .iter()
                .position(|(id, _)| *id == inputs.name_case)
                .unwrap_or(0),
            prefix: inputs.name_prefix.clone(),
            rules: inputs.rules.join("\n"),
            observed_palette: brief.observed_palette.clone(),
            observed_density: brief.observed_density,
        }
    }

    /// Copies the observed palette (up to 16 colours) and the median texel density into the
    /// targets.
    fn use_observed(&mut self) {
        let colors: Vec<String> = self
            .observed_palette
            .iter()
            .take(OBSERVED_COLORS)
            .map(hex)
            .collect();
        if !colors.is_empty() {
            self.palette = colors.join(" ");
        }
        if let Some(density) = self.observed_density.filter(|d| *d >= 0.5) {
            self.density = format!("{density:.0}");
        }
    }

    /// Draws the dialog. `None` while it is open.
    pub fn show(&mut self, ctx: &Context) -> Option<BriefAnswer> {
        let buttons = [ModalButton::new("Cancel"), ModalButton::primary("Save")];
        let outcome = Modal::new("brief_dialog", "Project brief")
            .width(520.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                fixed_body(ui, "brief_dialog_body", BODY, |ui| self.body(ui));
            });
        let save = outcome.button == Some(1) || outcome.confirmed;
        if save {
            return Some(BriefAnswer::Save(self.request()));
        }
        (outcome.button == Some(0) || outcome.cancelled).then_some(BriefAnswer::Cancel)
    }

    fn body(&mut self, ui: &mut Ui) {
        Section::new("Project").show(ui, "brief_project", |ui| {
            PropertyGrid::new("brief_project").show(ui, |grid| {
                grid.row("Name", |ui| {
                    text_field(ui, "brief_name", &mut self.name, "Name of the project");
                });
                grid.row("Goal", |ui| {
                    let hint = "What the project is for";
                    text_area(ui, "brief_goal", &mut self.goal, hint, GOAL_ROWS);
                });
                grid.row("Platform", |ui| {
                    let names = PLATFORMS.map(|(_, name)| name);
                    dropdown(ui, "brief_platform", &mut self.platform, &names);
                });
                grid.row("Namespace", |ui| {
                    text_field(ui, "brief_namespace", &mut self.namespace, "splatcraft");
                });
                grid.row("Minecraft", |ui| {
                    if self.from_table {
                        let options: Vec<&str> = self.versions.iter().map(String::as_str).collect();
                        dropdown(ui, "brief_version", &mut self.version, &options);
                    } else {
                        text_field(ui, "brief_version_text", &mut self.typed, "1.21.6");
                    }
                });
                grid.row("Style", |ui| {
                    text_field(ui, "brief_tags", &mut self.tags, "pixel, low-poly");
                });
            });
        });
        Section::new("Targets").show(ui, "brief_targets", |ui| {
            PropertyGrid::new("brief_targets").show(ui, |grid| {
                grid.row("Texel density", |ui| {
                    text_field(ui, "brief_density", &mut self.density, "Texels per block");
                });
                grid.row("Texture size", |ui| {
                    let mut options = vec![NOT_SET.to_owned()];
                    options.extend(TEXTURE_SIZES.iter().map(|s| format!("{s} px at most")));
                    let options: Vec<&str> = options.iter().map(String::as_str).collect();
                    dropdown(ui, "brief_texture_size", &mut self.texture_size, &options);
                });
                grid.row("Palette", |ui| {
                    let hint = "#c86432 #3a7bd5";
                    text_field(ui, "brief_palette", &mut self.palette, hint);
                });
                grid.row("", |ui| {
                    let label = "Colours outside the palette are errors";
                    toggle(ui, &mut self.strict, label);
                });
                grid.row("Element budget", |ui| {
                    text_field(ui, "brief_budget", &mut self.budget, "Elements per model");
                });
                grid.row("Display", |ui| {
                    let mut options = vec![NOT_SET];
                    options.extend(DisplayPreset::ALL.map(DisplayPreset::id));
                    dropdown(ui, "brief_display", &mut self.display, &options);
                });
                grid.row("", |ui| {
                    let observed =
                        !self.observed_palette.is_empty() || self.observed_density.is_some();
                    let button = Button::new("Use observed values").ghost();
                    if ui.add_enabled(observed, button).clicked() {
                        self.use_observed();
                    }
                    ui.spacing_mut().item_spacing = Vec2::splat(space::XS);
                    for rgb in parse_colors(&self.palette).iter().take(PREVIEW_SWATCHES) {
                        color_swatch(ui, rgb_color(*rgb), vec2(SWATCH, SWATCH), false)
                            .on_hover_text(hex(rgb));
                    }
                });
            });
        });
        Section::new("Naming").show(ui, "brief_naming", |ui| {
            PropertyGrid::new("brief_naming").show(ui, |grid| {
                grid.row("Case", |ui| {
                    let names = CASES.map(|(_, name)| name);
                    segmented(ui, &mut self.case, &names);
                });
                grid.row("Prefix", |ui| {
                    let hint = "For files and textures, such as ink_";
                    text_field(ui, "brief_prefix", &mut self.prefix, hint);
                });
            });
        });
        Section::new("Rules").show(ui, "brief_rules", |ui| {
            let hint = "One do or don't per line";
            text_area(ui, "brief_rules_text", &mut self.rules, hint, RULES_ROWS);
        });
    }

    fn request(&self) -> LibraryRequest {
        let mc_version = if self.from_table {
            match self.version {
                0 => String::new(),
                at => self.versions[at].clone(),
            }
        } else {
            self.typed.trim().to_owned()
        };
        LibraryRequest::SetBrief(Box::new(BriefInputs {
            project_name: self.name.trim().to_owned(),
            mc_version,
            style_tags: split_tags(&self.tags),
            goal: self.goal.trim().to_owned(),
            platform: PLATFORMS[self.platform.min(PLATFORMS.len() - 1)]
                .0
                .to_owned(),
            namespace: self.namespace.trim().to_owned(),
            texel_density: self.density.trim().parse().ok().filter(|d: &f32| *d > 0.0),
            texture_size: self
                .texture_size
                .checked_sub(1)
                .and_then(|at| TEXTURE_SIZES.get(at).copied()),
            palette: parse_colors(&self.palette),
            palette_strict: self.strict,
            max_elements: self.budget.trim().parse().ok().filter(|n: &u32| *n > 0),
            name_case: CASES[self.case.min(CASES.len() - 1)].0.to_owned(),
            name_prefix: self.prefix.trim().to_owned(),
            display_preset: self
                .display
                .checked_sub(1)
                .and_then(|at| DisplayPreset::ALL.get(at).copied()),
            rules: self
                .rules
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(version: &str) -> BriefCardView {
        BriefCardView {
            inputs: BriefInputs {
                project_name: "Mossy Hollow".to_owned(),
                mc_version: version.to_owned(),
                style_tags: vec!["pixel".to_owned(), "low-poly".to_owned()],
                ..BriefInputs::default()
            },
            ..BriefCardView::default()
        }
    }

    fn table() -> Vec<VersionChoice> {
        ["1.21.11", "1.21.6"]
            .into_iter()
            .map(|id| VersionChoice {
                id: id.to_owned(),
                pack_format: None,
            })
            .collect()
    }

    fn sent(dialog: &BriefDialog) -> BriefInputs {
        let LibraryRequest::SetBrief(inputs) = dialog.request() else {
            panic!("a brief request");
        };
        *inputs
    }

    #[test]
    fn the_summary_names_the_version_and_the_style() {
        assert_eq!(
            summary(&brief("1.21.6")),
            "Java 1.21.6 \u{b7} pixel, low-poly"
        );
        assert_eq!(
            summary(&BriefCardView::default()),
            "No version or style set"
        );
    }

    #[test]
    fn the_card_lists_the_platform_and_the_targets_that_are_set() {
        let mut inputs = BriefInputs::default();
        assert_eq!(platform_line(&inputs), "Vanilla");
        assert_eq!(targets_line(&inputs), None);
        inputs.platform = "items_adder".to_owned();
        inputs.namespace = "splatcraft".to_owned();
        assert_eq!(platform_line(&inputs), "ItemsAdder \u{b7} splatcraft");
        inputs.texel_density = Some(16.0);
        inputs.texture_size = Some(32);
        inputs.palette = vec![[1, 2, 3], [4, 5, 6]];
        inputs.palette_strict = true;
        inputs.max_elements = Some(64);
        inputs.name_case = "snake".to_owned();
        inputs.name_prefix = "ink_".to_owned();
        inputs.display_preset = Some(DisplayPreset::ItemHandheld);
        assert_eq!(
            targets_line(&inputs).as_deref(),
            Some(
                "16 texels per block \u{b7} textures up to 32 px \u{b7} 2 palette colours, strict \
                 \u{b7} up to 64 elements \u{b7} snake_case names \u{b7} prefix ink_ \u{b7} \
                 item/handheld display"
            )
        );
        // A goal is flattened to one line and cut.
        assert_eq!(goal_on_card("Splat\nweapons"), "Splat weapons");
        let long = goal_on_card(&"word ".repeat(100));
        assert!(long.chars().count() <= GOAL_ON_CARD + 1 && long.ends_with('\u{2026}'));
    }

    #[test]
    fn the_dialog_chooses_the_version_from_the_table() {
        let mut dialog = BriefDialog::open(&brief("1.21.6"), &table());
        assert_eq!(dialog.versions, ["Not set", "1.21.11", "1.21.6"]);
        assert_eq!(dialog.version, 2);
        dialog.version = 1;
        dialog.tags = " pixel, , cozy ".to_owned();
        let inputs = sent(&dialog);
        assert_eq!(inputs.project_name, "Mossy Hollow");
        assert_eq!(inputs.mc_version, "1.21.11");
        assert_eq!(inputs.style_tags, ["pixel", "cozy"]);
        dialog.version = 0;
        assert!(sent(&dialog).mc_version.is_empty());
    }

    #[test]
    fn a_version_outside_the_table_stays_choosable_and_an_empty_table_allows_typing() {
        let dialog = BriefDialog::open(&brief("1.19"), &table());
        assert_eq!(dialog.versions[1], "1.19");
        assert_eq!(dialog.version, 1);
        let mut typed = BriefDialog::open(&brief("1.19"), &[]);
        typed.typed = " 1.20 ".to_owned();
        assert_eq!(sent(&typed).mc_version, "1.20");
    }

    #[test]
    fn the_dialog_returns_every_field_of_the_brief() {
        let mut card = brief("1.21.6");
        card.inputs = BriefInputs {
            goal: "Splat weapons".to_owned(),
            platform: "oraxen".to_owned(),
            namespace: "splat".to_owned(),
            texel_density: Some(16.0),
            texture_size: Some(64),
            palette: vec![[0xc8, 0x64, 0x32]],
            palette_strict: true,
            max_elements: Some(32),
            name_case: "kebab".to_owned(),
            name_prefix: "ink-".to_owned(),
            display_preset: Some(DisplayPreset::BlockBlock),
            rules: vec!["No gradients".to_owned(), "Keep it blocky".to_owned()],
            ..card.inputs
        };
        let dialog = BriefDialog::open(&card, &table());
        // What the dialog opens on is what it sends back.
        assert_eq!(sent(&dialog), card.inputs);
    }

    #[test]
    fn numbers_colours_and_rules_are_read_from_the_fields() {
        let mut dialog = BriefDialog::open(&brief(""), &table());
        dialog.density = " 12.5 ".to_owned();
        dialog.budget = "x".to_owned();
        dialog.palette = "#C86432, 3a7bd5 nonsense #12345".to_owned();
        dialog.rules = " one \n\n two ".to_owned();
        dialog.texture_size = 3;
        dialog.display = 2;
        let inputs = sent(&dialog);
        assert_eq!(inputs.texel_density, Some(12.5));
        assert_eq!(inputs.max_elements, None);
        assert_eq!(inputs.palette, [[0xc8, 0x64, 0x32], [0x3a, 0x7b, 0xd5]]);
        assert_eq!(inputs.rules, ["one", "two"]);
        assert_eq!(inputs.texture_size, Some(64));
        assert_eq!(inputs.display_preset, Some(DisplayPreset::ItemHandheld));
        dialog.density = "-3".to_owned();
        assert_eq!(sent(&dialog).texel_density, None);
    }

    #[test]
    fn use_observed_values_copies_the_palette_and_the_density() {
        let mut card = brief("1.21.6");
        card.observed_palette = (0..20).map(|i| [i, 0, 0]).collect();
        card.observed_density = Some(18.625);
        let mut dialog = BriefDialog::open(&card, &table());
        assert!(dialog.palette.is_empty() && dialog.density.is_empty());
        dialog.use_observed();
        assert_eq!(parse_colors(&dialog.palette).len(), OBSERVED_COLORS);
        assert_eq!(dialog.density, "19");
        let inputs = sent(&dialog);
        assert_eq!(inputs.texel_density, Some(19.0));
        assert_eq!(inputs.palette.len(), OBSERVED_COLORS);
        // Nothing observed, nothing copied.
        let mut empty = BriefDialog::open(&brief("1.21.6"), &table());
        empty.density = "16".to_owned();
        empty.use_observed();
        assert_eq!(empty.density, "16");
    }
}
