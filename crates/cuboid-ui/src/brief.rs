//! The project brief of the start screen: a read-only card and the dialog that edits it.

use cuboid_ui_kit::metrics::space;
use cuboid_ui_kit::{
    Button, Icon, Modal, ModalButton, PropertyGrid, Type, color_swatch, dropdown, rgb_color, text,
    text_field,
};
use egui::{Context, Label, Ui, Vec2, vec2};

use crate::export_dialog::VersionChoice;
use crate::library::{BriefCardView, LibraryRequest, split_tags};

/// Side of a palette swatch.
const SWATCH: f32 = 16.0;
/// Most palette colours the card lists.
const SWATCHES: usize = 12;
/// The choice that leaves the game version unset.
const NO_VERSION: &str = "Not set";

/// The text under the project name: the game version and the style words.
pub(crate) fn summary(brief: &BriefCardView) -> String {
    let mut parts = Vec::new();
    if !brief.mc_version.is_empty() {
        parts.push(format!("Java {}", brief.mc_version));
    }
    if !brief.style_tags.is_empty() {
        parts.push(brief.style_tags.join(", "));
    }
    if parts.is_empty() {
        "No version or style set".to_owned()
    } else {
        parts.join(" \u{b7} ")
    }
}

/// The text under the palette: the texel density and the entry counts.
fn counts(brief: &BriefCardView) -> String {
    let density = brief.density.map_or_else(
        || "no texel density yet".to_owned(),
        |d| format!("{d:.0} texels per block"),
    );
    format!(
        "{density} \u{b7} {} models, {} textures",
        brief.models, brief.textures
    )
}

/// The read-only card. Returns whether Edit was clicked.
pub(crate) fn card(ui: &mut Ui, brief: &BriefCardView, busy: bool) -> bool {
    let mut edit = false;
    ui.horizontal(|ui| {
        ui.label(text(Type::Title, "Project brief"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = Button::new("Edit").ghost().icon(Icon::Pencil);
            edit = ui.add_enabled(!busy, button).clicked();
        });
    });
    ui.add_space(space::XS);
    if brief.project_name.is_empty() {
        ui.add(Label::new(text(Type::Label, "Untitled project")).selectable(false));
    } else {
        ui.add(
            Label::new(text(Type::Strong, &brief.project_name))
                .truncate()
                .selectable(false),
        );
    }
    ui.add(
        Label::new(text(Type::Caption, summary(brief)))
            .wrap()
            .selectable(false),
    );
    ui.add_space(space::SM);
    if brief.palette.is_empty() {
        ui.add(Label::new(text(Type::Caption, "No shared palette yet")).selectable(false));
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(space::XS);
            for rgb in brief.palette.iter().take(SWATCHES) {
                let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                color_swatch(ui, rgb_color(*rgb), vec2(SWATCH, SWATCH), false).on_hover_text(hex);
            }
        });
    }
    ui.add_space(space::SM);
    ui.add(
        Label::new(text(Type::Caption, counts(brief)))
            .wrap()
            .selectable(false),
    );
    edit
}

/// The dialog's state: the inputs, and the game versions to choose from.
pub(crate) struct BriefDialog {
    name: String,
    /// Index into `versions`; 0 is "Not set".
    version: usize,
    versions: Vec<String>,
    tags: String,
    /// The table is empty: the version is typed.
    typed: String,
    /// The version is chosen from the table.
    from_table: bool,
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
        let mut versions = vec![NO_VERSION.to_owned()];
        versions.extend(table.iter().map(|v| v.id.clone()));
        let version = match versions.iter().position(|v| *v == brief.mc_version) {
            Some(at) => at,
            None if brief.mc_version.is_empty() || table.is_empty() => 0,
            None => {
                versions.insert(1, brief.mc_version.clone());
                1
            }
        };
        Self {
            name: brief.project_name.clone(),
            version,
            versions,
            tags: brief.style_tags.join(", "),
            typed: brief.mc_version.clone(),
            from_table: !table.is_empty(),
        }
    }

    /// Draws the dialog. `None` while it is open.
    pub fn show(&mut self, ctx: &Context) -> Option<BriefAnswer> {
        let buttons = [ModalButton::new("Cancel"), ModalButton::primary("Save")];
        let table = self.from_table;
        let outcome = Modal::new("brief_dialog", "Project brief")
            .width(360.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                PropertyGrid::new("brief_dialog").show(ui, |grid| {
                    grid.row("Project", |ui| {
                        text_field(ui, "brief_name", &mut self.name, "Name of the project");
                    });
                    grid.row("Minecraft", |ui| {
                        if table {
                            let options: Vec<&str> =
                                self.versions.iter().map(String::as_str).collect();
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
        let save = outcome.button == Some(1) || outcome.confirmed;
        if save {
            return Some(BriefAnswer::Save(self.request()));
        }
        (outcome.button == Some(0) || outcome.cancelled).then_some(BriefAnswer::Cancel)
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
        LibraryRequest::SetBrief {
            name: self.name.trim().to_owned(),
            mc_version,
            style_tags: split_tags(&self.tags),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(version: &str) -> BriefCardView {
        BriefCardView {
            project_name: "Mossy Hollow".to_owned(),
            mc_version: version.to_owned(),
            style_tags: vec!["pixel".to_owned(), "low-poly".to_owned()],
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
    fn the_dialog_chooses_the_version_from_the_table() {
        let mut dialog = BriefDialog::open(&brief("1.21.6"), &table());
        assert_eq!(dialog.versions, ["Not set", "1.21.11", "1.21.6"]);
        assert_eq!(dialog.version, 2);
        dialog.version = 1;
        dialog.tags = " pixel, , cozy ".to_owned();
        let LibraryRequest::SetBrief {
            name,
            mc_version,
            style_tags,
        } = dialog.request()
        else {
            panic!("a brief request");
        };
        assert_eq!(name, "Mossy Hollow");
        assert_eq!(mc_version, "1.21.11");
        assert_eq!(style_tags, ["pixel", "cozy"]);
        dialog.version = 0;
        let LibraryRequest::SetBrief { mc_version, .. } = dialog.request() else {
            panic!("a brief request");
        };
        assert!(mc_version.is_empty());
    }

    #[test]
    fn a_version_outside_the_table_stays_choosable_and_an_empty_table_allows_typing() {
        let dialog = BriefDialog::open(&brief("1.19"), &table());
        assert_eq!(dialog.versions[1], "1.19");
        assert_eq!(dialog.version, 1);
        let mut typed = BriefDialog::open(&brief("1.19"), &[]);
        typed.typed = " 1.20 ".to_owned();
        let LibraryRequest::SetBrief { mc_version, .. } = typed.request() else {
            panic!("a brief request");
        };
        assert_eq!(mc_version, "1.20");
    }
}
