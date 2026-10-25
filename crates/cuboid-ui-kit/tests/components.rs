//! Component state rendering in headless frames: which fill and which ink each state paints, the
//! sizes of the specification, and the keyboard behaviour.

mod common;

use cuboid_ui_kit::metrics::{height, label_column};
use cuboid_ui_kit::{
    Button, ControlState, DragFieldOpts, FontSet, Icon, IconButton, ListRow, Modal, ModalButton,
    PropertyGrid, Theme, Tokens, Tone, Type, badge, checkbox, chip, drag_field, forced_state,
    install_fonts, segmented, tab_button, tabs, text, toggle,
};
use egui::{Color32, Context, Key, Modifiers, RawInput, Rect, Shape, Ui, vec2};
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
