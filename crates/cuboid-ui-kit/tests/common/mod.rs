//! The component gallery: every component and state of the kit on a few sheets, built once and
//! used by the PNG writer (`gallery.rs`) and by the smoke tests.

#![allow(dead_code)]

use cuboid_ui_kit::metrics::{gutter, height, radius, space};
use cuboid_ui_kit::{
    Button, Card, ControlState, DragFieldOpts, DropHint, EmptyState, FontSet, Icon, IconButton,
    Level, ListRow, Modal, ModalButton, PanelFrame, PropertyGrid, RowToggle, Section, SliderOpts,
    Surface, Theme, ThemeMode, Toasts, Tokens, Tone, Toolbar, TreeRow, Type, badge, checkbox, chip,
    color_swatch, drag_field, dropdown, forced_state, install_fonts, key_chip, segmented, slider,
    tab_button, text, text_field, toggle, tree_row, vector_field,
};
use egui::{
    Align, Color32, ColorImage, Context, Frame, Layout, Margin, RichText, TextureHandle,
    TextureOptions, Ui, UiBuilder, vec2,
};

/// Which sheet to draw.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Sheet {
    /// Typography, surfaces, the state matrix and the anatomy of the structure components.
    Components,
    /// A modal over a view with toasts.
    Overlays,
    /// Every icon with its Phosphor name.
    Icons,
}

impl Sheet {
    pub const ALL: [Sheet; 3] = [Sheet::Components, Sheet::Overlays, Sheet::Icons];

    pub fn name(self) -> &'static str {
        match self {
            Sheet::Components => "components",
            Sheet::Overlays => "overlays",
            Sheet::Icons => "icons",
        }
    }
}

/// State the gallery keeps between frames.
pub struct Gallery {
    pub theme: Theme,
    pub sheet: Sheet,
    texture: Option<TextureHandle>,
    toasts: Toasts,
    pub last_modal: Option<usize>,
    pub text: String,
    pub choice: usize,
    pub tab: usize,
}

impl Gallery {
    pub fn new(theme: Theme, sheet: Sheet) -> Self {
        let mut toasts = Toasts::default();
        toasts.push(Level::Success, "Saved lantern.cubik");
        toasts.push_with_action(Level::Warning, "2 faces have no texture", "Fix", 1);
        toasts.push(Level::Error, "Could not read palette.png");
        Self {
            theme,
            sheet,
            texture: None,
            toasts,
            last_modal: None,
            text: "Lantern".to_owned(),
            choice: 1,
            tab: 1,
        }
    }
}

/// The theme used by the PNG writer: Mocha or Latte at a UI scale.
pub fn theme(light: bool, scale: f32) -> Theme {
    Theme {
        mode: if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        scale,
        ..Theme::default()
    }
}

/// Installs the fonts; run before the first frame.
pub fn setup(ctx: &Context) {
    install_fonts(ctx, &FontSet::default());
}

/// A brick-like picture for thumbnails.
fn picture(w: usize, h: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let band = (x / 8 + y / 8) % 2 == 0;
            pixels.push(if band {
                Color32::from_rgb(196, 120, 72)
            } else {
                Color32::from_rgb(150, 86, 52)
            });
        }
    }
    ColorImage::new([w, h], pixels)
}

/// Draws one sheet. Returns nothing; the harness fits the window to the content.
pub fn show(ui: &mut Ui, g: &mut Gallery) {
    g.theme.apply(ui.ctx());
    let t = Tokens::of(ui.ctx());
    if g.texture.is_none() {
        g.texture = Some(ui.ctx().load_texture(
            "gallery_picture",
            picture(64, 48),
            TextureOptions::NEAREST,
        ));
    }
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(t.surface(Surface::App))
                .inner_margin(Margin::same(24)),
        )
        .show(ui, |ui| match g.sheet {
            Sheet::Components => components(ui, g, &t),
            Sheet::Overlays => overlays(ui, g, &t),
            Sheet::Icons => icons(ui, &t),
        });
}

fn region<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(t.surface(Surface::Region))
        .corner_radius(radius::CARD)
        .inner_margin(Margin::same(16))
        .show(ui, add)
        .inner
}

fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(gutter::SECTION);
    ui.label(text(Type::Heading, title));
    ui.add_space(space::SM);
}

fn components(ui: &mut Ui, g: &mut Gallery, t: &Tokens) {
    ui.set_width(1100.0);
    ui.label(text(Type::Display, "cuboid ui-kit v2"));
    typography(ui, t);
    surfaces(ui, t);
    state_matrix(ui, g, t);
    anatomy(ui, g, t);
}

fn typography(ui: &mut Ui, t: &Tokens) {
    heading(ui, "Typography");
    region(ui, t, |ui| {
        ui.spacing_mut().item_spacing.y = space::SM;
        for role in Type::ALL {
            let name = format!("{role:?}");
            let sample = format!(
                "{name} - {} pt, {:?}: The quick brown fox jumps over 0123456789 sizes",
                role.size(),
                role.weight()
            );
            ui.label(text(role, sample));
        }
    });
}

fn surfaces(ui: &mut Ui, t: &Tokens) {
    heading(ui, "Surfaces");
    ui.horizontal(|ui| {
        for level in Surface::ALL {
            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 40.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, radius::CONTROL, t.surface(level));
            ui.painter().rect_stroke(
                rect,
                radius::CONTROL,
                egui::Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{level:?}"),
                Type::Caption.font(),
                t.text_weak,
            );
        }
        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, radius::CONTROL, t.surface(Surface::Region));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                axis,
                Type::Strong.font(),
                t.axis[i],
            );
        }
        for (name, colour) in [
            ("Accent", t.accent),
            ("Danger", t.danger),
            ("Warning", t.warn),
            ("Success", t.ok),
        ] {
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 40.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, radius::CONTROL, colour);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                name,
                Type::Caption.font(),
                t.on_accent,
            );
        }
    });
}

/// One row of the state matrix: the component name and one cell per state.
fn matrix_row(ui: &mut Ui, name: &str, mut cell: impl FnMut(&mut Ui, ControlState)) {
    ui.horizontal(|ui| {
        let (label_rect, _) = ui.allocate_exact_size(vec2(150.0, 32.0), egui::Sense::hover());
        let mut label = ui.new_child(
            UiBuilder::new()
                .max_rect(label_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        label.label(text(Type::Label, name));
        for state in ControlState::ALL {
            ui.push_id((name, state), |ui| {
                ui.allocate_ui_with_layout(
                    vec2(136.0, 32.0),
                    Layout::left_to_right(Align::Center),
                    |ui| forced_state(ui, state, |ui| cell(ui, state)),
                );
            });
        }
    });
}

fn state_matrix(ui: &mut Ui, g: &mut Gallery, t: &Tokens) {
    heading(ui, "States");
    let tex = g.texture.as_ref().map(TextureHandle::id);
    region(ui, t, |ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        ui.horizontal(|ui| {
            ui.add_space(150.0 + ui.spacing().item_spacing.x);
            for state in ControlState::ALL {
                let (rect, _) = ui.allocate_exact_size(vec2(136.0, 16.0), egui::Sense::hover());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    state.name(),
                    Type::Caption.font(),
                    t.text_weak,
                );
            }
        });
        matrix_row(ui, "Primary", |ui, _| {
            ui.add(Button::new("Save").primary());
        });
        matrix_row(ui, "Primary 32, icon", |ui, _| {
            ui.add(Button::new("Open").primary().tall().icon(Icon::FolderOpen));
        });
        matrix_row(ui, "Secondary", |ui, s| {
            ui.add(Button::new("Cancel").selected(s == ControlState::Selected));
        });
        matrix_row(ui, "Ghost", |ui, s| {
            ui.add(
                Button::new("Details")
                    .ghost()
                    .selected(s == ControlState::Selected),
            );
        });
        matrix_row(ui, "Danger", |ui, _| {
            ui.add(Button::new("Delete").danger().icon(Icon::Trash));
        });
        matrix_row(ui, "IconButton", |ui, s| {
            ui.add(
                IconButton::new(Icon::Settings, "Settings").selected(s == ControlState::Selected),
            );
        });
        matrix_row(ui, "IconButton, tool", |ui, s| {
            ui.add(
                IconButton::new(Icon::Move, "Move")
                    .toolbar()
                    .active(s == ControlState::Selected),
            );
        });
        matrix_row(ui, "Tab", |ui, s| {
            tab_button(ui, "Element", s == ControlState::Selected);
        });
        matrix_row(ui, "Segmented", |ui, s| {
            let mut current = usize::from(s == ControlState::Selected);
            segmented(ui, &mut current, &["Local", "Global"]);
        });
        matrix_row(ui, "Chip", |ui, s| {
            chip(ui, "Textured", s == ControlState::Selected);
        });
        matrix_row(ui, "Checkbox", |ui, s| {
            let mut on = s == ControlState::Selected;
            checkbox(ui, &mut on, "Mirror");
        });
        matrix_row(ui, "Toggle", |ui, s| {
            let mut on = s == ControlState::Selected;
            toggle(ui, &mut on, "Snap");
        });
        matrix_row(ui, "DragValue", |ui, _| {
            let mut v = 12.5;
            drag_field(
                ui,
                "dv",
                &mut v,
                &DragFieldOpts {
                    name: "Width",
                    unit: "px",
                    width: Some(110.0),
                    ..DragFieldOpts::default()
                },
            );
        });
        matrix_row(ui, "TextField", |ui, _| {
            let mut s = String::from("Lantern");
            ui.allocate_ui(vec2(120.0, 24.0), |ui| {
                text_field(ui, "tf", &mut s, "Name");
            });
        });
        matrix_row(ui, "Dropdown", |ui, _| {
            let mut current = 1;
            ui.allocate_ui(vec2(120.0, 24.0), |ui| {
                dropdown(ui, "dd", &mut current, &["Nearest", "Linear"]);
            });
        });
        matrix_row(ui, "Slider", |ui, _| {
            let mut v = 0.4;
            ui.allocate_ui(vec2(120.0, 24.0), |ui| {
                slider(
                    ui,
                    "sl",
                    &mut v,
                    0.0..=1.0,
                    &SliderOpts {
                        name: "Opacity",
                        ..SliderOpts::default()
                    },
                );
            });
        });
        matrix_row(ui, "ColorSwatch", |ui, s| {
            color_swatch(
                ui,
                Color32::from_rgb(196, 120, 72),
                vec2(40.0, height::CONTROL),
                s == ControlState::Selected,
            );
        });
        matrix_row(ui, "ListRow", |ui, s| {
            ui.allocate_ui(vec2(128.0, 28.0), |ui| {
                ui.add(
                    ListRow::new("Lantern")
                        .icon(Icon::Cube)
                        .caption("12")
                        .selected(s == ControlState::Selected),
                );
            });
        });
        let _ = tex;
    });
}

fn anatomy(ui: &mut Ui, g: &mut Gallery, t: &Tokens) {
    heading(ui, "Anatomy");
    let tex = g.texture.as_ref().map(|h| (h.id(), vec2(64.0, 48.0)));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gutter::GROUP;
        // The properties panel.
        ui.allocate_ui_with_layout(vec2(340.0, 560.0), Layout::top_down(Align::Min), |ui| {
            Frame::new()
                .fill(t.surface(Surface::Region))
                .corner_radius(radius::CARD)
                .show(ui, |ui| {
                    ui.set_min_size(vec2(340.0, 560.0));
                    ui.set_max_size(vec2(340.0, 560.0));
                    PanelFrame::new("Element").icon(Icon::Cube).show(
                        ui,
                        |ui| {
                            ui.add(IconButton::new(Icon::More, "More"));
                            ui.add(IconButton::new(Icon::Eye, "Visible").shortcut("H"));
                        },
                        |ui| properties(ui, g),
                    );
                });
        });
        // Lists, toolbars, tabs.
        ui.allocate_ui_with_layout(vec2(300.0, 560.0), Layout::top_down(Align::Min), |ui| {
            region(ui, t, |ui| {
                ui.set_min_size(vec2(268.0, 528.0));
                lists(ui, g, tex);
            });
        });
        // Cards, empty state, badges.
        ui.allocate_ui_with_layout(vec2(420.0, 560.0), Layout::top_down(Align::Min), |ui| {
            region(ui, t, |ui| {
                ui.set_min_size(vec2(388.0, 528.0));
                cards(ui, tex);
            });
        });
    });
}

fn properties(ui: &mut Ui, g: &mut Gallery) {
    Section::new("Transform").show(ui, "sec_transform", |ui| {
        PropertyGrid::new("transform").show(ui, |grid| {
            let mut position = [8.0, 16.5, -4.0];
            let mut size = [4.0, 4.0, 12.0];
            let opts = DragFieldOpts {
                name: "Position",
                unit: "px",
                width: None,
                ..DragFieldOpts::default()
            };
            grid.row("Position", |ui| {
                vector_field(ui, "pos", &mut position, &opts);
            });
            grid.row("Size", |ui| {
                vector_field(
                    ui,
                    "size",
                    &mut size,
                    &DragFieldOpts {
                        name: "Size",
                        ..opts.clone()
                    },
                );
            });
            let mut rotation = 22.5;
            grid.row("Rotation angle", |ui| {
                drag_field(
                    ui,
                    "rot",
                    &mut rotation,
                    &DragFieldOpts {
                        name: "Rotation",
                        unit: "deg",
                        width: None,
                        ..DragFieldOpts::default()
                    },
                );
            });
            grid.row("A rather long label that is cut", |ui| {
                ui.label(text(Type::Body, "ellipsis"));
            });
        });
    });
    Section::new("Material")
        .collapsible(true)
        .show(ui, "sec_material", |ui| {
            PropertyGrid::new("material").show(ui, |grid| {
                grid.row("Name", |ui| {
                    text_field(ui, "name", &mut g.text, "Name");
                });
                grid.row("Filter", |ui| {
                    dropdown(
                        ui,
                        "filter",
                        &mut g.choice,
                        &["Nearest", "Linear", "Mipmap"],
                    );
                });
                let mut opacity = 0.75;
                grid.row("Opacity", |ui| {
                    slider(
                        ui,
                        "opacity",
                        &mut opacity,
                        0.0..=1.0,
                        &SliderOpts {
                            name: "Opacity",
                            ..SliderOpts::default()
                        },
                    );
                });
                let (mut on, mut mirror) = (true, false);
                grid.row("Shade", |ui| {
                    toggle(ui, &mut on, "Shade faces");
                });
                grid.row("Mirror", |ui| {
                    checkbox(ui, &mut mirror, "Across X");
                });
                grid.row("Tint", |ui| {
                    color_swatch(ui, Color32::from_rgb(196, 120, 72), vec2(40.0, 24.0), false);
                });
            });
        });
    Section::new("Folded")
        .collapsible(false)
        .show(ui, "sec_folded", |ui| {
            ui.label("hidden");
        });
}

fn lists(ui: &mut Ui, g: &mut Gallery, tex: Option<(egui::TextureId, egui::Vec2)>) {
    ui.spacing_mut().item_spacing.y = space::XS;
    Toolbar::horizontal().show(ui, |bar| {
        bar.tool(Icon::Select, true, "Select", Some("V"));
        bar.tool(Icon::Move, false, "Move", Some("G"));
        bar.tool(Icon::Rotate, false, "Rotate", Some("R"));
        bar.separator();
        bar.toggle(Icon::Magnet, true, "Snap");
        bar.button(Icon::Grid, "Grid");
    });
    Toolbar::vertical().show(ui, |bar| {
        bar.tool(Icon::Pencil, true, "Pencil", None);
        bar.tool(Icon::Eraser, false, "Eraser", None);
        bar.separator();
        bar.button(Icon::Fill, "Fill");
    });
    ui.add_space(space::SM);
    cuboid_ui_kit::tabs(ui, &mut g.tab, &["Element", "Faces", "Texture"]);
    ui.add_space(space::SM);
    ui.add(
        ListRow::new("Lantern body")
            .icon(Icon::Cube)
            .caption("6 faces"),
    );
    ui.add(ListRow::new("Selected row").icon(Icon::Mesh).selected(true));
    if let Some((id, size)) = tex {
        ui.add(
            ListRow::new("With thumbnail")
                .thumbnail(id, size)
                .caption("64 x 48"),
        );
    }
    ui.add_space(space::SM);
    for (depth, label, icon, selected, expanded, toggles) in [
        (
            0,
            "Root group",
            Icon::Group,
            false,
            Some(true),
            (true, false),
        ),
        (1, "Body", Icon::Cube, true, None, (true, false)),
        (1, "Glass (hidden)", Icon::Mesh, false, None, (false, false)),
        (1, "Frame (locked)", Icon::Voxel, false, None, (true, true)),
    ] {
        let list = [
            RowToggle {
                on: Icon::Eye,
                off: Icon::EyeOff,
                state: toggles.0,
                tip: "Visible",
            },
            RowToggle {
                on: Icon::Lock,
                off: Icon::Unlock,
                state: toggles.1,
                tip: "Locked",
            },
        ];
        ui.push_id(label, |ui| {
            tree_row(
                ui,
                label,
                TreeRow {
                    depth,
                    label,
                    icon,
                    expanded,
                    selected,
                    primary: selected,
                    dimmed: !toggles.0,
                    toggles: &list,
                    drop: if label == "Body" {
                        Some(DropHint::After)
                    } else {
                        None
                    },
                    editing: None,
                },
            );
        });
    }
}

fn cards(ui: &mut Ui, tex: Option<(egui::TextureId, egui::Vec2)>) {
    ui.horizontal_top(|ui| {
        for (title, caption, badge_text, state) in [
            (
                "lantern.cubik",
                "Edited 2 minutes ago",
                "cubik",
                ControlState::Normal,
            ),
            (
                "tower.bbmodel",
                "Edited yesterday",
                "bbmodel",
                ControlState::Hover,
            ),
        ] {
            ui.push_id(title, |ui| {
                forced_state(ui, state, |ui| {
                    let mut card = Card::new(title)
                        .caption(caption)
                        .badge(badge_text)
                        .pin(title.starts_with("lantern"))
                        .width(186.0);
                    if let Some((id, size)) = tex {
                        card = card.thumbnail(id, size);
                    }
                    card.show(ui);
                });
            });
        }
    });
    ui.add_space(space::MD);
    ui.horizontal(|ui| {
        badge(ui, "cubik", Tone::Neutral);
        badge(ui, "2 errors", Tone::Danger);
        badge(ui, "1 warning", Tone::Warning);
        badge(ui, "Valid", Tone::Success);
        key_chip(ui, "Ctrl+S");
    });
    ui.add_space(space::MD);
    ui.allocate_ui(vec2(388.0, 220.0), |ui| {
        EmptyState::new(Icon::Package, "No models yet")
            .body("Create one or open a file to start.")
            .action(Button::new("New block").primary().icon(Icon::Plus))
            .action(Button::new("Open file"))
            .show(ui);
    });
}

fn overlays(ui: &mut Ui, g: &mut Gallery, t: &Tokens) {
    ui.set_min_size(vec2(1000.0, 560.0));
    region(ui, t, |ui| {
        ui.set_min_size(vec2(950.0, 510.0));
        ui.label(text(Type::Title, "The view behind the dialog"));
        ui.label(text(Type::Body, "The backdrop dims it to 40 %."));
        ui.add(Button::new("Behind").primary());
    });
    let buttons = [
        ModalButton::new("Cancel"),
        ModalButton::danger("Discard"),
        ModalButton::primary("Save"),
    ];
    let out = Modal::new("gallery_modal", "Save changes?")
        .width(520.0)
        .buttons(&buttons)
        .show(ui.ctx(), |ui| {
            ui.label(text(
                Type::Body,
                "The model has unsaved changes. Save them before closing, or discard them.",
            ));
            ui.add_space(space::MD);
            PropertyGrid::new("modal_grid").show(ui, |grid| {
                grid.row("File", |ui| {
                    text_field(ui, "modal_name", &mut g.text, "Name");
                });
            });
        });
    g.last_modal = out.button;
    g.toasts.show(ui.ctx());
}

fn icons(ui: &mut Ui, t: &Tokens) {
    ui.set_width(1100.0);
    ui.label(text(Type::Heading, "Icons (Phosphor Regular)"));
    ui.add_space(space::SM);
    region(ui, t, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for &icon in Icon::ALL {
                let (rect, _) = ui.allocate_exact_size(vec2(130.0, 72.0), egui::Sense::hover());
                icon.paint(
                    ui.painter(),
                    egui::Rect::from_center_size(rect.center() - vec2(0.0, 10.0), vec2(24.0, 24.0)),
                    t.text,
                );
                ui.painter().text(
                    rect.center() + vec2(0.0, 18.0),
                    egui::Align2::CENTER_CENTER,
                    icon.name(),
                    Type::Caption.font(),
                    t.text_weak,
                );
            }
        });
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            for size in [16.0, 20.0, 40.0] {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(size + 16.0, size + 16.0), egui::Sense::hover());
                Icon::Cube.paint(
                    ui.painter(),
                    egui::Rect::from_center_size(rect.center(), vec2(size, size)),
                    t.text_strong,
                );
            }
            let _: Option<RichText> = None;
        });
    });
}
