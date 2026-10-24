#![forbid(unsafe_code)]
//! Layer L2. Theme, fonts, icons and the reusable widgets of the modern UI (roadmap_lean 6).
//!
//! Owns the look: dark and light themes with an accent colour, UI scale 75-200 %, a high-contrast
//! option, vector icons drawn with the painter (no icon font: the candidate `egui-phosphor` and
//! the docking crate `egui_dock` both require egui 0.36, the workspace is on 0.37), numeric drag
//! fields, colour swatches, tree rows, toolbar buttons, tabs and toasts. Must not know
//! documents, files or actions.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod fonts;
pub mod icons;
pub mod theme;
pub mod widgets;

pub use fonts::{FontSet, install_fonts};
pub use icons::Icon;
pub use theme::{
    ACCENT_PRESETS, GRID, RADIUS, RADIUS_LG, RADIUS_SM, Theme, ThemeMode, Tokens, UI_SCALE_MAX,
    UI_SCALE_MIN, contrast_ratio, fit_contrast,
};
pub use widgets::{
    ColorEditResponse, DragFieldOpts, DropHint, EditOutcome, FieldResponse, Level, RowToggle,
    Toasts, TreeRow, TreeRowResponse, color_edit, color_swatch, drag_field, format_number,
    icon_button, key_chip, parse_number, search_field, section, segmented, tab_button, tool_button,
    tree_row, weak_text_right,
};
