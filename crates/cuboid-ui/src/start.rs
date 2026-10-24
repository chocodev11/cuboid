//! The start screen: new model templates and the recent files.

use std::path::PathBuf;

use cuboid_ui_kit::{Icon, RADIUS, RADIUS_LG, Tokens, segmented};
use egui::{
    Align2, FontId, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2,
};

use crate::common::ImageCache;
use crate::model::UiCx;
use crate::request::Template;

const CARD_W: f32 = 210.0;
const CARD_H: f32 = 118.0;
const RECENT_W: f32 = 200.0;
const RECENT_H: f32 = 156.0;
/// Resolutions offered for a voxel model.
const VOXEL_RESOLUTIONS: [u32; 4] = [16, 32, 64, 128];

/// What the user chose.
pub(crate) enum StartEvent {
    New(Template),
    Open(PathBuf),
    OpenDialog,
    RemoveRecent(PathBuf),
    Preferences,
    /// Leave the start screen (only when a document is open).
    Close,
}

pub(crate) struct StartState {
    resolution_index: usize,
}

impl Default for StartState {
    fn default() -> Self {
        Self {
            resolution_index: 1,
        }
    }
}

impl StartState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        images: &mut ImageCache,
        can_close: bool,
    ) -> Vec<StartEvent> {
        let tokens = Tokens::of(ui.ctx());
        let mut events = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(900.0);
                ui.add_space(36.0);
                ui.label(egui::RichText::new("Cuboid").size(34.0).strong().color(tokens.text_strong));
                ui.label(egui::RichText::new("Cuboid and voxel modelling for Minecraft and pixel art").color(tokens.text_weak));
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Open a file...").color(tokens.on_accent)).min_size(vec2(128.0, 32.0)).fill(tokens.accent).stroke(Stroke::NONE)).clicked() {
                        events.push(StartEvent::OpenDialog);
                    }
                    if ui.add(egui::Button::new("Preferences").min_size(vec2(112.0, 32.0))).clicked() {
                        events.push(StartEvent::Preferences);
                    }
                    if can_close && ui.add(egui::Button::new("Back to the model").min_size(vec2(128.0, 32.0))).clicked() {
                        events.push(StartEvent::Close);
                    }
                });
                ui.add_space(24.0);

                heading(ui, &tokens, "New model");
                ui.horizontal_wrapped(|ui| {
                    if card(ui, &tokens, Icon::Cube, "Block", "A full cube to carve from") {
                        events.push(StartEvent::New(Template::Block));
                    }
                    if card(ui, &tokens, Icon::Plane, "Item", "A flat model for inventories") {
                        events.push(StartEvent::New(Template::Item));
                    }
                    let voxel = VOXEL_RESOLUTIONS[self.resolution_index.min(VOXEL_RESOLUTIONS.len() - 1)];
                    ui.vertical(|ui| {
                        if card(ui, &tokens, Icon::Voxel, "Voxel model", &format!("{voxel} voxels per block")) {
                            events.push(StartEvent::New(Template::Voxel { resolution: voxel }));
                        }
                        let labels: Vec<String> = VOXEL_RESOLUTIONS.iter().map(u32::to_string).collect();
                        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                        segmented(ui, &mut self.resolution_index, &refs);
                    });
                });
                ui.add_space(24.0);

                heading(ui, &tokens, "Recent files");
                if cx.recent.is_empty() {
                    ui.label(egui::RichText::new("Files you open or save will show up here. You can also drop a file anywhere in this window.").color(tokens.text_weak));
                }
                ui.horizontal_wrapped(|ui| {
                    for entry in cx.recent.iter().take(24) {
                        let resp = recent_card(ui, &tokens, entry, images);
                        if resp.clicked() {
                            events.push(StartEvent::Open(entry.path.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Remove from list").clicked() {
                                events.push(StartEvent::RemoveRecent(entry.path.clone()));
                                ui.close();
                            }
                        });
                    }
                });
                ui.add_space(32.0);
            });
        });
        events
    }
}

fn heading(ui: &mut Ui, tokens: &Tokens, text: &str) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(16.0)
                .strong()
                .color(tokens.text_strong),
        );
    });
    ui.add_space(4.0);
}

fn card_frame(ui: &mut Ui, tokens: &Tokens, size: egui::Vec2, label: &str) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let fill = if resp.hovered() {
            tokens.bg_hover
        } else {
            tokens.bg_raised
        };
        let stroke = if resp.hovered() {
            Stroke::new(1.0, tokens.border_hover)
        } else {
            Stroke::new(1.0, tokens.border_strong)
        };
        ui.painter()
            .rect(rect, RADIUS_LG, fill, stroke, StrokeKind::Inside);
    }
    (rect, resp)
}

fn card(ui: &mut Ui, tokens: &Tokens, icon: Icon, title: &str, detail: &str) -> bool {
    let (rect, resp) = card_frame(ui, tokens, vec2(CARD_W, CARD_H), title);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        icon.paint(
            painter,
            Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(36.0, 36.0)),
            tokens.accent,
        );
        painter.text(
            rect.min + vec2(16.0, 68.0),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(15.0),
            tokens.text_strong,
        );
        painter.text(
            rect.min + vec2(16.0, 90.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(12.0),
            tokens.text_weak,
        );
    }
    resp.clicked()
}

fn recent_card(
    ui: &mut Ui,
    tokens: &Tokens,
    entry: &crate::model::RecentEntry,
    images: &mut ImageCache,
) -> Response {
    let (rect, resp) = card_frame(ui, tokens, vec2(RECENT_W, RECENT_H), &entry.name);
    if ui.is_rect_visible(rect) {
        let thumb = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(RECENT_W - 16.0, 94.0));
        let painter = ui.painter();
        painter.rect_filled(thumb, RADIUS, tokens.bg_input);
        let drawn = entry.thumbnail.as_ref().and_then(|image| {
            let id =
                images.texture(ui.ctx(), &format!("recent:{}", entry.path.display()), image)?;
            let [w, h] = image.size;
            #[allow(clippy::cast_precision_loss)]
            let aspect = w as f32 / h.max(1) as f32;
            let fitted = fit(thumb, aspect);
            painter.image(
                id,
                fitted,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            Some(())
        });
        if drawn.is_none() {
            Icon::Cube.paint(
                painter,
                Rect::from_center_size(thumb.center(), vec2(32.0, 32.0)),
                tokens.text_weak,
            );
        }
        let clip = painter.with_clip_rect(Rect::from_min_max(
            rect.min + vec2(8.0, 0.0),
            rect.max - vec2(8.0, 0.0),
        ));
        clip.text(
            pos2(rect.left() + 10.0, rect.top() + 116.0),
            Align2::LEFT_CENTER,
            &entry.name,
            FontId::proportional(13.5),
            tokens.text_strong,
        );
        clip.text(
            pos2(rect.left() + 10.0, rect.top() + 136.0),
            Align2::LEFT_CENTER,
            &entry.detail,
            FontId::proportional(11.5),
            tokens.text_weak,
        );
    }
    resp.on_hover_text(entry.path.display().to_string())
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
