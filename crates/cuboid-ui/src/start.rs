//! The start screen: new models and Open on the left, recent files and suggestions in the
//! middle, the library and the agent brief on the right, and a strip with the camera preset, the
//! theme, the UI scale and the version at the bottom.

use std::path::PathBuf;

use cuboid_ui_kit::{Icon, RADIUS, RADIUS_LG, Tokens, search_field};
use egui::{
    Align, Align2, Color32, FontId, Layout, Panel, Rect, Response, Role, Sense, Stroke, StrokeKind,
    Ui, WidgetInfo, pos2, vec2,
};

use crate::common::ImageCache;
use crate::library::{
    LibKind, LibraryRequest, LibraryView, ThumbState, choice_row, paint_thumb, scope_switch,
    split_tags,
};
use crate::model::{RecentEntry, UiCx};
use crate::prefs::{ColorMode, NavPreset, Prefs};
use crate::request::Template;

const CARD_W: f32 = 108.0;
const CARD_H: f32 = 96.0;
const RECENT_W: f32 = 168.0;
const RECENT_H: f32 = 140.0;
const LEFT_W: f32 = 232.0;
const RIGHT_W: f32 = 272.0;
const GAP: f32 = 16.0;
/// Below this width the three columns stack.
const WIDE: f32 = 880.0;
const MAX_RECENT: usize = 48;
/// Resolutions offered for a voxel model.
const VOXEL_RESOLUTIONS: [u32; 4] = [16, 32, 64, 128];
const SCALES: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];
const SCALE_LABELS: [&str; 5] = ["75%", "100%", "125%", "150%", "200%"];
const MODES: [ColorMode; 3] = [ColorMode::Dark, ColorMode::Light, ColorMode::System];
const MODE_LABELS: [&str; 3] = ["Dark", "Light", "System"];
const NAV_LABELS: [&str; 2] = ["Blockbench", "Blender"];

/// What the user chose.
pub(crate) enum StartEvent {
    New(Template),
    Open(PathBuf),
    OpenDialog,
    RemoveRecent(PathBuf),
    Preferences,
    /// The answer of the first-run camera question, or the preset of the bottom strip.
    Nav(NavPreset),
    /// The accent chosen in the first-run panel.
    Accent([u8; 3]),
    /// The first-run panel is done.
    Continue,
    Theme(ColorMode),
    Scale(f32),
    Library(LibraryRequest),
    /// Leave the start screen (only when a document is open).
    Close,
}

pub(crate) struct StartState {
    resolution_index: usize,
    filter: String,
    brief_name: String,
    brief_version: String,
    brief_tags: String,
    brief_rev: Option<u64>,
}

impl Default for StartState {
    fn default() -> Self {
        Self {
            resolution_index: 1,
            filter: String::new(),
            brief_name: String::new(),
            brief_version: String::new(),
            brief_tags: String::new(),
            brief_rev: None,
        }
    }
}

impl StartState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        prefs: &Prefs,
        can_close: bool,
        ask_nav: bool,
    ) -> Vec<StartEvent> {
        let tokens = Tokens::of(ui.ctx());
        let mut events = Vec::new();
        Panel::bottom("cuboid_start_strip")
            .frame(
                egui::Frame::new()
                    .fill(tokens.bg_panel)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0, tokens.border)),
            )
            .show(ui, |ui| strip(ui, &tokens, prefs, &mut events));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width().min(1280.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new("Cuboid")
                            .size(32.0)
                            .strong()
                            .color(tokens.text_strong),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Cuboid and voxel modelling for Minecraft and pixel art",
                        )
                        .color(tokens.text_weak),
                    );
                    ui.add_space(16.0);
                });
                if ask_nav {
                    first_run(ui, &tokens, prefs, &mut events);
                }
                ui.horizontal_top(|ui| {
                    ui.add_space(((ui.available_width() - width) / 2.0).max(8.0));
                    ui.vertical(|ui| {
                        ui.set_width(width - 16.0);
                        self.columns(ui, &tokens, cx, images, can_close, &mut events);
                    });
                });
                ui.add_space(24.0);
            });
        events
    }

    fn columns(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        can_close: bool,
        events: &mut Vec<StartEvent>,
    ) {
        let total = ui.available_width();
        if total >= WIDE {
            let middle = total - LEFT_W - RIGHT_W - 2.0 * GAP;
            ui.horizontal_top(|ui| {
                column(ui, LEFT_W, |ui| self.left(ui, tokens, can_close, events));
                ui.add_space(GAP);
                column(ui, middle, |ui| self.middle(ui, tokens, cx, images, events));
                ui.add_space(GAP);
                column(ui, RIGHT_W, |ui| self.right(ui, tokens, cx, events));
            });
        } else {
            self.left(ui, tokens, can_close, events);
            ui.add_space(GAP);
            self.middle(ui, tokens, cx, images, events);
            ui.add_space(GAP);
            self.right(ui, tokens, cx, events);
        }
    }

    fn left(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        can_close: bool,
        events: &mut Vec<StartEvent>,
    ) {
        heading(ui, tokens, "New");
        let voxel = VOXEL_RESOLUTIONS[self.resolution_index.min(VOXEL_RESOLUTIONS.len() - 1)];
        ui.horizontal(|ui| {
            if card(ui, tokens, Icon::Cube, "Block", "Full cube") {
                events.push(StartEvent::New(Template::Block));
            }
            if card(ui, tokens, Icon::Plane, "Item", "Flat model") {
                events.push(StartEvent::New(Template::Item));
            }
        });
        ui.horizontal(|ui| {
            if card(
                ui,
                tokens,
                Icon::Voxel,
                "Voxel",
                &format!("{voxel} per block"),
            ) {
                events.push(StartEvent::New(Template::Voxel { resolution: voxel }));
            }
            if card(ui, tokens, Icon::File, "Empty", "Nothing in it") {
                events.push(StartEvent::New(Template::Empty));
            }
        });
        ui.horizontal(|ui| {
            if card(ui, tokens, Icon::Cube, "Cubik classic", "Cubik rules") {
                events.push(StartEvent::New(Template::CubikClassic));
            }
        });
        let labels: Vec<String> = VOXEL_RESOLUTIONS.iter().map(u32::to_string).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        ui.label(
            egui::RichText::new("Voxels per block")
                .size(12.0)
                .color(tokens.text_weak),
        );
        choice_row(ui, &mut self.resolution_index, &refs);
        ui.add_space(12.0);
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Open a file...").color(tokens.on_accent))
                    .min_size(vec2(LEFT_W, 32.0))
                    .fill(tokens.accent)
                    .stroke(Stroke::NONE),
            )
            .clicked()
        {
            events.push(StartEvent::OpenDialog);
        }
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new("Preferences").min_size(vec2(110.0, 28.0)))
                .clicked()
            {
                events.push(StartEvent::Preferences);
            }
            if can_close
                && ui
                    .add(egui::Button::new("Back to the model").min_size(vec2(116.0, 28.0)))
                    .clicked()
            {
                events.push(StartEvent::Close);
            }
        });
        ui.add_space(12.0);
        drop_hint(ui, tokens);
    }

    fn middle(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        events: &mut Vec<StartEvent>,
    ) {
        heading(ui, tokens, "Recent files");
        search_field(ui, "start_filter", &mut self.filter, "Filter recent files");
        ui.add_space(6.0);
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
            ui.label(
                egui::RichText::new("Files you open or save will show up here.")
                    .color(tokens.text_weak),
            );
        } else if shown.is_empty() {
            ui.label(egui::RichText::new("No recent file matches.").color(tokens.text_weak));
        }
        ui.horizontal_wrapped(|ui| {
            for (entry, pin) in shown.into_iter().take(MAX_RECENT) {
                let (resp, pin_clicked) = recent_card(ui, tokens, entry, pin, images);
                if pin_clicked {
                    events.push(StartEvent::Library(LibraryRequest::TogglePin(
                        entry.path.clone(),
                    )));
                } else if resp.clicked() {
                    events.push(StartEvent::Open(entry.path.clone()));
                }
                resp.context_menu(|ui| {
                    let label = if pin { "Unpin" } else { "Pin to the top" };
                    if ui.button(label).clicked() {
                        events.push(StartEvent::Library(LibraryRequest::TogglePin(
                            entry.path.clone(),
                        )));
                        ui.close();
                    }
                    if ui.button("Remove from the list").clicked() {
                        events.push(StartEvent::RemoveRecent(entry.path.clone()));
                        ui.close();
                    }
                });
            }
        });
        ui.add_space(16.0);
        suggestions(ui, tokens, cx.library);
    }

    fn right(&mut self, ui: &mut Ui, tokens: &Tokens, cx: &UiCx<'_>, events: &mut Vec<StartEvent>) {
        heading(ui, tokens, "Library");
        let Some(view) = cx.library else {
            ui.label(egui::RichText::new("The library is not available.").color(tokens.text_weak));
            return;
        };
        let mut sink = crate::common::Sink::default();
        scope_switch(ui, view, &mut sink);
        events.extend(sink.requests.into_iter().filter_map(|r| match r {
            crate::request::UiRequest::Library(request) => Some(StartEvent::Library(request)),
            _ => None,
        }));
        ui.add_space(4.0);
        for kind in LibKind::ALL {
            let count = view.counts[LibKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)];
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(kind.plural()).color(tokens.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new(count.to_string()).color(tokens.text_strong));
                });
            });
        }
        if view.busy {
            ui.label(
                egui::RichText::new("Working...")
                    .size(12.0)
                    .color(tokens.text_weak),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!view.busy, egui::Button::new("Import folder..."))
                .clicked()
            {
                events.push(StartEvent::Library(LibraryRequest::ImportFolder));
            }
            if ui
                .add_enabled(!view.busy, egui::Button::new("Import files..."))
                .clicked()
            {
                events.push(StartEvent::Library(LibraryRequest::ImportFiles));
            }
        });
        ui.add_space(16.0);
        self.brief_card(ui, tokens, view, events);
    }

    fn brief_card(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        view: &LibraryView,
        events: &mut Vec<StartEvent>,
    ) {
        heading(ui, tokens, "Agent brief");
        let brief = &view.brief;
        card_frame(tokens).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut focused = false;
            let mut committed = false;
            for (label, text, hint) in [
                ("Project", &mut self.brief_name, "Name of the project"),
                ("Minecraft", &mut self.brief_version, "1.21.6"),
                ("Style", &mut self.brief_tags, "pixel, low-poly"),
            ] {
                ui.label(
                    egui::RichText::new(label)
                        .size(12.0)
                        .color(tokens.text_weak),
                );
                let resp = ui.add_enabled(
                    !view.busy,
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY),
                );
                focused |= resp.has_focus();
                committed |= resp.lost_focus();
            }
            if committed {
                events.push(StartEvent::Library(LibraryRequest::SetBrief {
                    name: self.brief_name.clone(),
                    mc_version: self.brief_version.clone(),
                    style_tags: split_tags(&self.brief_tags),
                }));
            }
            if !focused && self.brief_rev != Some(view.rev) {
                self.brief_rev = Some(view.rev);
                self.brief_name.clone_from(&brief.project_name);
                self.brief_version.clone_from(&brief.mc_version);
                self.brief_tags = brief.style_tags.join(", ");
            }
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Shared palette")
                    .size(12.0)
                    .color(tokens.text_weak),
            );
            palette_strip(ui, tokens, &brief.palette);
            let density = brief.density.map_or_else(
                || "no measure yet".to_owned(),
                |d| format!("{d:.0} texels per block"),
            );
            ui.label(
                egui::RichText::new(format!(
                    "Density: {density}\n{} models, {} textures",
                    brief.models, brief.textures
                ))
                .size(12.0)
                .color(tokens.text),
            );
        });
    }
}

fn column(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, 10.0), Layout::top_down(Align::Min), |ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        add(ui);
    });
}

fn card_frame(tokens: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(tokens.bg_raised)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(RADIUS_LG)
        .inner_margin(egui::Margin::same(10))
}

/// The first-run panel: the camera preset and the accent colour, then Continue.
fn first_run(ui: &mut Ui, tokens: &Tokens, prefs: &Prefs, events: &mut Vec<StartEvent>) {
    ui.vertical_centered(|ui| {
        card_frame(tokens).show(ui, |ui| {
            ui.set_width(460.0);
            heading(ui, tokens, "How do you move the camera?");
            for preset in NavPreset::ALL {
                let on = prefs.nav_chosen && prefs.nav == preset;
                let label = egui::RichText::new(preset.name()).color(tokens.text_strong);
                if ui
                    .add(
                        egui::Button::new(label)
                            .selected(on)
                            .min_size(vec2(440.0, 32.0)),
                    )
                    .clicked()
                {
                    events.push(StartEvent::Nav(preset));
                }
            }
            ui.add_space(10.0);
            heading(ui, tokens, "Pick an accent colour");
            let theme = prefs.theme();
            let system_dark = ui.ctx().system_theme().map(|t| t == egui::Theme::Dark);
            let presets = theme.accent_presets(theme.is_dark(system_dark));
            ui.horizontal(|ui| {
                for (preset, name) in presets.into_iter().zip(prefs.family.accent_names()) {
                    let on = prefs.accent_chosen && prefs.accent == preset;
                    if swatch(ui, tokens, preset, name, on).clicked() {
                        events.push(StartEvent::Accent(preset));
                    }
                }
            });
            ui.label(
                egui::RichText::new("You can change both later in Preferences.")
                    .size(12.0)
                    .color(tokens.text_weak),
            );
            ui.add_space(8.0);
            let ready = prefs.nav_chosen && prefs.accent_chosen;
            let label = egui::RichText::new("Continue").color(if ready {
                tokens.on_accent
            } else {
                tokens.text_weak
            });
            let button = egui::Button::new(label)
                .min_size(vec2(440.0, 32.0))
                .fill(if ready {
                    tokens.accent
                } else {
                    tokens.bg_input
                });
            if ui.add_enabled(ready, button).clicked() {
                events.push(StartEvent::Continue);
            }
        });
        ui.add_space(12.0);
    });
}

/// A round colour choice with its name as the accessible label.
fn swatch(ui: &mut Ui, tokens: &Tokens, color: [u8; 3], name: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, name));
    let painter = ui.painter();
    let fill = Color32::from_rgb(color[0], color[1], color[2]);
    painter.circle_filled(rect.center(), 15.0, fill);
    if selected {
        painter.circle_stroke(rect.center(), 19.0, Stroke::new(2.0, tokens.text_strong));
    } else if resp.hovered() {
        painter.circle_stroke(rect.center(), 18.0, Stroke::new(1.0, tokens.border_hover));
    }
    resp.on_hover_text(name)
}

fn strip(ui: &mut Ui, tokens: &Tokens, prefs: &Prefs, events: &mut Vec<StartEvent>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let weak = |text: &str| {
            egui::RichText::new(text.to_owned())
                .size(12.0)
                .color(tokens.text_weak)
        };
        ui.label(weak("Camera"));
        let mut nav = NavPreset::ALL
            .iter()
            .position(|p| *p == prefs.nav)
            .unwrap_or(0);
        if choice_row(ui, &mut nav, &NAV_LABELS) {
            events.push(StartEvent::Nav(NavPreset::ALL[nav]));
        }
        ui.label(weak("Theme"));
        let mut mode = MODES
            .iter()
            .position(|m| *m == prefs.color_mode)
            .unwrap_or(0);
        if choice_row(ui, &mut mode, &MODE_LABELS) {
            events.push(StartEvent::Theme(MODES[mode]));
        }
        ui.label(weak("Scale"));
        let mut scale = SCALES
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1 - prefs.ui_scale)
                    .abs()
                    .total_cmp(&(b.1 - prefs.ui_scale).abs())
            })
            .map_or(1, |(i, _)| i);
        if choice_row(ui, &mut scale, &SCALE_LABELS) {
            events.push(StartEvent::Scale(SCALES[scale]));
        }
        ui.add_space(16.0);
        ui.label(weak(&format!("Cuboid {}", env!("CARGO_PKG_VERSION"))));
    });
}

fn heading(ui: &mut Ui, tokens: &Tokens, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(16.0)
            .strong()
            .color(tokens.text_strong),
    );
    ui.add_space(4.0);
}

fn drop_hint(ui: &mut Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(LEFT_W, 56.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        RADIUS_LG,
        Stroke::new(1.0, tokens.border_strong),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "Drop a .cubik, .cub or .bbmodel\nfile anywhere to open it",
        FontId::proportional(12.0),
        tokens.text_weak,
    );
}

fn suggestions(ui: &mut Ui, tokens: &Tokens, library: Option<&LibraryView>) {
    heading(ui, tokens, "Suggestions");
    card_frame(tokens).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let list = library.map_or(&[][..], |lib| &lib.suggestions[..]);
        if list.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Import models, textures and lore notes. Hints about missing models, \
                     colours and texel density show up here.",
                )
                .size(12.0)
                .color(tokens.text_weak),
            );
        }
        for item in list.iter().take(8) {
            ui.label(egui::RichText::new(&item.title).color(tokens.text_strong));
            ui.label(
                egui::RichText::new(&item.detail)
                    .size(12.0)
                    .color(tokens.text_weak),
            );
            ui.add_space(4.0);
        }
        if list.len() > 8 {
            ui.label(
                egui::RichText::new(format!("{} more", list.len() - 8))
                    .size(12.0)
                    .color(tokens.text_weak),
            );
        }
    });
}

fn palette_strip(ui: &mut Ui, tokens: &Tokens, palette: &[[u8; 3]]) {
    if palette.is_empty() {
        ui.label(
            egui::RichText::new("none yet")
                .size(12.0)
                .color(tokens.text_weak),
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for [r, g, b] in palette {
            let (rect, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            ui.painter().rect(
                rect,
                RADIUS,
                Color32::from_rgb(*r, *g, *b),
                Stroke::new(1.0, tokens.border_strong),
                StrokeKind::Inside,
            );
            resp.on_hover_text(format!("#{r:02x}{g:02x}{b:02x}"));
        }
    });
}

fn card_surface(ui: &mut Ui, tokens: &Tokens, size: egui::Vec2, label: &str) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let (fill, stroke) = if resp.hovered() {
            (tokens.bg_hover, tokens.border_hover)
        } else {
            (tokens.bg_raised, tokens.border_strong)
        };
        ui.painter().rect(
            rect,
            RADIUS_LG,
            fill,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }
    (rect, resp)
}

fn card(ui: &mut Ui, tokens: &Tokens, icon: Icon, title: &str, detail: &str) -> bool {
    let (rect, resp) = card_surface(ui, tokens, vec2(CARD_W, CARD_H), title);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        icon.paint(
            painter,
            Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(32.0, 32.0)),
            tokens.accent,
        );
        painter.text(
            rect.min + vec2(12.0, 60.0),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(15.0),
            tokens.text_strong,
        );
        painter.text(
            rect.min + vec2(12.0, 80.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(11.5),
            tokens.text_weak,
        );
    }
    resp.clicked()
}

/// A recent file. The second value says the pin mark was clicked.
fn recent_card(
    ui: &mut Ui,
    tokens: &Tokens,
    entry: &RecentEntry,
    pinned: bool,
    images: &mut ImageCache,
) -> (Response, bool) {
    let (rect, resp) = card_surface(ui, tokens, vec2(RECENT_W, RECENT_H), &entry.name);
    let pin_rect = Rect::from_min_size(rect.right_top() + vec2(-26.0, 8.0), vec2(18.0, 18.0));
    let pin = ui.interact(pin_rect, resp.id.with("pin"), Sense::click());
    pin.widget_info(|| {
        WidgetInfo::selected(Role::Button, true, pinned, format!("Pin {}", entry.name))
    });
    if ui.is_rect_visible(rect) {
        let thumb = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(RECENT_W - 16.0, 90.0));
        let state = entry
            .thumbnail
            .clone()
            .map_or(ThumbState::Missing, ThumbState::Ready);
        paint_thumb(
            ui,
            tokens,
            thumb,
            LibKind::Model,
            &state,
            &format!("recent:{}", entry.path.display()),
            images,
        );
        let painter = ui.painter();
        let centre = pin_rect.center();
        if pinned {
            painter.circle_filled(centre, 5.0, tokens.accent);
        } else if resp.hovered() || pin.hovered() {
            painter.circle_stroke(centre, 5.0, Stroke::new(1.5, tokens.text_weak));
        }
        let clip = painter.with_clip_rect(Rect::from_min_max(
            rect.min + vec2(8.0, 0.0),
            rect.max - vec2(8.0, 0.0),
        ));
        clip.text(
            pos2(rect.left() + 10.0, rect.top() + 110.0),
            Align2::LEFT_CENTER,
            &entry.name,
            FontId::proportional(13.0),
            tokens.text_strong,
        );
        clip.text(
            pos2(rect.left() + 10.0, rect.top() + 127.0),
            Align2::LEFT_CENTER,
            &entry.detail,
            FontId::proportional(11.0),
            tokens.text_weak,
        );
    }
    let clicked = pin.clicked();
    (
        resp.on_hover_text(entry.path.display().to_string()),
        clicked,
    )
}

/// The largest rectangle of the given aspect ratio centred in `outer`.
pub(crate) fn fit(outer: Rect, aspect: f32) -> Rect {
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
}
