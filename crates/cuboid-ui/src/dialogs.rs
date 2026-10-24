//! Modal dialogs: the unsaved-changes question and the recovery dialog. Modal windows are only
//! used where the user must decide before anything else happens.

use std::path::PathBuf;

use cuboid_ui_kit::{Icon, RADIUS_LG, Tokens};
use egui::{Context, Id, Modal, Stroke, vec2};

use crate::request::{RecoveryChoice, UiRequest};

/// A request waiting for the answer of the unsaved-changes dialog.
pub(crate) struct Unsaved {
    /// What the user asked for.
    pub then: UiRequest,
    /// Name of the document, for the question.
    pub title: String,
}

/// The answer of the unsaved-changes dialog.
pub(crate) enum UnsavedAnswer {
    /// Still open.
    Pending,
    /// Save first, then run the request.
    Save(UiRequest),
    /// Run the request and lose the changes.
    Discard(UiRequest),
    /// Do nothing.
    Cancel,
}

impl Unsaved {
    pub fn show(&self, ctx: &Context) -> UnsavedAnswer {
        let tokens = Tokens::of(ctx);
        let mut answer = UnsavedAnswer::Pending;
        let modal = Modal::new(Id::unique("cuboid_unsaved_dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                Icon::Warning.paint(ui.painter(), rect, tokens.warn);
                ui.label(
                    egui::RichText::new("Unsaved changes")
                        .size(16.0)
                        .strong()
                        .color(tokens.text_strong),
                );
            });
            ui.add_space(8.0);
            ui.label(format!("{} has changes that are not saved.", self.title));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Save").color(tokens.on_accent))
                            .fill(tokens.accent)
                            .stroke(Stroke::NONE)
                            .min_size(vec2(96.0, 28.0)),
                    )
                    .clicked()
                {
                    answer = UnsavedAnswer::Save(self.then.clone());
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Discard changes").color(tokens.danger),
                        )
                        .min_size(vec2(128.0, 28.0)),
                    )
                    .clicked()
                {
                    answer = UnsavedAnswer::Discard(self.then.clone());
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(vec2(80.0, 28.0)))
                    .clicked()
                {
                    answer = UnsavedAnswer::Cancel;
                }
            });
        });
        if matches!(answer, UnsavedAnswer::Pending) && modal.should_close() {
            answer = UnsavedAnswer::Cancel;
        }
        answer
    }
}

/// One recovery file.
#[derive(Clone, Debug)]
pub struct RecoveryEntry {
    /// Where the recovery file is.
    pub path: PathBuf,
    /// Name of the model it came from.
    pub name: String,
    /// When it was saved, as text.
    pub saved: String,
    /// Extra line, such as "saved before a crash".
    pub note: String,
}

/// What the recovery dialog shows.
#[derive(Clone, Debug)]
pub struct RecoveryInfo {
    /// Why the dialog is open, for example "Cuboid did not close properly last time."
    pub message: String,
    /// The files to offer, newest first.
    pub entries: Vec<RecoveryEntry>,
}

/// The recovery dialog; the entries shrink as files are restored or discarded.
pub(crate) struct Recovery {
    pub info: RecoveryInfo,
}

impl Recovery {
    /// Draws the dialog. Returns the choice made this frame and whether to close the dialog.
    pub fn show(&mut self, ctx: &Context) -> (Option<RecoveryChoice>, bool) {
        let tokens = Tokens::of(ctx);
        let mut choice = None;
        let mut close = false;
        let modal = Modal::new(Id::unique("cuboid_recovery_dialog")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                Icon::History.paint(ui.painter(), rect, tokens.accent);
                ui.label(
                    egui::RichText::new("Recover your work")
                        .size(16.0)
                        .strong()
                        .color(tokens.text_strong),
                );
            });
            ui.add_space(8.0);
            ui.label(&self.info.message);
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for entry in &self.info.entries {
                        egui::Frame::new()
                            .fill(tokens.bg_panel)
                            .stroke(Stroke::new(1.0, tokens.border))
                            .corner_radius(RADIUS_LG)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(&entry.name).strong());
                                        ui.label(
                                            egui::RichText::new(&entry.saved)
                                                .color(tokens.text_weak),
                                        );
                                        if !entry.note.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&entry.note)
                                                    .color(tokens.text_weak),
                                            );
                                        }
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Restore")
                                                            .color(tokens.on_accent),
                                                    )
                                                    .fill(tokens.accent)
                                                    .stroke(Stroke::NONE),
                                                )
                                                .clicked()
                                            {
                                                choice = Some(RecoveryChoice::Restore(
                                                    entry.path.clone(),
                                                ));
                                            }
                                            if ui.button("Discard").clicked() {
                                                choice = Some(RecoveryChoice::Discard(
                                                    entry.path.clone(),
                                                ));
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Discard all").clicked() {
                    choice = Some(RecoveryChoice::DiscardAll);
                }
                if ui.button("Decide later").clicked() {
                    choice = Some(RecoveryChoice::Later);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(RecoveryChoice::Later);
        }
        match &choice {
            Some(RecoveryChoice::Restore(path) | RecoveryChoice::Discard(path)) => {
                self.info.entries.retain(|e| e.path != *path);
                close = self.info.entries.is_empty()
                    || matches!(choice, Some(RecoveryChoice::Restore(_)));
            }
            Some(RecoveryChoice::DiscardAll | RecoveryChoice::Later) => close = true,
            None => {}
        }
        (choice, close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &Context, f: impl FnMut(&Context)) {
        ctx.run_pass(egui::RawInput::default(), f)
            .drop_without_applying_deltas();
    }

    #[test]
    fn unsaved_dialog_is_pending_until_answered() {
        let ctx = Context::default();
        let dialog = Unsaved {
            then: UiRequest::Quit,
            title: "tower.cubik".into(),
        };
        let mut answer = UnsavedAnswer::Cancel;
        run(&ctx, |ctx| answer = dialog.show(ctx));
        assert!(matches!(answer, UnsavedAnswer::Pending));
    }

    #[test]
    fn recovery_dialog_lists_entries_and_stays_open() {
        let ctx = Context::default();
        let mut dialog = Recovery {
            info: RecoveryInfo {
                message: "Last time ended unexpectedly.".into(),
                entries: vec![RecoveryEntry {
                    path: PathBuf::from("a.cubik"),
                    name: "a".into(),
                    saved: "10:00".into(),
                    note: String::new(),
                }],
            },
        };
        let mut result = (Some(RecoveryChoice::DiscardAll), true);
        run(&ctx, |ctx| result = dialog.show(ctx));
        assert_eq!(result, (None, false));
        assert_eq!(dialog.info.entries.len(), 1);
    }
}
