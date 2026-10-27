//! Reusable widgets. Each one takes plain values and returns a response; none knows documents.

mod button;
mod controls;
mod dropdown;
mod field;
mod layout;
mod list;
mod menu;
mod misc;
mod modal;
mod paint;
mod slider;
mod state;
mod swatch;
mod tabs;
mod toast;
mod tree;

pub use button::{
    Button, ButtonKind, IconButton, Tone, WindowButton, badge, icon_button, key_chip, tool_button,
    weak_text_right,
};
pub use controls::{checkbox, command_field, progress, text_area, text_field, text_input, toggle};
pub use dropdown::dropdown;
pub use field::{
    DragFieldOpts, FieldResponse, VectorResponse, axis_field_min, drag_field, format_number,
    parse_number, vector_field,
};
pub use layout::{
    Card, CardResponse, EmptyState, PanelFrame, PropertyGrid, PropertyRows, Section, Toolbar,
    ToolbarUi, content_frame, inset,
};
pub use list::ListRow;
pub use menu::{MenuItem, menu_bar, menu_button, menu_trigger, overflow_menu, submenu};
pub use misc::{search_field, section};
pub use modal::{Modal, ModalButton, ModalOutcome};
pub use paint::{fit_centered, paint_image};
pub use slider::{SliderOpts, slider};
pub use state::{ControlState, forced_state};
pub use swatch::{ColorEditResponse, color_edit, color_rgba, color_swatch, rgb_color, rgba_color};
pub use tabs::{
    chip, icon_tab, icon_tab_badged, segmented, tab_button, tab_button_max, tab_icon, tab_width,
    tabs,
};
pub use toast::{Level, Toasts};
pub use tree::{DropHint, EditOutcome, RowToggle, TreeRow, TreeRowResponse, tree_row};
