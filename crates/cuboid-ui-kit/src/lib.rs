#![forbid(unsafe_code)]
//! Layer L2. Theme, fonts, icons and the reusable widgets of the modern UI (UI v2 specification,
//! `docs/design/ui_v2_spec.md`).
//!
//! Owns the look: the Catppuccin and Graphite themes (dark, light, system) with an accent colour,
//! UI scale 75-200 % and a high-contrast option; the typography roles in Inter; the Phosphor icon
//! font behind the [`Icon`] enum; the size tokens in [`metrics`]; and every component of the
//! specification. Screens build from these and set no font, colour, margin or radius of their
//! own (`cargo xtask ui-style`). Must not know documents, files or actions.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod fonts;
pub mod icons;
pub mod metrics;
pub mod theme;
pub mod typography;
pub mod widgets;

pub use fonts::{FontSet, install_fonts};
pub use icons::Icon;
pub use theme::{
    DarkFlavor, GRAPHITE_ACCENTS, GRID, RADIUS, RADIUS_LG, RADIUS_MODAL, RADIUS_SM, Surface, Theme,
    ThemeFamily, ThemeMode, Tokens, UI_SCALE_MAX, UI_SCALE_MIN, contrast_ratio, fit_contrast,
};
pub use typography::{TextColor, Type, Weight, text};
pub use widgets::{
    Button, ButtonKind, Card, CardResponse, ColorEditResponse, ControlState, DragFieldOpts,
    DropHint, EditOutcome, EmptyState, FieldResponse, IconButton, Level, ListRow, MenuItem, Modal,
    ModalButton, ModalOutcome, PanelFrame, PropertyGrid, PropertyRows, RowToggle, Section,
    SliderOpts, Toasts, Tone, Toolbar, ToolbarUi, TreeRow, TreeRowResponse, VectorResponse,
    WindowButton, axis_field_min, badge, checkbox, chip, color_edit, color_rgba, color_swatch,
    command_field, content_frame, drag_field, dropdown, fit_centered, forced_state, format_number,
    icon_button, icon_tab, icon_tab_badged, inset, key_chip, menu_bar, menu_button, menu_trigger,
    overflow_menu, paint_image, parse_number, progress, rgb_color, rgba_color, search_field,
    section, segmented, slider, submenu, tab_button, tab_icon, tab_width, tabs, text_area,
    text_field, text_input, toggle, tool_button, tree_row, vector_field, weak_text_right,
};
