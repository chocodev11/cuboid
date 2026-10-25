//! Modal dialogs: the unsaved-changes question and the recovery dialog, and the two pieces every
//! dialog of the crate builds on. Modal windows are only used where the user must decide before
//! anything else happens.

use std::path::PathBuf;

use cuboid_ui_kit::metrics::{height, icon_size, radius, space};
use cuboid_ui_kit::{Button, Icon, Modal, ModalButton, Tokens, Type, text};
use egui::{
    Align, Color32, Context, Frame, Label, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2,
    vec2,
};

use crate::request::{RecoveryChoice, UiRequest};

/// Height of the list in the recovery dialog.
const RECOVERY_LIST: f32 = height::ROW_THUMB * 4.0;

/// The body of a dialog: a scroll area of a fixed `height`, so the dialog keeps its size
/// whatever the body holds.
pub(crate) fn fixed_body<R>(
    ui: &mut Ui,
    id_salt: &'static str,
    height: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ScrollArea::vertical()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .min_scrolled_height(height)
        .max_height(height)
        .show(ui, add)
        .inner
}

/// A card inside a dialog: one surface step below the dialog, padded by 8 pt.
pub(crate) fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let tokens = Tokens::of(ui.ctx());
    Frame::new()
        .fill(tokens.bg_panel)
        .corner_radius(radius::CARD)
        .inner_margin(space::SM)
        .show(ui, add)
        .inner
}

/// A 16 pt icon in `tone` at the first line of a message of one or more lines.
pub(crate) fn status_line(ui: &mut Ui, icon: Icon, tone: Color32, message: String) {
    ui.horizontal_top(|ui| {
        let size = vec2(icon_size::CONTROL, Type::Body.line_height());
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        icon.paint(ui.painter(), rect, tone);
        ui.add(Label::new(text(Type::Body, message)).wrap());
    });
}

/// One recovery file as a card: its name and when it was saved on the left, Discard and Restore
/// on the right. A long name is cut before it reaches the buttons.
fn recovery_row(ui: &mut Ui, entry: &RecoveryEntry) -> Option<RecoveryChoice> {
    let mut choice = None;
    card(ui, |ui| {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), height::ACTION), Sense::hover());
        let restore = Button::new("Restore");
        let discard = Button::new("Discard").ghost();
        let buttons = restore.natural_width(ui) + discard.natural_width(ui) + space::XS;
        let left = pos2(rect.right() - buttons, rect.top());
        let buttons_rect = Rect::from_min_max(left, rect.max);
        let text_rect = Rect::from_min_max(rect.min, pos2(left.x - space::SM, rect.bottom()));
        let mut words = ui.new_child(
            UiBuilder::new()
                .max_rect(text_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        words.add(Label::new(text(Type::Strong, &entry.name)).truncate());
        let detail = if entry.note.is_empty() {
            entry.saved.clone()
        } else {
            format!("{} \u{b7} {}", entry.saved, entry.note)
        };
        words.add(Label::new(text(Type::Caption, detail)).truncate());
        let mut actions = ui.new_child(
            UiBuilder::new()
                .max_rect(buttons_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        actions.spacing_mut().item_spacing.x = space::XS;
        if actions.add(discard).clicked() {
            choice = Some(RecoveryChoice::Discard(entry.path.clone()));
        }
        if actions.add(restore).clicked() {
            choice = Some(RecoveryChoice::Restore(entry.path.clone()));
        }
    });
    choice
}

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
    /// Draws the dialog: Enter saves, Esc cancels.
    pub fn show(&self, ctx: &Context) -> UnsavedAnswer {
        let buttons = [
            ModalButton::new("Cancel"),
            ModalButton::danger("Discard changes"),
            ModalButton::primary("Save"),
        ];
        let outcome = Modal::new("unsaved", "Unsaved changes")
            .buttons(&buttons)
            .show(ctx, |ui| {
                ui.add(
                    Label::new(text(
                        Type::Body,
                        format!("{} has changes that are not saved.", self.title),
                    ))
                    .wrap(),
                );
            });
        match outcome.button {
            Some(0) => UnsavedAnswer::Cancel,
            Some(1) => UnsavedAnswer::Discard(self.then.clone()),
            Some(_) => UnsavedAnswer::Save(self.then.clone()),
            None if outcome.confirmed => UnsavedAnswer::Save(self.then.clone()),
            None if outcome.cancelled => UnsavedAnswer::Cancel,
            None => UnsavedAnswer::Pending,
        }
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
    /// Enter and Esc decide later.
    pub fn show(&mut self, ctx: &Context) -> (Option<RecoveryChoice>, bool) {
        let mut choice = None;
        let buttons = [
            ModalButton::danger("Discard all"),
            ModalButton::new("Decide later"),
        ];
        let outcome = Modal::new("recovery", "Recover your work")
            .width(520.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                ui.add(Label::new(text(Type::Body, &self.info.message)).wrap());
                ui.add_space(space::SM);
                fixed_body(ui, "recovery_list", RECOVERY_LIST, |ui| {
                    ui.spacing_mut().item_spacing.y = space::XS;
                    for entry in &self.info.entries {
                        if let Some(picked) = recovery_row(ui, entry) {
                            choice = Some(picked);
                        }
                    }
                });
            });
        match outcome.button {
            Some(0) => choice = Some(RecoveryChoice::DiscardAll),
            Some(_) => choice = Some(RecoveryChoice::Later),
            None if choice.is_none() && (outcome.confirmed || outcome.cancelled) => {
                choice = Some(RecoveryChoice::Later);
            }
            None => {}
        }
        let mut close = false;
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

/// Installs the fonts a dialog needs before its first frame, for the tests of the dialogs.
#[cfg(test)]
pub(crate) fn prepare(ctx: &Context) {
    cuboid_ui_kit::install_fonts(ctx, &cuboid_ui_kit::FontSet::default());
}

#[cfg(test)]
mod tests {
    use egui::{Key, Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

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

    struct Asked {
        dialog: Unsaved,
        answer: UnsavedAnswer,
    }

    fn asked() -> Harness<'static, Asked> {
        let state = Asked {
            dialog: Unsaved {
                then: UiRequest::Quit,
                title: "tower.cubik".into(),
            },
            answer: UnsavedAnswer::Pending,
        };
        let mut harness = Harness::builder()
            .with_context_setup(prepare)
            .build_ui_state(
                |ui, state: &mut Asked| {
                    let answer = state.dialog.show(ui.ctx());
                    if !matches!(answer, UnsavedAnswer::Pending) {
                        state.answer = answer;
                    }
                },
                state,
            );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn enter_saves_and_escape_cancels_the_unsaved_question() {
        let mut harness = asked();
        harness.key_press(Key::Enter);
        harness.run_steps(2);
        assert!(matches!(harness.state().answer, UnsavedAnswer::Save(_)));
        let mut harness = asked();
        harness.key_press(Key::Escape);
        harness.run_steps(2);
        assert!(matches!(harness.state().answer, UnsavedAnswer::Cancel));
        let mut harness = asked();
        harness.get_by_label("Discard changes").click();
        harness.run_steps(2);
        assert!(matches!(harness.state().answer, UnsavedAnswer::Discard(_)));
    }

    #[test]
    fn a_long_recovery_name_leaves_the_buttons_in_reach() {
        let mut harness = Harness::builder()
            .with_context_setup(prepare)
            .build_ui_state(
                |ui, dialog: &mut Recovery| {
                    dialog.show(ui.ctx());
                },
                Recovery {
                    info: RecoveryInfo {
                        message: "Last time ended unexpectedly.".into(),
                        entries: vec![RecoveryEntry {
                            path: PathBuf::from("a.cubik"),
                            name: "a_rather_long_model_name_".repeat(8),
                            saved: "10:00".into(),
                            note: "saved before a crash".into(),
                        }],
                    },
                },
            );
        harness.run_steps(3);
        let restore = harness
            .get_by_role_and_label(Role::Button, "Restore")
            .rect();
        let discard = harness
            .get_by_role_and_label(Role::Button, "Discard")
            .rect();
        assert!(discard.right() <= restore.left());
        let screen = harness.ctx.content_rect();
        assert!(screen.contains_rect(restore));
    }
}
