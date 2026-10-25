//! The library as the UI sees it: plain view data from the app, the requests it sends back, and
//! the dockable Library panel. The UI never reads library files; the app builds the view from
//! `cuboid-library` and keeps the thumbnails.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use cuboid_ui_kit::{Icon, RADIUS, RADIUS_SM, Tokens, search_field};
use egui::{Align2, Context, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::ImageData;
use crate::request::{Template, UiRequest};

const ROW_H: f32 = 44.0;
const THUMB: f32 = 36.0;
/// Tags offered as quick filters.
const TAG_CHIPS: usize = 8;

/// The kinds of library entry.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum LibKind {
    /// Models.
    Model,
    /// Textures.
    Texture,
    /// JSON files.
    Json,
    /// Reference pictures.
    Reference,
    /// Lore notes.
    Lore,
}

impl LibKind {
    /// All kinds in display order.
    pub const ALL: [LibKind; 5] = [
        LibKind::Model,
        LibKind::Texture,
        LibKind::Json,
        LibKind::Reference,
        LibKind::Lore,
    ];

    /// Plural name for counts and filters.
    pub fn plural(self) -> &'static str {
        match self {
            LibKind::Model => "Models",
            LibKind::Texture => "Textures",
            LibKind::Json => "JSON",
            LibKind::Reference => "References",
            LibKind::Lore => "Lore",
        }
    }

    fn icon(self) -> Icon {
        match self {
            LibKind::Model => Icon::Cube,
            LibKind::Texture => Icon::Grid,
            LibKind::Json => Icon::File,
            LibKind::Reference => Icon::Image,
            LibKind::Lore => Icon::Menu,
        }
    }
}

/// Which library is shown.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum LibScope {
    /// Shared by every project.
    #[default]
    Global,
    /// Beside the open document.
    Project,
}

/// What the app knows about the picture of an entry.
#[derive(Clone, Debug, Default)]
pub enum ThumbState {
    /// Not asked for yet: the UI asks when the row is visible.
    #[default]
    Unknown,
    /// There is none (yet): a placeholder shows.
    Missing,
    /// The picture.
    Ready(ImageData),
}

/// One row of the library list.
#[derive(Clone, Debug)]
pub struct LibEntryView {
    /// Identity, echoed by requests.
    pub id: String,
    /// Kind.
    pub kind: LibKind,
    /// File name.
    pub name: String,
    /// Second line, such as the size or the element count.
    pub detail: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Picture state.
    pub thumb: ThumbState,
}

/// The brief card: the inputs and what is computed from the entries.
#[derive(Clone, Debug, Default)]
pub struct BriefCardView {
    /// Project name; empty when not named.
    pub project_name: String,
    /// Target Minecraft version.
    pub mc_version: String,
    /// Style words.
    pub style_tags: Vec<String>,
    /// Shared palette, RGB.
    pub palette: Vec<[u8; 3]>,
    /// Median texels per block of the models.
    pub density: Option<f32>,
    /// Models in the library.
    pub models: usize,
    /// Textures in the library.
    pub textures: usize,
}

/// What the button of a suggestion does.
#[derive(Clone, Debug, PartialEq)]
pub enum SuggestionGoal {
    /// Start a new model.
    New(Template),
    /// Open a file.
    Open(PathBuf),
}

/// The button of a suggestion.
#[derive(Clone, Debug, PartialEq)]
pub struct SuggestionAction {
    /// The label, such as "Create" or "Open".
    pub label: String,
    /// What it does.
    pub goal: SuggestionGoal,
}

/// A hint under the recent files.
#[derive(Clone, Debug)]
pub struct SuggestionView {
    /// One line.
    pub title: String,
    /// What to do.
    pub detail: String,
    /// The button of the row; `None` for a hint without one.
    pub action: Option<SuggestionAction>,
}

/// Everything the library panel and the start screen show. Build it when the library changes
/// and keep it between frames; `rev` tells the UI to look again.
#[derive(Clone, Debug)]
pub struct LibraryView {
    /// Changes whenever anything below changes.
    pub rev: u64,
    /// The library shown.
    pub scope: LibScope,
    /// The project library can be chosen (the document has a path).
    pub project_available: bool,
    /// The library is being read or a job holds it; edits wait.
    pub busy: bool,
    /// Entries per kind in [`LibKind::ALL`] order.
    pub counts: [usize; 5],
    /// Entries in total.
    pub total: usize,
    /// Entries that match the query; `entries` may hold fewer.
    pub matches: usize,
    /// The entries that match the query, newest first.
    pub entries: Arc<[LibEntryView]>,
    /// Tags in use, most used first.
    pub tags: Arc<[(String, usize)]>,
    /// The brief.
    pub brief: BriefCardView,
    /// Hints.
    pub suggestions: Arc<[SuggestionView]>,
    /// Recent files the user pinned.
    pub pinned: Arc<[PathBuf]>,
}

impl Default for LibraryView {
    fn default() -> Self {
        Self {
            rev: 0,
            scope: LibScope::Global,
            project_available: false,
            busy: true,
            counts: [0; 5],
            total: 0,
            matches: 0,
            entries: Arc::from([]),
            tags: Arc::from([]),
            brief: BriefCardView::default(),
            suggestions: Arc::from([]),
            pinned: Arc::from([]),
        }
    }
}

/// What the library UI asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum LibraryRequest {
    /// Ask for a folder, then import it.
    ImportFolder,
    /// Ask for files, then import them.
    ImportFiles,
    /// Import these files and folders (dropped on the panel).
    Import(Vec<PathBuf>),
    /// Choose the library to show.
    SetScope(LibScope),
    /// The search changed.
    Query {
        /// Words.
        text: String,
        /// Only this kind.
        kind: Option<LibKind>,
        /// Only this tag.
        tag: Option<String>,
    },
    /// Pictures of these entries are wanted.
    Thumbs(Vec<String>),
    /// Replace the tags of an entry.
    SetTags {
        /// The entry.
        id: String,
        /// The new tags.
        tags: Vec<String>,
    },
    /// Take an entry out of the library.
    Remove(String),
    /// Replace the brief inputs.
    SetBrief {
        /// Project name.
        name: String,
        /// Target Minecraft version.
        mc_version: String,
        /// Style words.
        style_tags: Vec<String>,
    },
    /// A texture was dropped on the model: add it to the document.
    UseTexture(String),
    /// A reference was dropped on the model: pin it to the view.
    PinReference(String),
    /// Pin a recent file to the top of the list, or unpin it.
    TogglePin(PathBuf),
}

/// What a library row carries while it is dragged.
#[derive(Clone, Debug)]
pub(crate) struct LibDrag {
    id: String,
    kind: LibKind,
    name: String,
}

#[derive(Default)]
pub(crate) struct LibraryPanel {
    text: String,
    kind: Option<LibKind>,
    tag: Option<String>,
    sent: Option<(String, Option<LibKind>, Option<String>)>,
    asked: HashSet<String>,
    seen_rev: u64,
    selected: Option<String>,
    tag_text: String,
    tag_for: Option<String>,
    /// The entry whose removal waits for the answer of the question.
    confirm_remove: Option<String>,
    /// Where the panel was drawn last frame, so a file dropped on it imports instead of opens.
    pub rect: Option<Rect>,
}

impl LibraryPanel {
    /// The panel body.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        view: Option<&LibraryView>,
        images: &mut ImageCache,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        self.rect = Some(ui.max_rect());
        let Some(view) = view else {
            ui.label(egui::RichText::new("The library is not available.").color(tokens.text_weak));
            return;
        };
        if view.rev != self.seen_rev {
            // A new view: pictures that were asked for may have arrived or been lost.
            self.seen_rev = view.rev;
            self.asked.clear();
        }
        self.dropped_files(ui, sink);
        self.header(ui, view, sink);
        self.filters(ui, view, sink);
        ui.add_space(4.0);
        self.list(ui, view, images, sink);
        self.selection(ui, view, sink);
    }

    fn header(&mut self, ui: &mut Ui, view: &LibraryView, sink: &mut Sink) {
        ui.horizontal(|ui| {
            scope_switch(ui, view, sink);
            if ui
                .add_enabled(!view.busy, egui::Button::new("Import files..."))
                .clicked()
            {
                sink.push(UiRequest::Library(LibraryRequest::ImportFiles));
            }
            if ui
                .add_enabled(!view.busy, egui::Button::new("Import folder..."))
                .clicked()
            {
                sink.push(UiRequest::Library(LibraryRequest::ImportFolder));
            }
        });
    }

    fn filters(&mut self, ui: &mut Ui, view: &LibraryView, sink: &mut Sink) {
        search_field(ui, "library_search", &mut self.text, "Search the library");
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.kind.is_none(), "All").clicked() {
                self.kind = None;
            }
            for kind in LibKind::ALL {
                let label = format!("{} {}", kind.plural(), view.counts[kind_index(kind)]);
                if ui
                    .selectable_label(self.kind == Some(kind), label)
                    .clicked()
                {
                    self.kind = (self.kind != Some(kind)).then_some(kind);
                }
            }
        });
        if !view.tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (tag, count) in view.tags.iter().take(TAG_CHIPS) {
                    let on = self.tag.as_deref() == Some(tag.as_str());
                    if ui.selectable_label(on, format!("#{tag} {count}")).clicked() {
                        self.tag = (!on).then(|| tag.clone());
                    }
                }
            });
        }
        let now = (self.text.clone(), self.kind, self.tag.clone());
        if self.sent.as_ref() != Some(&now) {
            self.sent = Some(now.clone());
            sink.push(UiRequest::Library(LibraryRequest::Query {
                text: now.0,
                kind: now.1,
                tag: now.2,
            }));
        }
    }

    fn list(&mut self, ui: &mut Ui, view: &LibraryView, images: &mut ImageCache, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        if view.entries.is_empty() {
            let text = if view.busy {
                "Reading the library..."
            } else if view.total == 0 {
                "The library is empty. Import files or a folder, or drop them here."
            } else {
                "Nothing matches."
            };
            ui.label(egui::RichText::new(text).color(tokens.text_weak));
            return;
        }
        let height = (ui.available_height() - 70.0).max(ROW_H * 3.0);
        let mut wanted = Vec::new();
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, view.entries.len(), |ui, range| {
                for entry in &view.entries[range] {
                    let selected = self.selected.as_deref() == Some(entry.id.as_str());
                    let resp = entry_row(ui, &tokens, entry, selected, images);
                    if matches!(entry.thumb, ThumbState::Unknown) && !self.asked.contains(&entry.id)
                    {
                        wanted.push(entry.id.clone());
                    }
                    if resp.clicked() {
                        self.selected = Some(entry.id.clone());
                    }
                    resp.dnd_set_drag_payload(LibDrag {
                        id: entry.id.clone(),
                        kind: entry.kind,
                        name: entry.name.clone(),
                    });
                    resp.context_menu(|ui| {
                        if ui.button("Remove from the library").clicked() {
                            self.confirm_remove = Some(entry.id.clone());
                            ui.close();
                        }
                    });
                }
            });
        self.ask_remove(ui.ctx(), view, sink);
        if view.matches > view.entries.len() {
            ui.label(
                egui::RichText::new(format!(
                    "{} more match; refine the search.",
                    view.matches - view.entries.len()
                ))
                .size(12.0)
                .color(tokens.text_weak),
            );
        }
        if !wanted.is_empty() {
            self.asked.extend(wanted.iter().cloned());
            sink.push(UiRequest::Library(LibraryRequest::Thumbs(wanted)));
        }
    }

    /// The question before the stored copy, its thumbnail, tags and licence are deleted.
    fn ask_remove(&mut self, ctx: &Context, view: &LibraryView, sink: &mut Sink) {
        let Some(id) = self.confirm_remove.clone() else {
            return;
        };
        let name = view
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map_or("this file", |entry| entry.name.as_str());
        let modal = egui::Modal::new(egui::Id::unique("cuboid_library_remove")).show(ctx, |ui| {
            ui.label(format!("Remove {name} from the library?"));
            ui.label("The stored copy, its picture, tags and licence are deleted. Your original file is not touched.");
            ui.horizontal(|ui| {
                if ui.button("Remove").clicked() {
                    sink.push(UiRequest::Library(LibraryRequest::Remove(id.clone())));
                    self.confirm_remove = None;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_remove = None;
                }
            });
        });
        if modal.should_close() {
            self.confirm_remove = None;
        }
    }

    /// The tag editor of the selected entry.
    fn selection(&mut self, ui: &mut Ui, view: &LibraryView, sink: &mut Sink) {
        let Some(entry) = self
            .selected
            .as_deref()
            .and_then(|id| view.entries.iter().find(|e| e.id == id))
        else {
            return;
        };
        if self.tag_for.as_deref() != Some(entry.id.as_str()) {
            self.tag_for = Some(entry.id.clone());
            self.tag_text = entry.tags.join(", ");
        }
        ui.separator();
        ui.label(egui::RichText::new(format!("Tags of {}", entry.name)).size(12.0));
        let resp = ui.add_enabled(
            !view.busy,
            egui::TextEdit::singleline(&mut self.tag_text)
                .hint_text("pixel, weapon, blue")
                .desired_width(f32::INFINITY),
        );
        if resp.lost_focus() {
            sink.push(UiRequest::Library(LibraryRequest::SetTags {
                id: entry.id.clone(),
                tags: split_tags(&self.tag_text),
            }));
        }
    }

    /// Files dropped on the panel are imported; elsewhere a drop opens a file.
    fn dropped_files(&mut self, ui: &Ui, sink: &mut Sink) {
        let Some(rect) = self.rect else { return };
        let over = ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|p| rect.contains(p));
        if !over {
            return;
        }
        let paths: Vec<PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !paths.is_empty() {
            sink.push(UiRequest::Library(LibraryRequest::Import(paths)));
        }
    }
}

fn kind_index(kind: LibKind) -> usize {
    LibKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

/// Comma separated tags to a list; the library trims and dedupes.
pub(crate) fn split_tags(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A row of mutually exclusive text buttons, like the kit's `segmented` but with the label of the
/// selected one drawn in the colour for text on the accent (the kit bakes the normal text colour
/// into every label). Returns whether the choice changed.
pub(crate) fn choice_row(ui: &mut Ui, current: &mut usize, labels: &[&str]) -> bool {
    let tokens = Tokens::of(ui.ctx());
    let mut changed = false;
    egui::Frame::new()
        .fill(tokens.bg_input)
        .corner_radius(RADIUS)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let selected = *current == i;
                    let font = FontId::proportional(13.0);
                    let width = ui
                        .painter()
                        .layout_no_wrap((*label).to_owned(), font.clone(), tokens.text)
                        .size()
                        .x;
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(width + 16.0, 24.0), Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::Button, true, selected, *label)
                    });
                    let (fill, ink) = if selected {
                        (tokens.accent, tokens.on_accent)
                    } else if resp.hovered() {
                        (tokens.bg_hover, tokens.text_strong)
                    } else {
                        (egui::Color32::TRANSPARENT, tokens.text)
                    };
                    ui.painter().rect_filled(rect, RADIUS_SM, fill);
                    ui.painter()
                        .text(rect.center(), Align2::CENTER_CENTER, *label, font, ink);
                    if resp.clicked() && !selected {
                        *current = i;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// The Global or Project switch. A project library needs a saved document.
pub(crate) fn scope_switch(ui: &mut Ui, view: &LibraryView, sink: &mut Sink) {
    let mut index = usize::from(view.scope == LibScope::Project);
    let labels: &[&str] = if view.project_available {
        &["Global", "Project"]
    } else {
        &["Global"]
    };
    if choice_row(ui, &mut index, labels) {
        let scope = if index == 1 {
            LibScope::Project
        } else {
            LibScope::Global
        };
        sink.push(UiRequest::Library(LibraryRequest::SetScope(scope)));
    }
}

/// Draws a picture or the placeholder of a kind in `rect`.
pub(crate) fn paint_thumb(
    ui: &Ui,
    tokens: &Tokens,
    rect: Rect,
    kind: LibKind,
    thumb: &ThumbState,
    key: &str,
    images: &mut ImageCache,
) {
    let painter = ui.painter();
    painter.rect(
        rect,
        RADIUS_SM,
        tokens.bg_input,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    if let ThumbState::Ready(image) = thumb
        && let Some(id) = images.texture(ui.ctx(), key, image)
    {
        let [w, h] = image.size;
        #[allow(clippy::cast_precision_loss)]
        let aspect = w as f32 / h.max(1) as f32;
        let fitted = fit(rect.shrink(1.0), aspect);
        painter.image(
            id,
            fitted,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        return;
    }
    kind.icon().paint(
        painter,
        Rect::from_center_size(rect.center(), rect.size() * 0.5),
        tokens.text_weak,
    );
}

/// The largest rectangle of the given aspect ratio centred in `outer`.
fn fit(outer: Rect, aspect: f32) -> Rect {
    if !aspect.is_finite() || aspect <= 0.0 {
        return outer;
    }
    let (mut w, mut h) = (outer.width(), outer.width() / aspect);
    if h > outer.height() {
        h = outer.height();
        w = h * aspect;
    }
    Rect::from_center_size(outer.center(), vec2(w, h))
}

fn entry_row(
    ui: &mut Ui,
    tokens: &Tokens,
    entry: &LibEntryView,
    selected: bool,
    images: &mut ImageCache,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, selected, entry.name.as_str())
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, RADIUS, tokens.accent_soft);
        } else if resp.hovered() {
            painter.rect_filled(rect, RADIUS, tokens.bg_hover);
        }
        let thumb = Rect::from_min_size(
            rect.min + vec2(4.0, (ROW_H - THUMB) / 2.0),
            vec2(THUMB, THUMB),
        );
        paint_thumb(
            ui,
            tokens,
            thumb,
            entry.kind,
            &entry.thumb,
            &format!("lib:{}", entry.id),
            images,
        );
        let clip = ui.painter().with_clip_rect(rect.shrink2(vec2(2.0, 0.0)));
        let x = thumb.right() + 8.0;
        clip.text(
            pos2(x, rect.center().y - 8.0),
            Align2::LEFT_CENTER,
            &entry.name,
            FontId::proportional(13.0),
            if selected {
                tokens.text_strong
            } else {
                tokens.text
            },
        );
        clip.text(
            pos2(x, rect.center().y + 9.0),
            Align2::LEFT_CENTER,
            &entry.detail,
            FontId::proportional(11.0),
            tokens.text_weak,
        );
    }
    resp
}

/// While a library row is dragged a label follows the pointer; when it is released over a view
/// the matching request goes out: a texture is added to the model, a reference is pinned.
pub(crate) fn finish_drag(
    ctx: &Context,
    views: &[crate::request::ViewportPlacement],
    sink: &mut Sink,
) {
    let Some(drag) = egui::DragAndDrop::payload::<LibDrag>(ctx) else {
        return;
    };
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    let over_view = views.iter().any(|v| {
        Rect::from_min_size(pos2(v.rect[0], v.rect[1]), vec2(v.rect[2], v.rect[3])).contains(pos)
    });
    let usable = matches!(drag.kind, LibKind::Texture | LibKind::Reference);
    if ctx.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ctx);
        if over_view && usable {
            let request = if drag.kind == LibKind::Texture {
                LibraryRequest::UseTexture(drag.id.clone())
            } else {
                LibraryRequest::PinReference(drag.id.clone())
            };
            sink.push(UiRequest::Library(request));
        }
        return;
    }
    let tokens = Tokens::of(ctx);
    let text = if over_view && usable {
        match drag.kind {
            LibKind::Texture => format!("Add {} to the model", drag.name),
            _ => format!("Pin {}", drag.name),
        }
    } else {
        drag.name.clone()
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("cuboid_library_drag"),
    ));
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), tokens.text_strong);
    let rect = Rect::from_min_size(pos + vec2(14.0, 14.0), galley.size() + vec2(16.0, 8.0));
    painter.rect(
        rect,
        RADIUS,
        tokens.bg_raised,
        Stroke::new(1.0, tokens.border_strong),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, tokens.text_strong);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_aspect_inside_the_box() {
        let outer = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        let wide = fit(outer, 4.0);
        assert!((wide.width() - 200.0).abs() < 0.01 && (wide.height() - 50.0).abs() < 0.01);
        let tall = fit(outer, 0.5);
        assert!((tall.width() - 50.0).abs() < 0.01 && (tall.height() - 100.0).abs() < 0.01);
        assert_eq!(fit(outer, f32::NAN), outer);
        assert_eq!(fit(outer, 0.0), outer);
    }

    #[test]
    fn tags_are_split_on_commas() {
        assert_eq!(split_tags(" a, b ,,c "), ["a", "b", "c"]);
        assert!(split_tags(" , ").is_empty());
    }
}
