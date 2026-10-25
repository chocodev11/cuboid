//! The two questions only the app asks: the unsaved-changes question for the window's close
//! button, and the overwrite question after the file dialog. The UI crate asks its own
//! unsaved-changes question for every request it sends, and the recovery dialog is its own.
//! Both are kit modals (ui_v2_spec 10): Esc cancels, Enter takes the last button.

use std::path::{Path, PathBuf};

use cuboid_ui_kit::metrics::space;
use cuboid_ui_kit::{Modal, ModalButton, Type, text};
use egui::Label;

use crate::state::{Dialog, Intent, State, UnsavedAnswer};

/// Draws the dialog the state holds, if it is one of the app's own, and returns the intents.
pub fn show(ctx: &egui::Context, state: &State) -> Vec<Intent> {
    let mut intents = Vec::new();
    match &state.dialog {
        Some(Dialog::Unsaved { name }) => unsaved_dialog(ctx, name, &mut intents),
        Some(Dialog::Overwrite { path, also, .. }) => {
            overwrite_dialog(ctx, path, also, &mut intents);
        }
        Some(Dialog::Recovery { .. }) | None => {}
    }
    intents
}

fn unsaved_dialog(ctx: &egui::Context, name: &str, intents: &mut Vec<Intent>) {
    let buttons = [
        ModalButton::new("Cancel"),
        ModalButton::danger("Don't save"),
        ModalButton::primary("Save"),
    ];
    let outcome = Modal::new("app_unsaved", "Unsaved changes")
        .buttons(&buttons)
        .show(ctx, |ui| {
            let question = format!("Do you want to save the changes you made to {name}?");
            ui.add(Label::new(text(Type::Body, question)).wrap());
        });
    let answer = match outcome.button {
        Some(0) => UnsavedAnswer::Cancel,
        Some(1) => UnsavedAnswer::Discard,
        Some(_) => UnsavedAnswer::Save,
        None if outcome.confirmed => UnsavedAnswer::Save,
        None if outcome.cancelled => UnsavedAnswer::Cancel,
        None => return,
    };
    intents.push(Intent::Unsaved(answer));
}

/// The most files the question names; the others are counted.
const LISTED: usize = 8;

/// Shown after the UI of the frame, so it stands above the export dialog, which is a modal too.
fn overwrite_dialog(ctx: &egui::Context, path: &Path, also: &[PathBuf], intents: &mut Vec<Intent>) {
    let buttons = [ModalButton::new("Cancel"), ModalButton::danger("Overwrite")];
    let outcome = Modal::new("app_overwrite", "Replace files")
        .width(520.0)
        .buttons(&buttons)
        .show(ctx, |ui| {
            if also.is_empty() {
                let question = format!("{} already exists. Replace it?", path.display());
                ui.add(Label::new(text(Type::Body, question)).wrap());
                return;
            }
            ui.label(text(
                Type::Body,
                "These files already exist. Overwrite them?",
            ));
            ui.add_space(space::XS);
            for file in also.iter().take(LISTED) {
                let shown = file.display().to_string();
                ui.add(Label::new(text(Type::Label, shown)).wrap());
            }
            if also.len() > LISTED {
                let more = format!("and {} more", also.len() - LISTED);
                ui.label(text(Type::Label, more));
            }
        });
    // Enter does nothing here: the only confirming answer destroys files, so it takes a click.
    let replace = match outcome.button {
        Some(0) => false,
        Some(_) => true,
        None if outcome.cancelled => false,
        None => return,
    };
    intents.push(Intent::Overwrite(replace));
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use cuboid_settings::{Environment, Settings};
    use cuboid_ui_kit::{FontSet, install_fonts};
    use egui::{Event, Key, Modifiers, RawInput};

    use super::*;
    use crate::io_thread::IoThread;
    use crate::platform::Dirs;

    fn state() -> (tempfile::TempDir, State) {
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
        (dir, state)
    }

    /// Runs a few passes so the modal is on top, then one more with `key` pressed. Returns the
    /// intents of that last pass.
    fn frames(state: &State, key: Option<Key>) -> Vec<Intent> {
        let ctx = egui::Context::default();
        install_fonts(&ctx, &FontSet::default());
        let pass = |events: Vec<Event>| {
            ctx.begin_pass(RawInput {
                events,
                ..RawInput::default()
            });
            let intents = show(&ctx, state);
            ctx.end_pass().textures_delta.clear();
            intents
        };
        for _ in 0..3 {
            pass(Vec::new());
        }
        let events = key
            .map(|key| Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
            .into_iter()
            .collect();
        pass(events)
    }

    fn unsaved(state: &mut State) {
        state.dialog = Some(Dialog::Unsaved {
            name: "model".to_owned(),
        });
    }

    fn overwrite(state: &mut State, files: usize) {
        state.dialog = Some(Dialog::Overwrite {
            path: "model.cubik".into(),
            also: vec!["a.png".into(); files],
            purpose: crate::flows::PickPurpose::Export,
            plan: None,
        });
    }

    #[test]
    fn no_dialog_asks_for_nothing() {
        let (_dir, state) = state();
        assert!(frames(&state, None).is_empty());
        assert!(frames(&state, Some(Key::Escape)).is_empty());
    }

    #[test]
    fn the_questions_build_without_panicking() {
        let (_dir, mut state) = state();
        unsaved(&mut state);
        assert!(frames(&state, None).is_empty());
        overwrite(&mut state, 12);
        assert!(frames(&state, None).is_empty());
    }

    #[test]
    fn the_unsaved_question_saves_on_enter_and_cancels_on_escape() {
        let (_dir, mut state) = state();
        unsaved(&mut state);
        assert_eq!(
            frames(&state, Some(Key::Enter)),
            [Intent::Unsaved(UnsavedAnswer::Save)]
        );
        assert_eq!(
            frames(&state, Some(Key::Escape)),
            [Intent::Unsaved(UnsavedAnswer::Cancel)]
        );
    }

    #[test]
    fn the_overwrite_question_ignores_enter_and_keeps_the_files_on_escape() {
        let (_dir, mut state) = state();
        for files in [0, 3] {
            overwrite(&mut state, files);
            assert!(
                frames(&state, Some(Key::Enter)).is_empty(),
                "Enter never overwrites"
            );
            assert_eq!(
                frames(&state, Some(Key::Escape)),
                [Intent::Overwrite(false)]
            );
        }
    }
}
