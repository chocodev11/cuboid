//! Collapsing sections and the search field, in the signatures the screens already use.

use egui::{Response, Ui};

use crate::icons::Icon;
use crate::widgets::controls::text_input;
use crate::widgets::layout::Section;

/// A titled block that folds. The open state lives in the context under `id_salt`.
/// Returns the body's result while it is open.
pub fn section<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    add_body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    Section::new(title)
        .collapsible(default_open)
        .show(ui, id_salt, add_body)
}

/// A single-line search box with a magnifier and a clear button. The returned response is the
/// text edit's.
pub fn search_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    hint: &str,
) -> Response {
    text_input(ui, id_salt, text, hint, Some(Icon::Search), true)
}
