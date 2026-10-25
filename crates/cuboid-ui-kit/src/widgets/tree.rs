//! Tree rows for the outliner: indent, disclosure arrow, icon, label, trailing toggles and inline
//! rename.

use egui::text::{CCursor, CCursorRange};
use egui::{Key, Rect, Response, Role, Sense, Stroke, TextEdit, Ui, WidgetInfo, pos2, vec2};

use crate::icons::Icon;
use crate::metrics::{TAB_UNDERLINE, height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::paint::paint_clipped;
use crate::widgets::state::focus_ring;

/// Indent per nesting level.
const INDENT: f32 = space::MD;
const TOGGLE_SIDE: f32 = height::CONTROL;

/// A small icon button at the right end of a row, such as visibility or lock. It shows while the
/// pointer is over the row, the row is selected or the button has focus, and always while it
/// marks the row (hidden or locked).
#[derive(Copy, Clone, Debug)]
pub struct RowToggle<'a> {
    /// Icon while `state` is on.
    pub on: Icon,
    /// Icon while `state` is off.
    pub off: Icon,
    /// Current state.
    pub state: bool,
    /// Tooltip and accessible name.
    pub tip: &'a str,
}

/// Where a dragged row would land relative to the row under the pointer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DropHint {
    /// Above the row, as its previous sibling.
    Before,
    /// Inside the row, as its last child.
    Inside,
    /// Below the row, as its next sibling.
    After,
}

impl DropHint {
    /// The hint for a pointer at height `y` over `rect`: the top quarter is `Before`, the bottom
    /// quarter `After`, the middle `Inside` when `can_nest`, else the nearer half.
    pub fn at(rect: Rect, y: f32, can_nest: bool) -> DropHint {
        let t = ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        if can_nest {
            if t < 0.25 {
                DropHint::Before
            } else if t > 0.75 {
                DropHint::After
            } else {
                DropHint::Inside
            }
        } else if t < 0.5 {
            DropHint::Before
        } else {
            DropHint::After
        }
    }
}

/// How an inline rename ended.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EditOutcome {
    /// Still editing, or not editing.
    None,
    /// Enter was pressed or the box lost focus.
    Commit,
    /// Escape was pressed.
    Cancel,
}

/// One row of a tree.
pub struct TreeRow<'a> {
    /// Nesting level, 0 at the root.
    pub depth: usize,
    /// Text of the row.
    pub label: &'a str,
    /// Icon in front of the text.
    pub icon: Icon,
    /// `Some(open)` for a row with children, `None` for a leaf.
    pub expanded: Option<bool>,
    /// The row is part of the selection.
    pub selected: bool,
    /// The row is the primary selection.
    pub primary: bool,
    /// Draw the text weak, for hidden items.
    pub dimmed: bool,
    /// Up to two trailing toggles, drawn right to left.
    pub toggles: &'a [RowToggle<'a>],
    /// Drop indicator to paint while a drag hovers this row.
    pub drop: Option<DropHint>,
    /// Rename buffer; when set the label is a text box.
    pub editing: Option<&'a mut String>,
}

/// What happened on a [`tree_row`].
pub struct TreeRowResponse {
    /// The whole row; click, double click, drag and secondary click come from here.
    pub row: Response,
    /// The disclosure arrow was clicked.
    pub arrow: bool,
    /// Which trailing toggles were clicked.
    pub toggled: [bool; 2],
    /// The inline rename ended.
    pub edit: EditOutcome,
}

/// Draws one full-width row.
pub fn tree_row(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    row: TreeRow<'_>,
) -> TreeRowResponse {
    let tokens = Tokens::of(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let width = ui.available_width();
    let (_, rect) = ui.allocate_space(vec2(width, height::ROW));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::TreeItem, enabled, row.selected, row.label));

    let indent = space::XS + row.depth as f32 * INDENT;
    let arrow_rect = Rect::from_min_size(
        pos2(rect.left() + indent, rect.top()),
        vec2(icon_size::CONTROL, height::ROW),
    );
    let icon_rect = Rect::from_center_size(
        pos2(arrow_rect.right() + space::SM + 2.0, rect.center().y),
        vec2(icon_size::CONTROL, icon_size::CONTROL),
    );
    let label_left = icon_rect.right() + space::SM;
    let toggle_count = row.toggles.len().min(2);
    let toggles_left = rect.right() - space::XS - toggle_count as f32 * TOGGLE_SIDE;

    let mut out = TreeRowResponse {
        row: response.clone(),
        arrow: false,
        toggled: [false; 2],
        edit: EditOutcome::None,
    };

    if row.expanded.is_some() {
        let arrow = ui.interact(arrow_rect, id.with("arrow"), Sense::click());
        out.arrow = arrow.clicked();
    }
    for (i, toggle) in row.toggles.iter().take(2).enumerate() {
        let slot = Rect::from_min_size(
            pos2(
                toggles_left + i as f32 * TOGGLE_SIDE,
                rect.top() + (height::ROW - TOGGLE_SIDE) / 2.0,
            ),
            vec2(TOGGLE_SIDE, TOGGLE_SIDE),
        );
        let resp = ui.interact(slot, id.with(("toggle", i)), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(Role::Button, enabled, toggle.state, toggle.tip));
        out.toggled[i] = resp.clicked();
        let icon = if toggle.state { toggle.on } else { toggle.off };
        let marks_row = matches!(icon, Icon::EyeOff | Icon::Lock);
        let shown =
            marks_row || response.hovered() || row.selected || resp.has_focus() || resp.hovered();
        if shown && ui.is_rect_visible(rect) {
            let color = if resp.hovered() {
                tokens.text_strong
            } else if marks_row {
                tokens.text
            } else {
                tokens.text_weak
            };
            icon.paint(
                ui.painter(),
                Rect::from_center_size(slot.center(), vec2(icon_size::CONTROL, icon_size::CONTROL)),
                color,
            );
        }
        resp.on_hover_text(toggle.tip);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if row.selected {
            painter.rect_filled(
                rect.shrink2(vec2(2.0, 0.0)),
                radius::CONTROL,
                tokens.accent_soft,
            );
            if row.primary {
                let bar = Rect::from_min_size(
                    rect.min + vec2(space::XXS, space::XS),
                    vec2(TAB_UNDERLINE, height::ROW - 2.0 * space::XS),
                );
                painter.rect_filled(bar, 0.0, tokens.accent);
            }
        } else if response.hovered() {
            painter.rect_filled(
                rect.shrink2(vec2(2.0, 0.0)),
                radius::CONTROL,
                tokens.bg_hover,
            );
        }
        if let Some(open) = row.expanded {
            let icon = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            icon.paint(
                painter,
                Rect::from_center_size(
                    arrow_rect.center(),
                    vec2(icon_size::CONTROL - 4.0, icon_size::CONTROL - 4.0),
                ),
                tokens.text_weak,
            );
        }
        let fg = if row.dimmed {
            tokens.text_weak.gamma_multiply(0.75)
        } else {
            tokens.text
        };
        row.icon.paint(painter, icon_rect, fg);
        if row.editing.is_none() {
            let label_rect = Rect::from_min_max(
                pos2(label_left, rect.top()),
                pos2(toggles_left - 4.0, rect.bottom()),
            );
            let (role, ink) = if row.selected {
                (Type::Strong, tokens.text_strong)
            } else {
                (Type::Body, fg)
            };
            paint_clipped(ui, role, row.label, label_rect, label_left, ink);
        }
        paint_drop(painter, rect, row.drop, &tokens);
    }

    if let Some(buffer) = row.editing {
        let field = Rect::from_min_max(
            pos2(label_left - space::XS, rect.top() + space::XXS),
            pos2(toggles_left - space::XS, rect.bottom() - space::XXS),
        );
        ui.painter()
            .rect_filled(field, radius::CONTROL, tokens.bg_control);
        focus_ring(ui, field, radius::CONTROL, &tokens);
        let label_rect = field.shrink2(vec2(space::XS, 0.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(label_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let escape = child.input(|i| i.key_pressed(Key::Escape));
        let edit_id = id.with("rename");
        let first = !child.data(|d| d.get_temp::<bool>(edit_id)).unwrap_or(false);
        let mut output = TextEdit::singleline(buffer)
            .id(edit_id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(Type::Body.font())
            .desired_width(label_rect.width())
            .min_size(label_rect.size())
            .align(egui::Align2::LEFT_CENTER)
            .show(&mut child);
        output.response.response.clone().accessible_name("Rename");
        if first {
            output.response.request_focus();
            let len = output.galley.text().chars().count();
            output
                .state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
            output.state.store(child.ctx(), edit_id);
            child.data_mut(|d| d.insert_temp(edit_id, true));
        } else if escape {
            out.edit = EditOutcome::Cancel;
        } else if output.response.lost_focus() {
            out.edit = EditOutcome::Commit;
        }
        if out.edit != EditOutcome::None {
            child.data_mut(|d| d.remove::<bool>(edit_id));
        }
    }
    out
}

fn paint_drop(painter: &egui::Painter, rect: Rect, drop: Option<DropHint>, tokens: &Tokens) {
    let Some(hint) = drop else { return };
    let stroke = Stroke::new(2.0, tokens.accent);
    match hint {
        DropHint::Before => painter.hline(rect.x_range(), rect.top() + 1.0, stroke),
        DropHint::After => painter.hline(rect.x_range(), rect.bottom() - 1.0, stroke),
        DropHint::Inside => painter.rect_stroke(
            rect.shrink2(vec2(2.0, 1.0)),
            radius::CONTROL,
            stroke,
            egui::StrokeKind::Inside,
        ),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_hint_splits_a_row_into_bands() {
        let rect = Rect::from_min_size(pos2(0.0, 100.0), vec2(200.0, 24.0));
        assert_eq!(DropHint::at(rect, 101.0, true), DropHint::Before);
        assert_eq!(DropHint::at(rect, 112.0, true), DropHint::Inside);
        assert_eq!(DropHint::at(rect, 123.0, true), DropHint::After);
        assert_eq!(DropHint::at(rect, 105.0, false), DropHint::Before);
        assert_eq!(DropHint::at(rect, 118.0, false), DropHint::After);
        assert_eq!(DropHint::at(rect, -50.0, true), DropHint::Before);
    }
}
