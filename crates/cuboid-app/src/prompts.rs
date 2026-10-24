//! The two questions only the app asks: the unsaved-changes question for the window's close
//! button, and the overwrite question after the file dialog. The UI crate asks its own
//! unsaved-changes question for every request it sends, and the recovery dialog is its own.

use std::path::Path;

use egui::{Align2, Window};

use crate::state::{Dialog, Intent, State, UnsavedAnswer};

/// Draws the dialog the state holds, if it is one of the app's own, and returns the intents.
pub fn show(ctx: &egui::Context, state: &State) -> Vec<Intent> {
    let mut intents = Vec::new();
    match &state.dialog {
        Some(Dialog::Unsaved { name }) => unsaved_dialog(ctx, name, &mut intents),
        Some(Dialog::Overwrite { path, .. }) => overwrite_dialog(ctx, path, &mut intents),
        Some(Dialog::Recovery { .. }) | None => {}
    }
    intents
}

fn unsaved_dialog(ctx: &egui::Context, name: &str, intents: &mut Vec<Intent>) {
    Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(format!(
                "Do you want to save the changes you made to {name}?"
            ));
            ui.horizontal(|ui| {
                for (label, answer) in [
                    ("Save", UnsavedAnswer::Save),
                    ("Don't save", UnsavedAnswer::Discard),
                    ("Cancel", UnsavedAnswer::Cancel),
                ] {
                    if ui.button(label).clicked() {
                        intents.push(Intent::Unsaved(answer));
                    }
                }
            });
        });
}

fn overwrite_dialog(ctx: &egui::Context, path: &Path, intents: &mut Vec<Intent>) {
    Window::new("Replace file")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(format!("{} already exists. Replace it?", path.display()));
            ui.horizontal(|ui| {
                if ui.button("Replace").clicked() {
                    intents.push(Intent::Overwrite(true));
                }
                if ui.button("Cancel").clicked() {
                    intents.push(Intent::Overwrite(false));
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use cuboid_settings::{Environment, Settings};

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

    fn frame(state: &State) -> Vec<Intent> {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let intents = show(&ctx, state);
        ctx.end_pass().textures_delta.clear();
        intents
    }

    #[test]
    fn no_dialog_asks_for_nothing() {
        let (_dir, state) = state();
        assert!(frame(&state).is_empty());
    }

    #[test]
    fn the_questions_build_without_panicking() {
        let (_dir, mut state) = state();
        state.dialog = Some(Dialog::Unsaved {
            name: "model".to_owned(),
        });
        assert!(frame(&state).is_empty());
        state.dialog = Some(Dialog::Overwrite {
            path: "model.cubik".into(),
            purpose: crate::flows::PickPurpose::SaveAs,
        });
        assert!(frame(&state).is_empty());
    }
}
