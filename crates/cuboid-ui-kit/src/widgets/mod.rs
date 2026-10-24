//! Reusable widgets. Each one takes plain values and returns a response; none knows documents.

mod button;
mod field;
mod misc;
mod swatch;
mod toast;
mod tree;

pub use button::{icon_button, key_chip, segmented, tab_button, tool_button, weak_text_right};
pub use field::{DragFieldOpts, FieldResponse, drag_field, format_number, parse_number};
pub use misc::{search_field, section};
pub use swatch::{ColorEditResponse, color_edit, color_swatch};
pub use toast::{Level, Toasts};
pub use tree::{DropHint, EditOutcome, RowToggle, TreeRow, TreeRowResponse, tree_row};
