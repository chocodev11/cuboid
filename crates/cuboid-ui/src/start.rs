//! The start screen: the sidebar with Open and the new-model templates, the recent files and
//! the suggestions in the middle, and the project brief at the right (ui_v2_spec 9).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cuboid_ui_kit::metrics::{gutter, height, space};
use cuboid_ui_kit::{
    Button, Card, EmptyState, Icon, IconButton, ListRow, MenuItem, Modal, ModalButton, Tokens,
    Tone, Type, badge, content_frame, inset, search_field, segmented, text,
};
use egui::{
    Align, CentralPanel, Frame, Label, Layout, Panel, Popup, PopupCloseBehavior, ScrollArea, Sense,
    TextureId, Ui, Vec2, vec2,
};

use crate::brief::{self, BriefAnswer, BriefDialog};
use crate::common::ImageCache;
use crate::export_dialog::VersionChoice;
use crate::first_run;
use crate::library::{
    LibKind, LibScope, LibraryRequest, LibraryView, SuggestionGoal, SuggestionView,
};
use crate::model::{RecentEntry, UiCx};
use crate::prefs::{ComingFrom, Prefs};
use crate::request::Template;

/// Width of the sidebar.
const SIDEBAR: f32 = 240.0;
/// Width of the sidebar that shows icons only.
const SIDEBAR_COMPACT: f32 = height::STRIP + 2.0 * gutter::EDGE;
/// Padding either side of a badge's text.
const BADGE_PAD: f32 = 6.0;
/// Width of the brief column.
const BRIEF: f32 = 260.0;
/// From this width (in points) the brief has a column of its own.
const WIDE: f32 = 1280.0;
/// Below this width the sidebar shows icons only.
const COMPACT: f32 = 1024.0;
/// Narrowest and widest a recent card is drawn.
const CARD_MIN: f32 = 176.0;
const CARD_MAX: f32 = 232.0;
/// Most recent files the grid shows.
const MAX_RECENT: usize = 48;
/// Most suggestions listed.
const MAX_SUGGESTIONS: usize = 8;
/// Height of a suggestion row.
const SUGGESTION_ROW: f32 = height::ROW_THUMB;
/// Resolutions offered for a voxel model.
const VOXEL_RESOLUTIONS: [u32; 4] = [16, 32, 64, 128];
const VOXEL_LABELS: [&str; 4] = ["16", "32", "64", "128"];

/// What the user chose.
pub(crate) enum StartEvent {
    New(Template),
    Open(PathBuf),
    OpenDialog,
    RemoveRecent(PathBuf),
    /// The app the user knows, from the first-run dialog.
    ComingFrom(ComingFrom),
    /// The accent chosen in the first-run dialog.
    Accent([u8; 3]),
    /// The first-run dialog is done.
    Continue,
    Library(LibraryRequest),
    /// Stop the job of this id, from the brief dialog, whose backdrop covers the status bar.
    CancelJob(u64),
    /// Leave the start screen (only when a document is open).
    Close,
}

/// How many columns the window width allows (ui_v2_spec 11).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Columns {
    /// Sidebar, main area and brief.
    Three,
    /// The brief sits in the main area.
    Two,
    /// The sidebar shows icons only.
    Compact,
}

impl Columns {
    fn of(width: f32) -> Columns {
        if width >= WIDE {
            Columns::Three
        } else if width >= COMPACT {
            Columns::Two
        } else {
            Columns::Compact
        }
    }
}

pub(crate) struct StartState {
    voxel_ask: bool,
    voxel_index: usize,
    filter: String,
    brief: Option<BriefDialog>,
}

impl Default for StartState {
    fn default() -> Self {
        Self {
            voxel_ask: false,
            voxel_index: 1,
            filter: String::new(),
            brief: None,
        }
    }
}

impl StartState {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        prefs: &Prefs,
        can_close: bool,
        ask_first_run: bool,
        versions: &[VersionChoice],
    ) -> Vec<StartEvent> {
        let tokens = Tokens::of(ui.ctx());
        let columns = Columns::of(ui.ctx().content_rect().width());
        let mut events = Vec::new();
        let compact = columns == Columns::Compact;
        Panel::left("cuboid_start_sidebar")
            .frame(Frame::new().fill(tokens.bg_panel))
            .exact_size(if compact { SIDEBAR_COMPACT } else { SIDEBAR })
            .resizable(false)
            .show(ui, |ui| {
                inset(ui, gutter::EDGE, |ui| {
                    self.sidebar(ui, cx, can_close, compact, &mut events);
                });
            });
        if columns == Columns::Three {
            Panel::right("cuboid_start_brief")
                .frame(Frame::new().fill(tokens.bg_panel))
                .exact_size(BRIEF)
                .resizable(false)
                .show(ui, |ui| {
                    inset(ui, gutter::EDGE, |ui| {
                        if let Some(view) = cx.library
                            && brief::card(ui, &view.brief, view.busy)
                        {
                            self.brief = Some(BriefDialog::open(&view.brief, versions));
                        }
                    });
                });
        }
        CentralPanel::default()
            .frame(Frame::new().fill(tokens.bg_app))
            .show(ui, |ui| {
                inset(ui, space::LG, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.main(ui, cx, images, columns, versions, &mut events);
                        });
                });
            });
        self.dialogs(ui.ctx(), cx, prefs, ask_first_run, &mut events);
        events
    }

    fn dialogs(
        &mut self,
        ctx: &egui::Context,
        cx: &UiCx<'_>,
        prefs: &Prefs,
        ask_first_run: bool,
        events: &mut Vec<StartEvent>,
    ) {
        if self.voxel_ask {
            let buttons = [ModalButton::new("Cancel"), ModalButton::primary("Create")];
            let outcome = Modal::new("voxel_size", "New voxel model")
                .buttons(&buttons)
                .show(ctx, |ui| {
                    ui.label(text(Type::Body, "Voxels per block"));
                    ui.add_space(space::SM);
                    segmented(ui, &mut self.voxel_index, &VOXEL_LABELS);
                });
            if outcome.button == Some(1) || outcome.confirmed {
                self.voxel_ask = false;
                let resolution =
                    VOXEL_RESOLUTIONS[self.voxel_index.min(VOXEL_RESOLUTIONS.len() - 1)];
                events.push(StartEvent::New(Template::Voxel { resolution }));
            } else if outcome.button == Some(0) || outcome.cancelled {
                self.voxel_ask = false;
            }
        }
        if let Some(dialog) = self.brief.as_mut()
            && let Some(view) = cx.library
        {
            match dialog.show(ctx, view) {
                Some(BriefAnswer::Save(request)) => {
                    events.push(StartEvent::Library(request));
                    self.brief = None;
                }
                Some(BriefAnswer::Style(request)) => events.push(StartEvent::Library(request)),
                Some(BriefAnswer::CancelJob(id)) => events.push(StartEvent::CancelJob(id)),
                Some(BriefAnswer::Cancel) => self.brief = None,
                None => {}
            }
        }
        if ask_first_run {
            first_run::show(ctx, prefs, events);
        }
        // Without a library there is nothing to edit.
        if cx.library.is_none() {
            self.brief = None;
        }
    }

    fn sidebar(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        can_close: bool,
        compact: bool,
        events: &mut Vec<StartEvent>,
    ) {
        ui.spacing_mut().item_spacing.y = space::XXS;
        if can_close {
            let back = Button::new("Back to the model")
                .ghost()
                .icon(Icon::ArrowLeft);
            let back = if compact {
                back.icon_only()
            } else {
                back.fill_width()
            };
            if ui.add(back).clicked() {
                events.push(StartEvent::Close);
            }
            ui.add_space(space::XS);
        }
        let open = Button::new("Open file")
            .primary()
            .tall()
            .icon(Icon::FolderOpen);
        let open = if compact {
            open.icon_only()
        } else {
            open.fill_width()
        };
        if ui.add(open.min_width(0.0)).clicked() {
            events.push(StartEvent::OpenDialog);
        }
        ui.add_space(space::SM);
        if !compact {
            ui.label(text(Type::Caption, "New"));
        }
        for (icon, title, caption, template) in [
            (
                Icon::Cube,
                "Block",
                "Minecraft, 16 px",
                Some(Template::Block),
            ),
            (
                Icon::Sword,
                "Item",
                "handheld or flat",
                Some(Template::Item),
            ),
            (Icon::Voxel, "Voxel", "pick a size", None),
            (Icon::File, "Empty", "", Some(Template::Empty)),
            (
                Icon::History,
                "Cubik classic",
                "",
                Some(Template::CubikClassic),
            ),
        ] {
            let clicked = if compact {
                let tip = if caption.is_empty() {
                    title.to_owned()
                } else {
                    format!("{title}: {caption}")
                };
                ui.add(IconButton::new(icon, &tip).toolbar()).clicked()
            } else {
                let mut row = ListRow::new(title).icon(icon);
                if !caption.is_empty() {
                    row = row.caption(caption);
                }
                ui.add(row).clicked()
            };
            if clicked {
                match template {
                    Some(template) => events.push(StartEvent::New(template)),
                    None => self.voxel_ask = true,
                }
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            library_block(ui, cx.library, compact, events);
        });
    }

    fn main(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        columns: Columns,
        versions: &[VersionChoice],
        events: &mut Vec<StartEvent>,
    ) {
        let library = cx.library;
        let empty = cx.recent.is_empty() && library.is_none_or(|l| l.total == 0 && !l.busy);
        if empty {
            ui.add_space(gutter::GROUP);
            let clicked = EmptyState::new(Icon::Package, "No models yet")
                .body("Start a new block, open a file or import a folder into the library.")
                .action(Button::new("New block").primary().icon(Icon::Plus))
                .action(Button::new("Open file").icon(Icon::FolderOpen))
                .action(Button::new("Import a folder").icon(Icon::Import))
                .show(ui);
            match clicked {
                Some(0) => events.push(StartEvent::New(Template::Block)),
                Some(1) => events.push(StartEvent::OpenDialog),
                Some(2) => events.push(StartEvent::Library(LibraryRequest::ImportFolder)),
                _ => {}
            }
        } else {
            self.recent(ui, cx, images, events);
        }
        if let Some(view) = library {
            ui.add_space(gutter::GROUP);
            suggestions(ui, &view.suggestions, events);
            if columns != Columns::Three {
                ui.add_space(gutter::GROUP);
                content_frame(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if brief::card(ui, &view.brief, view.busy) {
                        self.brief = Some(BriefDialog::open(&view.brief, versions));
                    }
                });
            }
        }
    }

    fn recent(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        events: &mut Vec<StartEvent>,
    ) {
        ui.horizontal(|ui| {
            ui.label(text(Type::Title, "Recent"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.allocate_ui_with_layout(
                    vec2(200.0, height::CONTROL),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        search_field(ui, "start_filter", &mut self.filter, "Filter recent files");
                    },
                );
            });
        });
        ui.add_space(space::SM);
        let pinned = cx.library.map_or(&[][..], |lib| &lib.pinned[..]);
        let needle = self.filter.trim().to_lowercase();
        let mut shown: Vec<(&RecentEntry, bool)> = cx
            .recent
            .iter()
            .filter(|e| {
                needle.is_empty()
                    || e.name.to_lowercase().contains(&needle)
                    || e.detail.to_lowercase().contains(&needle)
            })
            .map(|e| (e, pinned.contains(&e.path)))
            .collect();
        // Pinned files first; the order of the rest stays.
        shown.sort_by_key(|(_, pin)| !*pin);
        if cx.recent.is_empty() {
            ui.label(text(
                Type::Label,
                "Files you open or save will show up here.",
            ));
        } else if shown.is_empty() {
            ui.label(text(Type::Label, "No recent file matches."));
        }
        let gap = gutter::EDGE;
        let available = ui.available_width();
        let columns = ((available + gap) / (CARD_MIN + gap)).floor().max(1.0);
        let width = ((available - gap * (columns - 1.0)) / columns)
            .floor()
            .min(CARD_MAX);
        // `columns` is a small whole number of at least 1.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let per_row = columns as usize;
        let now = SystemTime::now();
        ui.spacing_mut().item_spacing = Vec2::splat(gap);
        let items: Vec<_> = shown.into_iter().take(MAX_RECENT).collect();
        for row in items.chunks(per_row) {
            ui.horizontal(|ui| {
                for (entry, pin) in row {
                    recent_card(ui, entry, *pin, width, now, images, events);
                }
            });
        }
    }
}

/// The library counts and the import button at the bottom of the sidebar. The widgets are added
/// bottom up.
fn library_block(
    ui: &mut Ui,
    library: Option<&LibraryView>,
    compact: bool,
    events: &mut Vec<StartEvent>,
) {
    ui.spacing_mut().item_spacing.y = space::XS;
    let busy = library.is_none_or(|l| l.busy);
    let import = {
        let button = Button::new("Import into library").icon(Icon::Import);
        let button = if compact {
            button.icon_only()
        } else {
            button.fill_width()
        };
        ui.add_enabled(!busy, button)
    };
    Popup::from_toggle_button_response(&import)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            for (label, request) in [
                ("Import folder...", LibraryRequest::ImportFolder),
                ("Import files...", LibraryRequest::ImportFiles),
            ] {
                if ui.add(MenuItem::new(label)).clicked() {
                    events.push(StartEvent::Library(request));
                    ui.close();
                }
            }
        });
    if compact {
        return;
    }
    let Some(view) = library else {
        ui.label(text(Type::Caption, "The library is not available."));
        return;
    };
    if view.busy {
        ui.label(text(Type::Caption, "Working..."));
    }
    let chips: Vec<String> = LibKind::ALL
        .into_iter()
        .zip(view.counts)
        .filter(|(_, count)| *count > 0)
        .map(|(kind, count)| format!("{count} {}", kind_noun(kind)))
        .collect();
    chip_rows(ui, &chips);
    if view.project_available {
        let mut scope = usize::from(view.scope == LibScope::Project);
        if segmented(ui, &mut scope, &["Global", "Project"]) {
            let scope = if scope == 1 {
                LibScope::Project
            } else {
                LibScope::Global
            };
            events.push(StartEvent::Library(LibraryRequest::SetScope(scope)));
        }
    }
    ui.label(text(
        Type::Caption,
        format!("Library \u{b7} {} items", view.total),
    ));
}

/// Badges in rows that wrap, as one block. Laid out by hand because the sidebar adds its bottom
/// block upwards, and a wrapping layout inside it would run into the rows above.
fn chip_rows(ui: &mut Ui, labels: &[String]) {
    if labels.is_empty() {
        return;
    }
    let gap = space::XS;
    let width = ui.available_width();
    let ink = Tokens::of(ui.ctx()).text_weak;
    let sizes: Vec<Vec2> = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap(label.clone(), Type::Caption.font(), ink)
                .size()
        })
        .collect();
    let widths: Vec<f32> = sizes.iter().map(|s| s.x + 2.0 * BADGE_PAD).collect();
    let row_height = sizes.first().map_or(0.0, |s| s.y) + 2.0 * space::XXS;
    let mut slots = Vec::with_capacity(labels.len());
    let (mut x, mut row) = (0.0_f32, 0.0_f32);
    for w in &widths {
        if x > 0.0 && x + w > width {
            x = 0.0;
            row += 1.0;
        }
        slots.push((x, row));
        x += w + gap;
    }
    let rows = row + 1.0;
    let (area, _) = ui.allocate_exact_size(
        vec2(width, rows * row_height + (rows - 1.0) * gap),
        Sense::hover(),
    );
    for (label, ((x, row), w)) in labels.iter().zip(slots.into_iter().zip(widths)) {
        let slot = egui::Rect::from_min_size(
            area.min + vec2(x, row * (row_height + gap)),
            vec2(w, row_height),
        );
        let mut place = ui.new_child(egui::UiBuilder::new().max_rect(slot));
        badge(&mut place, label, Tone::Neutral);
    }
}

fn kind_noun(kind: LibKind) -> &'static str {
    match kind {
        LibKind::Model => "models",
        LibKind::Texture => "textures",
        LibKind::Json => "JSON",
        LibKind::Reference => "references",
        LibKind::Lore => "lore",
    }
}

/// The suggestion rows, each with its action button.
fn suggestions(ui: &mut Ui, list: &[SuggestionView], events: &mut Vec<StartEvent>) {
    ui.label(text(Type::Title, "Suggestions"));
    ui.add_space(space::SM);
    if list.is_empty() {
        ui.add(
            Label::new(text(
                Type::Label,
                "Import models, textures and lore notes. Hints about missing models, colours and texel density show up here.",
            ))
            .wrap()
            .selectable(false),
        );
        return;
    }
    ui.spacing_mut().item_spacing.y = space::SM;
    for item in list.iter().take(MAX_SUGGESTIONS) {
        suggestion_row(ui, item, events);
    }
    if list.len() > MAX_SUGGESTIONS {
        ui.label(text(
            Type::Caption,
            format!("{} more", list.len() - MAX_SUGGESTIONS),
        ));
    }
}

fn suggestion_row(ui: &mut Ui, item: &SuggestionView, events: &mut Vec<StartEvent>) {
    let t = Tokens::of(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), SUGGESTION_ROW), Sense::hover());
    ui.painter()
        .rect_filled(rect, cuboid_ui_kit::metrics::radius::CARD, t.bg_raised);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(gutter::EDGE, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = space::SM;
    let (icon_rect, _) = row.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    Icon::Lightbulb.paint(row.painter(), icon_rect, t.text_weak);
    if let Some(action) = &item.action {
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(Button::new(&action.label)).clicked() {
                events.push(match &action.goal {
                    SuggestionGoal::New(template) => StartEvent::New(*template),
                    SuggestionGoal::Open(path) => StartEvent::Open(path.clone()),
                });
            }
            ui.with_layout(Layout::top_down(Align::Min), |ui| text_lines(ui, item));
        });
    } else {
        row.with_layout(Layout::top_down(Align::Min), |ui| text_lines(ui, item));
    }
}

/// The title and the detail of a suggestion.
fn text_lines(ui: &mut Ui, item: &SuggestionView) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add(
        Label::new(text(Type::Strong, &item.title))
            .truncate()
            .selectable(false),
    );
    ui.add(
        Label::new(text(Type::Caption, &item.detail))
            .truncate()
            .selectable(false),
    );
}

/// A recent file as a card; clicks go to `events`.
fn recent_card(
    ui: &mut Ui,
    entry: &RecentEntry,
    pinned: bool,
    width: f32,
    now: SystemTime,
    images: &mut ImageCache,
    events: &mut Vec<StartEvent>,
) {
    let stem = Path::new(&entry.name)
        .file_stem()
        .map_or(entry.name.as_str(), |s| s.to_str().unwrap_or(&entry.name));
    let format = Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    let caption = match entry.modified {
        Some(modified) => relative_time(now, modified),
        None => entry.detail.clone(),
    };
    let thumbnail: Option<(TextureId, Vec2)> = entry.thumbnail.as_ref().and_then(|image| {
        let key = format!("recent:{}", entry.path.display());
        let id = images.texture(ui.ctx(), &key, image)?;
        let [w, h] = image.size;
        #[allow(clippy::cast_precision_loss)]
        Some((id, vec2(w as f32, h as f32)))
    });
    let mut card = Card::new(stem).caption(&caption).width(width).pin(pinned);
    if let Some(format) = &format {
        card = card.badge(format);
    }
    if let Some((id, size)) = thumbnail {
        card = card.thumbnail(id, size);
    }
    let response = card.show(ui);
    let pin_clicked = response.pin.as_ref().is_some_and(|pin| pin.clicked());
    if pin_clicked {
        events.push(StartEvent::Library(LibraryRequest::TogglePin(
            entry.path.clone(),
        )));
    } else if response.clicked() {
        events.push(StartEvent::Open(entry.path.clone()));
    }
    response
        .response
        .on_hover_text(entry.path.display().to_string())
        .context_menu(|ui| {
            let label = if pinned { "Unpin" } else { "Pin to the top" };
            if ui.add(MenuItem::new(label)).clicked() {
                events.push(StartEvent::Library(LibraryRequest::TogglePin(
                    entry.path.clone(),
                )));
                ui.close();
            }
            if ui.add(MenuItem::new("Remove from the list")).clicked() {
                events.push(StartEvent::RemoveRecent(entry.path.clone()));
                ui.close();
            }
        });
}

/// "just now", "5 minutes ago", "1 day ago": the age of a file with the units in the right
/// number.
pub(crate) fn relative_time(now: SystemTime, then: SystemTime) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const MONTH: u64 = 30 * DAY;
    const YEAR: u64 = 365 * DAY;
    let secs = now.duration_since(then).map_or(0, |d| d.as_secs());
    let ago = |n: u64, unit: &str| {
        let plural = if n == 1 { "" } else { "s" };
        format!("{n} {unit}{plural} ago")
    };
    match secs {
        0..MINUTE => "just now".to_owned(),
        MINUTE..HOUR => ago(secs / MINUTE, "minute"),
        HOUR..DAY => ago(secs / HOUR, "hour"),
        DAY..WEEK => ago(secs / DAY, "day"),
        WEEK..MONTH => ago(secs / WEEK, "week"),
        MONTH..YEAR => ago(secs / MONTH, "month"),
        _ => ago(secs / YEAR, "year"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn age(secs: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000_000_000);
        relative_time(now, now - Duration::from_secs(secs))
    }

    #[test]
    fn the_age_of_a_file_reads_with_correct_plurals() {
        assert_eq!(age(5), "just now");
        assert_eq!(age(60), "1 minute ago");
        assert_eq!(age(150), "2 minutes ago");
        assert_eq!(age(3_600), "1 hour ago");
        assert_eq!(age(7_300), "2 hours ago");
        assert_eq!(age(86_400), "1 day ago");
        assert_eq!(age(3 * 86_400), "3 days ago");
        assert_eq!(age(7 * 86_400), "1 week ago");
        assert_eq!(age(21 * 86_400), "3 weeks ago");
        assert_eq!(age(31 * 86_400), "1 month ago");
        assert_eq!(age(120 * 86_400), "4 months ago");
        assert_eq!(age(400 * 86_400), "1 year ago");
        assert_eq!(age(800 * 86_400), "2 years ago");
    }

    #[test]
    fn a_file_from_the_future_counts_as_new() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(
            relative_time(now, now + Duration::from_secs(90)),
            "just now"
        );
    }

    #[test]
    fn the_window_width_picks_the_columns() {
        assert_eq!(Columns::of(1920.0), Columns::Three);
        assert_eq!(Columns::of(1280.0), Columns::Three);
        assert_eq!(Columns::of(1279.0), Columns::Two);
        assert_eq!(Columns::of(1024.0), Columns::Two);
        assert_eq!(Columns::of(1023.0), Columns::Compact);
    }
}
