//! Component state rendering in headless frames: which fill and which ink each state paints, the
//! sizes of the specification, and the keyboard behaviour.

mod common;

use cuboid_ui_kit::metrics::{height, label_column};
use cuboid_ui_kit::{
    Button, ControlState, DragFieldOpts, EmptyState, FontSet, Icon, IconButton, ListRow, Modal,
    ModalButton, PropertyGrid, Theme, Tokens, Tone, TreeRow, Type, WindowButton, axis_field_min,
    badge, checkbox, chip, color_rgba, command_field, contrast_ratio, drag_field, forced_state,
    icon_tab_badged, install_fonts, overflow_menu, rgba_color, segmented, tab_button, tab_icon,
    tab_width, tabs, text, text_area, toggle, tree_row, vector_field,
};
use egui::{Align, Color32, Context, Key, Modifiers, RawInput, Rect, Shape, Ui, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn context(light: bool) -> Context {
    let ctx = Context::default();
    install_fonts(&ctx, &FontSet::default());
    common::theme(light, 1.0).apply(&ctx);
    ctx
}

/// Everything one frame paints, with nested shapes flattened.
fn painted(ctx: &Context, add: impl FnMut(&mut Ui)) -> Vec<Shape> {
    let output = ctx.run_ui(RawInput::default(), add);
    let mut out = Vec::new();
    for clipped in &output.shapes {
        flatten(&clipped.shape, &mut out);
    }
    output.drop_without_applying_deltas();
    out
}

fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
    match shape {
        Shape::Vec(inner) => inner.iter().for_each(|s| flatten(s, out)),
        other => out.push(other.clone()),
    }
}

fn fills(shapes: &[Shape]) -> Vec<Color32> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.fill != Color32::TRANSPARENT => Some(r.fill),
            _ => None,
        })
        .collect()
}

fn has_ring(shapes: &[Shape], tokens: &Tokens) -> bool {
    shapes.iter().any(|s| match s {
        Shape::Rect(r) => r.stroke.color == tokens.focus && r.stroke.width > 0.0,
        _ => false,
    })
}

/// The ink of the first text shape that says `label`.
fn ink_of(shapes: &[Shape], label: &str) -> Option<Color32> {
    shapes.iter().find_map(|s| match s {
        Shape::Text(t) if t.galley.job.text == label => {
            let baked = t.galley.job.sections[0].format.color;
            Some(if baked == Color32::PLACEHOLDER {
                t.fallback_color
            } else {
                baked
            })
        }
        _ => None,
    })
}

fn size_of(ctx: &Context, mut add: impl FnMut(&mut Ui)) -> egui::Vec2 {
    let mut size = egui::Vec2::ZERO;
    let output = ctx.run_ui(RawInput::default(), |ui| {
        let before = ui.min_rect();
        add(ui);
        size = ui.min_rect().union(before).size();
    });
    output.drop_without_applying_deltas();
    size
}

#[test]
fn text_roles_lay_out_with_their_font() {
    let ctx = context(false);
    let shapes = painted(&ctx, |ui| {
        for role in Type::ALL {
            ui.label(text(role, format!("{role:?}")));
        }
    });
    for role in Type::ALL {
        let galley = shapes
            .iter()
            .find_map(|s| match s {
                Shape::Text(t) if t.galley.job.text == format!("{role:?}") => {
                    Some(t.galley.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{role:?} was not painted"));
        let format = &galley.job.sections[0].format;
        assert_eq!(format.font_id, role.font(), "{role:?}");
    }
}

#[test]
fn text_colour_follows_the_role() {
    for light in [false, true] {
        let ctx = context(light);
        let t = Tokens::of(&ctx);
        let shapes = painted(&ctx, |ui| {
            for role in [Type::Caption, Type::Body, Type::Title] {
                ui.label(text(role, format!("{role:?}")));
            }
        });
        assert_eq!(ink_of(&shapes, "Caption"), Some(t.text_weak));
        assert_eq!(ink_of(&shapes, "Body"), Some(t.text));
        assert_eq!(ink_of(&shapes, "Title"), Some(t.text_strong));
    }
}

#[test]
fn button_kinds_paint_their_fill_in_every_state() {
    for light in [false, true] {
        let ctx = context(light);
        let t = Tokens::of(&ctx);
        let fill = |state: ControlState, make: fn() -> Button<'static>| {
            let shapes = painted(&ctx, |ui| {
                forced_state(ui, state, |ui| {
                    ui.add(make());
                });
            });
            fills(&shapes).first().copied()
        };
        let primary = || Button::new("Save").primary();
        assert_eq!(fill(ControlState::Normal, primary), Some(t.accent));
        assert_eq!(fill(ControlState::Hover, primary), Some(t.accent_hover));
        assert_ne!(fill(ControlState::Pressed, primary), Some(t.accent));
        assert_ne!(fill(ControlState::Disabled, primary), Some(t.accent));
        let secondary = || Button::new("Cancel");
        assert_eq!(fill(ControlState::Normal, secondary), Some(t.bg_control));
        assert_eq!(fill(ControlState::Hover, secondary), Some(t.control_hover));
        assert_eq!(
            fill(ControlState::Pressed, secondary),
            Some(t.control_active)
        );
        assert_eq!(
            fill(ControlState::Selected, secondary),
            Some(t.control_active)
        );
        let ghost = || Button::new("Details").ghost();
        assert_eq!(fill(ControlState::Normal, ghost), None);
        assert_eq!(fill(ControlState::Hover, ghost), Some(t.bg_hover));
        let danger = || Button::new("Delete").danger();
        assert_eq!(fill(ControlState::Normal, danger), Some(t.danger));
    }
}

#[test]
fn primary_text_is_the_on_accent_ink() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let shapes = painted(&ctx, |ui| {
        ui.add(Button::new("Save").primary());
    });
    assert_eq!(ink_of(&shapes, "Save"), Some(t.on_accent));
}

#[test]
fn disabled_controls_dim_their_text() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let normal = painted(&ctx, |ui| {
        ui.add(Button::new("Cancel"));
    });
    let disabled = painted(&ctx, |ui| {
        forced_state(ui, ControlState::Disabled, |ui| {
            ui.add(Button::new("Cancel"));
        });
    });
    assert_eq!(ink_of(&normal, "Cancel"), Some(t.text));
    assert_ne!(ink_of(&disabled, "Cancel"), Some(t.text));
}

#[test]
fn focus_draws_a_ring_and_nothing_else_does() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let ringed = |state| {
        painted(&ctx, |ui| {
            forced_state(ui, state, |ui| {
                ui.add(Button::new("Cancel"));
                ui.add(IconButton::new(Icon::Settings, "Settings"));
                ui.add(WindowButton::new(Icon::Minus, "Minimize"));
                chip(ui, "Textured", false);
                tab_button(ui, "Element", false);
            });
        })
    };
    assert!(has_ring(&ringed(ControlState::Focus), &t));
    for state in [
        ControlState::Normal,
        ControlState::Hover,
        ControlState::Pressed,
        ControlState::Selected,
        ControlState::Disabled,
    ] {
        assert!(!has_ring(&ringed(state), &t), "{state:?}");
    }
}

#[test]
fn segmented_and_chips_select_without_the_accent() {
    for light in [false, true] {
        let ctx = context(light);
        let t = Tokens::of(&ctx);
        let shapes = painted(&ctx, |ui| {
            let mut current = 1;
            segmented(ui, &mut current, &["Local", "Global"]);
            chip(ui, "Textured", true);
            toggle(ui, &mut true, "Snap");
            checkbox(ui, &mut true, "Mirror");
        });
        let all = fills(&shapes);
        assert!(all.contains(&t.control_active), "light {light}");
        for accent in [t.accent, t.accent_soft, t.accent_hover] {
            assert!(!all.contains(&accent), "light {light}: the accent was used");
        }
        assert_eq!(ink_of(&shapes, "Global"), Some(t.text_strong));
        assert_eq!(ink_of(&shapes, "Local"), Some(t.text_weak));
        assert_eq!(ink_of(&shapes, "Textured"), Some(t.text_strong));
    }
}

#[test]
fn tabs_underline_only_the_active_one_in_the_accent() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let count = |selected: bool| {
        fills(&painted(&ctx, |ui| {
            tab_button(ui, "Element", selected);
        }))
        .iter()
        .filter(|c| **c == t.accent)
        .count()
    };
    assert_eq!(count(true), 1);
    assert_eq!(count(false), 0);
}

#[test]
fn the_active_tool_takes_the_accent_and_a_toggle_does_not() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let tool = painted(&ctx, |ui| {
        ui.add(IconButton::new(Icon::Move, "Move").toolbar().active(true));
    });
    assert!(fills(&tool).contains(&t.accent_soft));
    let toggled = painted(&ctx, |ui| {
        ui.add(
            IconButton::new(Icon::Magnet, "Snap")
                .toolbar()
                .selected(true),
        );
    });
    assert!(fills(&toggled).contains(&t.bg_active));
    assert!(!fills(&toggled).contains(&t.accent_soft));
}

#[test]
fn window_buttons_fill_on_hover_and_the_close_button_turns_danger() {
    for light in [false, true] {
        let ctx = context(light);
        let t = Tokens::of(&ctx);
        let paint = |state: ControlState, close: bool| {
            let shapes = painted(&ctx, |ui| {
                forced_state(ui, state, |ui| {
                    let button = WindowButton::new(Icon::Minus, "Minimize");
                    ui.add(if close { button.close() } else { button });
                });
            });
            let ink = ink_of(&shapes, &Icon::Minus.glyph().to_string());
            (fills(&shapes).first().copied(), ink)
        };
        assert_eq!(
            paint(ControlState::Normal, false),
            (None, Some(t.text_weak))
        );
        assert_eq!(
            paint(ControlState::Hover, false),
            (Some(t.control_hover), Some(t.text_strong))
        );
        assert_eq!(
            paint(ControlState::Pressed, false),
            (Some(t.control_active), Some(t.text_strong))
        );
        assert_eq!(paint(ControlState::Normal, true), (None, Some(t.text_weak)));
        assert_eq!(
            paint(ControlState::Hover, true),
            (Some(t.danger), Some(t.on_accent))
        );
        let (pressed, ink) = paint(ControlState::Pressed, true);
        assert_ne!(pressed, Some(t.danger));
        assert_eq!(ink, Some(t.on_accent));
    }
}

#[test]
fn icons_are_drawn_from_the_icon_family() {
    let ctx = context(false);
    let shapes = painted(&ctx, |ui| {
        ui.add(IconButton::new(Icon::Settings, "Settings"));
    });
    let glyph = shapes.iter().find_map(|s| match s {
        Shape::Text(t) => Some(t.galley.job.text.clone()),
        _ => None,
    });
    assert_eq!(glyph, Some(Icon::Settings.glyph().to_string()));
}

#[test]
fn sizes_follow_the_specification() {
    let ctx = context(false);
    let h = |add: &dyn Fn(&mut Ui)| size_of(&ctx, |ui| add(ui)).y;
    assert!(
        (h(&|ui| {
            ui.add(Button::new("Open"));
        }) - height::CONTROL)
            .abs()
            < 0.01
    );
    assert!(
        (h(&|ui| {
            ui.add(Button::new("Open").primary().tall());
        }) - height::ACTION)
            .abs()
            < 0.01
    );
    let icon = size_of(&ctx, |ui| {
        ui.add(IconButton::new(Icon::Cube, "Cube"));
    });
    assert_eq!([icon.x, icon.y], [24.0, 24.0]);
    let toolbar = size_of(&ctx, |ui| {
        ui.add(IconButton::new(Icon::Cube, "Cube").toolbar());
    });
    assert_eq!([toolbar.x, toolbar.y], [28.0, 28.0]);
    let window = size_of(&ctx, |ui| {
        ui.add(WindowButton::new(Icon::Close, "Close").close());
    });
    assert_eq!([window.x, window.y], [46.0, 32.0]);
    assert!(
        (h(&|ui| {
            tab_button(ui, "Element", true);
        }) - 28.0)
            .abs()
            < 0.01
    );
    assert!(
        (h(&|ui| {
            ui.add(ListRow::new("Row"));
        }) - 28.0)
            .abs()
            < 0.01
    );
    assert!(
        (h(&|ui| {
            ui.add(ListRow::new("Row").thumbnail(egui::TextureId::default(), vec2(4.0, 3.0)));
        }) - 44.0)
            .abs()
            < 0.01
    );
    assert!(
        (h(&|ui| {
            segmented(ui, &mut 0, &["A", "B"]);
        }) - 24.0)
            .abs()
            < 0.01
    );
    assert!(
        (h(&|ui| {
            drag_field(ui, "dv", &mut 1.0, &DragFieldOpts::default());
        }) - 24.0)
            .abs()
            < 0.01
    );
}

#[test]
fn badge_padding_is_two_by_six() {
    let ctx = context(false);
    let size = size_of(&ctx, |ui| {
        badge(ui, "Cubik", Tone::Neutral);
    });
    let label = egui::Vec2::new(size.x - 12.0, size.y - 4.0);
    let galley = painted(&ctx, |ui| {
        badge(ui, "Cubik", Tone::Neutral);
    });
    let text_size = galley.iter().find_map(|s| match s {
        Shape::Text(t) => Some(t.galley.size()),
        _ => None,
    });
    assert_eq!(text_size, Some(label));
}

#[test]
fn property_grid_rows_are_24_with_a_96_label_column() {
    let ctx = context(false);
    let mut value_left = 0.0;
    let mut row_top = Vec::new();
    let mut origin = 0.0;
    let output = ctx.run_ui(RawInput::default(), |ui| {
        origin = ui.cursor().left();
        PropertyGrid::new("g").show(ui, |grid| {
            for label in ["A", "B"] {
                grid.row(label, |ui| {
                    value_left = ui.max_rect().left();
                    row_top.push(ui.max_rect());
                });
            }
        });
    });
    output.drop_without_applying_deltas();
    assert!((value_left - origin - (label_column::DEFAULT + 8.0)).abs() < 0.01);
    assert!((row_top[0].height() - 24.0).abs() < 0.01);
    assert!((row_top[1].top() - row_top[0].bottom() - 4.0).abs() < 0.01);
    let narrow = Rect::from_min_size(egui::Pos2::ZERO, vec2(1.0, 1.0));
    let _ = narrow;
}

#[test]
fn a_row_with_a_text_area_is_as_tall_as_the_area_and_the_next_row_follows_it() {
    let ctx = context(false);
    let (mut area, mut next) = (Rect::NOTHING, Rect::NOTHING);
    let mut goal = String::from("A goal");
    let output = ctx.run_ui(RawInput::default(), |ui| {
        PropertyGrid::new("tall").show(ui, |grid| {
            grid.row("Goal", |ui| {
                area = text_area(ui, "goal", &mut goal, "Goal", 3).rect;
            });
            grid.row("Platform", |ui| next = ui.max_rect());
        });
    });
    output.drop_without_applying_deltas();
    assert!(next.top() >= area.bottom(), "{area:?} {next:?}");
    assert!(
        next.top() - area.top() >= 3.0 * Type::Body.line_height(),
        "{area:?} {next:?}"
    );
}

#[test]
fn a_text_area_grows_with_its_lines_up_to_eight_and_then_scrolls() {
    let ctx = context(false);
    let height_of = |text: &str| {
        let mut text = text.to_owned();
        let mut height = 0.0;
        let output = ctx.run_ui(RawInput::default(), |ui| {
            ui.set_max_width(320.0);
            let top = ui.cursor().top();
            text_area(ui, "area", &mut text, "Hint", 3);
            height = ui.cursor().top() - top;
        });
        output.drop_without_applying_deltas();
        height
    };
    let line = Type::Body.line_height();
    let three = height_of("one line");
    assert!((height_of("1\n2\n3\n4\n5") - three - 2.0 * line).abs() < 0.5);
    assert!((height_of(&"x\n".repeat(30)) - three - 5.0 * line).abs() < 0.5);
    // A long line wraps, and its wrapped lines count.
    assert!(height_of(&"word ".repeat(200)) > three + line);
}

#[test]
fn the_label_column_is_kept_between_72_and_128() {
    let ctx = context(false);
    for (asked, want) in [(10.0, 72.0), (100.0, 100.0), (500.0, 128.0)] {
        let mut left = 0.0;
        let mut origin = 0.0;
        let output = ctx.run_ui(RawInput::default(), |ui| {
            origin = ui.cursor().left();
            PropertyGrid::new("w").label_width(asked).show(ui, |grid| {
                grid.row("A", |ui| left = ui.max_rect().left());
            });
        });
        output.drop_without_applying_deltas();
        assert!((left - origin - (want + 8.0)).abs() < 0.01, "asked {asked}");
    }
}

fn harness<S: 'static>(
    state: S,
    mut app: impl FnMut(&mut Ui, &mut S) + 'static,
) -> Harness<'static, S> {
    Harness::builder()
        .with_size(vec2(480.0, 200.0))
        .with_context_setup(|ctx| {
            install_fonts(ctx, &FontSet::default());
            Theme::default().apply(ctx);
        })
        .build_ui_state(
            move |ui, s: &mut S| {
                Theme::default().apply(ui.ctx());
                egui::CentralPanel::default().show(ui, |ui| app(ui, s));
            },
            state,
        )
}

#[test]
fn arrow_keys_move_the_selection_of_segmented_and_tabs() {
    let mut h = harness((0_usize, 0_usize), |ui, (seg, tab)| {
        segmented(ui, seg, &["Local", "Global", "View"]);
        tabs(ui, tab, &["Element", "Faces", "Texture"]);
    });
    h.run_steps(2);
    h.get_by_label("Local").focus();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().0, 1);
    h.key_press(Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().0, 2);
    h.key_press(Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().0, 2, "the end is not wrapped");
    h.key_press(Key::ArrowLeft);
    h.run_steps(3);
    assert_eq!(h.state().0, 1);

    h.get_by_label("Element").focus();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().1, 1);
}

#[test]
fn enter_presses_a_focused_button_and_a_checkbox() {
    let mut h = harness((0_u32, false), |ui, (presses, checked)| {
        if ui.add(Button::new("Save").primary()).clicked() {
            *presses += 1;
        }
        checkbox(ui, checked, "Mirror");
    });
    h.run_steps(2);
    h.get_by_label("Save").focus();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().0, 1);
    h.get_by_label("Mirror").focus();
    h.run_steps(2);
    h.key_press(Key::Space);
    h.run_steps(2);
    assert!(h.state().1);
}

#[test]
fn up_and_down_nudge_a_focused_drag_value() {
    let mut h = harness(5.0_f32, |ui, v| {
        drag_field(
            ui,
            "dv",
            v,
            &DragFieldOpts {
                name: "Width",
                speed: 0.1,
                ..DragFieldOpts::default()
            },
        );
    });
    h.run_steps(2);
    h.get_by_label("Width").focus();
    h.run_steps(2);
    h.key_press(Key::ArrowUp);
    h.run_steps(2);
    assert!((*h.state() - 6.0).abs() < 1e-4, "{}", h.state());
    h.key_press(Key::ArrowDown);
    h.key_press(Key::ArrowDown);
    h.run_steps(3);
    assert!((*h.state() - 4.0).abs() < 1e-4, "{}", h.state());
}

/// The label and the three fields of a "Position" row in a property grid `width` wide.
struct PositionRow {
    label: Rect,
    axes: [Rect; 3],
}

fn position_row(width: f32) -> PositionRow {
    let mut h = harness((), move |ui, ()| {
        ui.allocate_ui(vec2(width, 200.0), |ui| {
            PropertyGrid::new("g").show(ui, |grid| {
                grid.row("Position", |ui| {
                    let opts = DragFieldOpts {
                        name: "Position",
                        ..DragFieldOpts::default()
                    };
                    vector_field(ui, "pos", &mut [8.0, 16.5, -4.0], &opts);
                });
            });
        });
    });
    h.run_steps(3);
    PositionRow {
        label: h.get_by_label("Position").rect(),
        axes: ["Position X", "Position Y", "Position Z"].map(|name| h.get_by_label(name).rect()),
    }
}

/// The narrowest a field with an axis caption is laid out.
fn field_min() -> f32 {
    let mut h = harness(0.0_f32, |ui, min| *min = axis_field_min(ui));
    h.run_steps(2);
    *h.state()
}

/// The width of a property grid whose value column is `value` wide.
fn grid_width(value: f32) -> f32 {
    label_column::DEFAULT + 8.0 + value
}

#[test]
fn a_vector_row_keeps_three_fields_in_a_row_while_each_has_room_for_a_caption_and_a_number() {
    let min = field_min();
    let width = grid_width(3.0 * min + 2.0 * 4.0 + 0.5);
    let row = position_row(width);
    let [x, y, z] = row.axes;
    assert!((x.top() - y.top()).abs() < 0.01 && (y.top() - z.top()).abs() < 0.01);
    assert!(x.right() <= y.left() && y.right() <= z.left());
    assert!(row.label.right() <= x.left(), "the label stands beside");
    assert!(x.width() >= min - 0.01, "{x:?} is narrower than {min}");
    assert!(z.right() <= row.label.left() + width + 0.01);
    assert!((x.height() - 24.0).abs() < 0.01);
}

#[test]
fn a_narrower_vector_row_puts_z_below_x_and_y() {
    let min = field_min();
    for value in [3.0 * min + 2.0 * 4.0 - 0.5, 2.0 * min + 4.0 + 0.5] {
        let row = position_row(grid_width(value));
        let [x, y, z] = row.axes;
        assert!(
            (x.top() - y.top()).abs() < 0.01,
            "{value}: X and Y share a row"
        );
        assert!(x.right() <= y.left());
        assert!(
            (z.top() - x.bottom() - 4.0).abs() < 0.01,
            "{value}: Z is the next row"
        );
        assert!((z.left() - x.left()).abs() < 0.01 && (z.width() - x.width()).abs() < 0.01);
        assert!(x.width() >= min - 0.01, "{value}: {x:?}");
        assert!(row.label.right() <= x.left());
        // The row grew to hold both lines, and the label stays beside the first one.
        assert!((row.label.center().y - x.center().y).abs() < 0.01);
    }
}

#[test]
fn a_still_narrower_vector_row_has_one_field_to_a_row() {
    let min = field_min();
    let value = 2.0 * min + 4.0 - 0.5;
    let row = position_row(grid_width(value));
    let [x, y, z] = row.axes;
    assert!((x.top() + 28.0 - y.top()).abs() < 0.01 && (y.top() + 28.0 - z.top()).abs() < 0.01);
    assert!((x.left() - y.left()).abs() < 0.01 && (x.left() - z.left()).abs() < 0.01);
    assert!(
        (x.width() - value).abs() < 0.01,
        "{x:?} fills the value column"
    );
    assert!(x.width() >= min - 0.01);
    assert!(row.label.right() <= x.left());
}

#[test]
fn a_label_stands_above_its_value_when_the_value_column_would_be_too_narrow() {
    let min = field_min();
    // Just enough for the value beside the label, then a little less.
    let beside = position_row(grid_width(min + 0.5));
    assert!(beside.label.right() <= beside.axes[0].left());
    // Above the label the value has the whole width: two fields to a row.
    let width = grid_width(min - 0.5);
    let above = position_row(width);
    let [x, y, z] = above.axes;
    assert!(above.label.bottom() <= x.top(), "{:?} {x:?}", above.label);
    assert!((above.label.left() - x.left()).abs() < 0.01);
    assert!((x.top() - y.top()).abs() < 0.01 && z.top() > x.bottom());
    // Rectangles are snapped to the pixel grid.
    assert!(
        (y.right() - (above.label.left() + width)).abs() < 0.5,
        "{y:?} {:?} {width}",
        above.label
    );
    assert!(x.width() >= min - 0.01);
    // Narrower still: one field to a row, each as wide as the grid.
    let width = 2.0 * min + 4.0 - 0.5;
    let above = position_row(width);
    let [x, y, z] = above.axes;
    assert!(above.label.bottom() <= x.top() && y.top() > x.top() && z.top() > y.top());
    assert!((x.width() - width).abs() < 0.01, "{x:?} fills {width}");
}

#[test]
fn a_checkbox_label_that_does_not_fit_wraps_instead_of_running_out_of_the_row() {
    let rect_in = |label: &'static str, width: f32| {
        let mut h = harness(false, move |ui, on| {
            ui.allocate_ui(vec2(width, 200.0), |ui| {
                checkbox(ui, on, label);
            });
        });
        h.run_steps(3);
        h.get_by_label(label).rect()
    };
    let wide = rect_in("Follow the element", 300.0);
    assert!((wide.height() - 24.0).abs() < 0.01, "{wide:?}");
    let narrow = rect_in("Follow the element", 90.0);
    assert!(narrow.width() <= 90.0 + 0.01, "{narrow:?}");
    assert!(narrow.height() > 24.0, "{narrow:?} has more than one line");
    // A word that is wider than the room is not broken in the middle.
    let word = rect_in("Directional", 70.0);
    assert!((word.height() - 24.0).abs() < 0.01, "{word:?}");
}

#[test]
fn stacked_rows_follow_each_other_with_the_label_closer_to_its_own_value() {
    let mut h = harness([Rect::NOTHING; 2], |ui, values| {
        ui.allocate_ui(vec2(120.0, 200.0), |ui| {
            PropertyGrid::new("g").show(ui, |grid| {
                grid.row("Name", |ui| values[0] = ui.max_rect());
                grid.row("Filter", |ui| values[1] = ui.max_rect());
            });
        });
    });
    h.run_steps(3);
    let [first, second] = *h.state();
    let label = h.get_by_label("Filter").rect();
    assert!((first.width() - 120.0).abs() < 0.01 && (first.height() - 24.0).abs() < 0.01);
    let (above, below) = (label.top() - first.bottom(), second.top() - label.bottom());
    assert!(
        below > 0.0 && below < above,
        "{label:?} {first:?} {second:?}"
    );
}

#[test]
fn a_modal_cancels_on_escape_and_confirms_on_enter() {
    let buttons = [ModalButton::new("Cancel"), ModalButton::primary("Save")];
    let mut h = harness(
        (false, false, None::<usize>),
        move |ui, (cancelled, confirmed, clicked)| {
            let out = Modal::new("m", "Save changes?")
                .buttons(&buttons)
                .show(ui.ctx(), |ui| {
                    ui.label("Body");
                });
            *cancelled |= out.cancelled;
            *confirmed |= out.confirmed;
            if out.button.is_some() {
                *clicked = out.button;
            }
        },
    );
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().1, "Enter confirms");
    assert!(!h.state().0);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().0, "Escape cancels");
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert_eq!(h.state().2, Some(1));
    let _ = Modifiers::NONE;
}

#[test]
fn tree_rows_and_picture_rows_keep_their_heights() {
    let ctx = context(false);
    let row = |add: &dyn Fn(&mut Ui)| size_of(&ctx, |ui| add(ui)).y;
    let tree = row(&|ui| {
        tree_row(
            ui,
            "t",
            TreeRow {
                depth: 0,
                label: "Body",
                icon: Icon::Cube,
                expanded: None,
                selected: false,
                primary: false,
                dimmed: false,
                toggles: &[],
                drop: None,
                editing: None,
            },
        );
    });
    assert!((tree - height::ROW).abs() < 0.01, "{tree}");
    let with_icon = row(&|ui| {
        ui.add(ListRow::new("Row").picture_icon(Icon::Texture));
    });
    assert!((with_icon - height::ROW_THUMB).abs() < 0.01, "{with_icon}");
}

#[test]
fn a_dimmed_row_is_weak_and_a_shortcut_shows_as_a_chip() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let shapes = painted(&ctx, |ui| {
        ui.add(ListRow::new("Undone").dimmed(true));
        ui.add(ListRow::new("Done"));
        ui.add(ListRow::new("Copy").shortcut("Ctrl+C").caption("Edit"));
    });
    assert_eq!(ink_of(&shapes, "Undone"), Some(t.text_weak));
    assert_eq!(ink_of(&shapes, "Done"), Some(t.text));
    assert!(ink_of(&shapes, "Ctrl+C").is_some());
    assert!(ink_of(&shapes, "Edit").is_some());
}

#[test]
fn an_icon_tab_shows_its_count_and_its_bar_on_the_chosen_side() {
    let ctx = context(false);
    let t = Tokens::of(&ctx);
    let strip = |badge: Option<(usize, Tone)>, side| {
        painted(&ctx, |ui| {
            icon_tab_badged(ui, Icon::Validator, "Validator", true, badge, side);
        })
    };
    let has_text = |shapes: &[Shape], text: &str| {
        shapes
            .iter()
            .any(|s| matches!(s, Shape::Text(t) if t.galley.job.text == text))
    };
    assert!(has_text(&strip(Some((3, Tone::Danger)), Align::Min), "3"));
    assert!(has_text(
        &strip(Some((400, Tone::Danger)), Align::Min),
        "99+"
    ));
    assert!(!has_text(&strip(Some((0, Tone::Danger)), Align::Min), "0"));
    let bar = |side| {
        strip(None, side)
            .iter()
            .find_map(|s| match s {
                Shape::Rect(r) if r.fill == t.accent => Some(r.rect),
                _ => None,
            })
            .expect("the bar of the chosen tab")
    };
    let (left, right) = (bar(Align::Min), bar(Align::Max));
    assert!(right.left() > left.left() + 20.0, "{left:?} {right:?}");
}

#[test]
fn the_command_field_leaves_out_a_hint_that_does_not_fit() {
    let ctx = context(false);
    let shapes = |width| {
        painted(&ctx, |ui| {
            command_field(ui, "Search commands", Some("Ctrl+P"), width);
        })
    };
    let says_hint = |shapes: &[Shape]| ink_of(shapes, "Search commands").is_some();
    assert!(says_hint(&shapes(Some(220.0))));
    assert!(
        !says_hint(&shapes(Some(100.0))),
        "never cut with an ellipsis"
    );
    let natural = size_of(&ctx, |ui| {
        command_field(ui, "Search commands", Some("Ctrl+P"), None);
    });
    assert!(natural.x < 90.0, "{natural:?}");
    assert!(!says_hint(&shapes(None)));
    assert!(ink_of(&shapes(None), "Ctrl+P").is_some(), "the chord stays");
}

#[test]
fn a_picture_tab_is_as_high_as_a_text_tab_and_a_tab_knows_its_width() {
    let ctx = context(false);
    let picture = size_of(&ctx, |ui| {
        tab_icon(ui, Icon::History, "History", false);
    });
    assert!((picture.y - height::BAR).abs() < 0.01);
    let word = size_of(&ctx, |ui| {
        tab_button(ui, "History", false);
    });
    let mut asked = 0.0;
    let output = ctx.run_ui(RawInput::default(), |ui| asked = tab_width(ui, "History"));
    output.drop_without_applying_deltas();
    assert!((word.x - asked).abs() < 0.01);
    assert!(picture.x < word.x);
}

#[test]
fn buttons_of_an_empty_state_stand_one_above_the_other_when_they_do_not_fit() {
    let mut h = Harness::builder()
        .with_size(vec2(150.0, 300.0))
        .with_context_setup(|ctx| {
            install_fonts(ctx, &FontSet::default());
            Theme::default().apply(ctx);
        })
        .build_ui(|ui| {
            Theme::default().apply(ui.ctx());
            egui::CentralPanel::default().show(ui, |ui| {
                EmptyState::new(Icon::Library, "Empty")
                    .action(Button::new("Import files").icon(Icon::Import))
                    .action(Button::new("Import folder").icon(Icon::Folder))
                    .show(ui);
            });
        });
    h.run_steps(3);
    let first = h.get_by_label("Import files").rect();
    let second = h.get_by_label("Import folder").rect();
    assert!(second.top() >= first.bottom(), "{first:?} {second:?}");
    assert!(first.right() <= 150.0 && second.right() <= 150.0);
}

#[test]
fn colours_of_user_data_round_trip_and_stay_in_range() {
    for rgba in [[0.0, 0.5, 1.0, 1.0], [1.0, 0.0, 0.25, 0.5]] {
        let back = color_rgba(rgba_color(rgba));
        for (a, b) in rgba.iter().zip(back) {
            assert!((a - b).abs() < 0.01, "{rgba:?} {back:?}");
        }
    }
    assert_eq!(
        rgba_color([2.0, -1.0, f32::NAN, 1.0]).to_array(),
        [255, 0, 0, 255]
    );
}

#[test]
fn axis_captions_read_on_every_field_surface() {
    for dark in [true, false] {
        let t = Theme::default().tokens(dark);
        for color in t.axis {
            for surface in [t.bg_panel, t.bg_raised, t.bg_input] {
                let ratio = contrast_ratio(color, surface);
                assert!(
                    ratio >= 4.5,
                    "dark {dark}: {color:?} on {surface:?} is {ratio:.2}"
                );
            }
        }
    }
}

#[test]
fn the_overflow_menu_holds_controls_that_stay_open_while_they_are_used() {
    let mut h = harness(0_u32, |ui, count| {
        overflow_menu(ui, "More options", |ui| {
            if ui.add(Button::new("Add one")).clicked() {
                *count += 1;
            }
        });
    });
    h.run_steps(2);
    assert!(h.query_by_label("Add one").is_none());
    h.get_by_label("More options").click();
    h.run_steps(3);
    h.get_by_label("Add one").click();
    h.run_steps(3);
    h.get_by_label("Add one").click();
    h.run_steps(3);
    assert_eq!(
        *h.state(),
        2,
        "the menu did not close after the first click"
    );
}

#[test]
fn a_palette_modal_has_no_heading_and_escape_cancels_it() {
    let mut h = harness(false, |ui, cancelled| {
        let out = Modal::new("p", "Commands")
            .palette()
            .width(520.0)
            .show(ui.ctx(), |ui| {
                ui.label("Type a command");
            });
        *cancelled |= out.cancelled;
    });
    h.run_steps(3);
    assert!(h.query_by_label("Commands").is_none(), "no heading");
    h.get_by_label("Type a command");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(*h.state());
}
