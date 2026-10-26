//! The Texture tab: the properties of the selected texture. The texture list is the Textures
//! panel.

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{height, radius, space};
use cuboid_ui_kit::{
    Button, DragFieldOpts, EmptyState, Icon, IconButton, ListRow, PropertyGrid, Section, Tokens,
    Type, checkbox, drag_field, fit_centered, format_number, paint_image, text,
};
use egui::{Label, ScrollArea, Sense, Ui, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::{DocView, FramesView, TextureView};
use crate::request::{Edit, FramesEdit, FramesRequest, UiRequest};

/// Tallest the preview grows, in points.
const PREVIEW_MAX: f32 = 160.0;

/// How many steps the list shows before it scrolls.
const STEP_ROWS: f32 = 6.0;

/// The longest time of a frame, in ticks, as the model limits it.
const MAX_TICKS: f32 = 72_000.0;

/// Game ticks per second.
const TICKS_PER_SECOND: f32 = 20.0;

/// The page of the tab.
pub(crate) fn show(ui: &mut Ui, doc: &DocView<'_>, images: &mut ImageCache, sink: &mut Sink) {
    let Some((index, texture)) = doc
        .active_texture
        .and_then(|i| doc.textures.get(i).map(|t| (i, t)))
    else {
        let add = EmptyState::new(Icon::Texture, "No texture selected")
            .body("Add a texture, or pick one in the Textures panel.")
            .action(Button::new("Add texture").icon(Icon::Plus))
            .show(ui);
        if add.is_some() {
            sink.action(ActionId::AddTexture);
        }
        return;
    };
    preview(ui, texture, images);
    ui.add_space(space::MD);
    Section::new("Details").show(ui, "props_texture", |ui| {
        PropertyGrid::new("props_texture").show(ui, |grid| {
            grid.row("Name", |ui| {
                ui.add(Label::new(text(Type::Body, &*texture.label)).truncate());
            });
            grid.row("Size", |ui| {
                let [w, h] = texture.size;
                let mut size = format!("{w} x {h} px");
                if let Some(frames) = &texture.animation {
                    let plural = if frames.tiles == 1 { "" } else { "s" };
                    size = format!("{size}, {} frame{plural}", frames.tiles);
                }
                ui.add(Label::new(text(Type::Body, size)).selectable(false));
            });
            grid.row("Source", |ui| match &texture.source {
                Some(path) => {
                    ui.add(Label::new(text(Type::Body, &**path)).truncate())
                        .on_hover_text(&**path);
                }
                None => {
                    ui.add(Label::new(text(Type::Label, "Not loaded from a file")).truncate());
                }
            });
        });
    });
    frames(ui, index, texture, sink);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(Button::new("Replace").icon(Icon::Replace))
            .on_hover_text("Replace the pixels with a picture from a file")
            .clicked()
        {
            sink.action(ActionId::LoadTexture);
        }
        let reload = ui.add_enabled(
            texture.source.is_some(),
            Button::new("Reload").icon(Icon::Reload),
        );
        if reload
            .on_hover_text("Read the file the texture came from again")
            .clicked()
        {
            sink.edit(Edit::ReloadTexture(index));
        }
        if ui
            .add(Button::new("Export").icon(Icon::Export))
            .on_hover_text("Save the texture as a picture file")
            .clicked()
        {
            sink.action(ActionId::SaveTexture);
        }
    });
}

fn ask(sink: &mut Sink, request: FramesRequest) {
    sink.push(UiRequest::Frames(request));
}

/// The animation as the view has it, ready to change one thing of.
fn edit_of(view: &FramesView) -> FramesEdit {
    FramesEdit {
        frame_height: view.frame_height,
        frametime: view.frametime,
        interpolate: view.interpolate,
        steps: if view.explicit {
            steps_of(view)
        } else {
            Vec::new()
        },
    }
}

fn steps_of(view: &FramesView) -> Vec<(u32, Option<u32>)> {
    view.steps.iter().map(|s| (s.tile, s.time)).collect()
}

/// The animation with another list of steps. The plain loop over the tiles is no list.
fn with_steps(view: &FramesView, steps: Vec<(u32, Option<u32>)>) -> FramesEdit {
    let plain = steps
        .iter()
        .copied()
        .eq((0..view.tiles).map(|tile| (tile, None)));
    FramesEdit {
        steps: if plain { Vec::new() } else { steps },
        ..edit_of(view)
    }
}

/// The Frames section: the play order of an animated texture and the buttons that change it, or
/// the button that makes a still texture a strip.
///
/// spec: textures T1 5
fn frames(ui: &mut Ui, index: usize, texture: &TextureView, sink: &mut Sink) {
    let Some(view) = &texture.animation else {
        Section::new("Frames").show(ui, "props_frames", |ui| {
            let animate = Button::new("Animate").icon(Icon::Film);
            let frame = texture.animatable;
            if ui
                .add_enabled(frame.is_some(), animate)
                .on_hover_text("Play the picture as a vertical strip of square frames")
                .clicked()
                && let Some(frame_height) = frame
            {
                let edit = FramesEdit {
                    frame_height,
                    frametime: 1,
                    interpolate: false,
                    steps: Vec::new(),
                };
                ask(
                    sink,
                    FramesRequest::Set {
                        texture: index,
                        animation: Some(edit),
                    },
                );
            }
            if frame.is_none() {
                ui.label(text(
                    Type::Caption,
                    "The picture must be a whole number of squares high.",
                ));
            }
        });
        return;
    };
    Section::new("Frames").show(ui, "props_frames", |ui| {
        let set = |sink: &mut Sink, edit: FramesEdit| {
            ask(
                sink,
                FramesRequest::Set {
                    texture: index,
                    animation: Some(edit),
                },
            );
        };
        PropertyGrid::new("props_frames").show(ui, |grid| {
            grid.row("Frame time", |ui| {
                let mut ticks = view.frametime as f32;
                let opts = tick_field("Frame time");
                if drag_field(ui, "frames_frametime", &mut ticks, &opts).committed {
                    set(
                        sink,
                        FramesEdit {
                            frametime: ticks_of(ticks),
                            ..edit_of(view)
                        },
                    );
                }
            });
            grid.row("Interpolate", |ui| {
                let mut on = view.interpolate;
                let blend = checkbox(ui, &mut on, "Blend frames");
                if blend
                    .on_hover_text("Blend each frame into the next one over its time")
                    .changed()
                {
                    set(
                        sink,
                        FramesEdit {
                            interpolate: on,
                            ..edit_of(view)
                        },
                    );
                }
            });
        });
        let ticks: u32 = view
            .steps
            .iter()
            .map(|s| s.time.unwrap_or(view.frametime))
            .sum();
        let caption = format!(
            "{} steps of {} frames, {} s",
            view.steps.len(),
            view.tiles,
            format_number(ticks as f32 / TICKS_PER_SECOND, 2)
        );
        ui.label(text(Type::Caption, caption));
        ui.add_space(space::XS);
        step_list(ui, index, view, sink);
        step_buttons(ui, index, view, sink);
        if ui
            .add(Button::new("Make still"))
            .on_hover_text("Show the whole picture as one texture again")
            .clicked()
        {
            ask(
                sink,
                FramesRequest::Set {
                    texture: index,
                    animation: None,
                },
            );
        }
    });
}

/// The steps in play order, a row each: the one the views show is selected, and a click shows
/// another.
fn step_list(ui: &mut Ui, index: usize, view: &FramesView, sink: &mut Sink) {
    ScrollArea::vertical()
        .id_salt(("frames_steps", index))
        .max_height(height::ROW * STEP_ROWS)
        .auto_shrink([false, true])
        .show_rows(ui, height::ROW, view.steps.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for at in range {
                let Some(step) = view.steps.get(at) else {
                    continue;
                };
                let title = format!("Step {}", at + 1);
                let caption = match step.time {
                    Some(time) => format!("frame {}, {time} ticks", step.tile + 1),
                    None => format!("frame {}", step.tile + 1),
                };
                let row = ListRow::new(&title)
                    .caption(&caption)
                    .selected(view.shown == at);
                if ui.add(row).clicked() {
                    ask(
                        sink,
                        FramesRequest::Show {
                            texture: index,
                            step: at,
                        },
                    );
                }
            }
        });
}

/// The buttons and the time of the step the views show: move it, remove it, add a frame.
fn step_buttons(ui: &mut Ui, index: usize, view: &FramesView, sink: &mut Sink) {
    let at = view.shown;
    let last = view.steps.len().saturating_sub(1);
    let order = |sink: &mut Sink, steps: Vec<(u32, Option<u32>)>, show: usize| {
        let edit = with_steps(view, steps);
        ask(
            sink,
            FramesRequest::Set {
                texture: index,
                animation: Some(edit),
            },
        );
        ask(
            sink,
            FramesRequest::Show {
                texture: index,
                step: show,
            },
        );
    };
    ui.horizontal(|ui| {
        let swap = |sink: &mut Sink, other: usize| {
            let mut steps = steps_of(view);
            steps.swap(at, other);
            order(sink, steps, other);
        };
        ui.add_enabled_ui(at > 0, |ui| {
            if ui
                .add(IconButton::new(Icon::ArrowUp, "Move the step up"))
                .clicked()
            {
                swap(sink, at - 1);
            }
        });
        ui.add_enabled_ui(at < last, |ui| {
            if ui
                .add(IconButton::new(Icon::ArrowDown, "Move the step down"))
                .clicked()
            {
                swap(sink, at + 1);
            }
        });
        ui.add_enabled_ui(last > 0, |ui| {
            if ui
                .add(IconButton::new(Icon::Trash, "Remove the step"))
                .clicked()
            {
                let mut steps = steps_of(view);
                steps.remove(at);
                order(sink, steps, at.min(last - 1));
            }
        });
        if ui
            .add(IconButton::new(
                Icon::Plus,
                "Add a copy of the frame shown at the end of the strip",
            ))
            .clicked()
        {
            ask(sink, FramesRequest::AddFrame { texture: index });
            ask(
                sink,
                FramesRequest::Show {
                    texture: index,
                    step: view.steps.len(),
                },
            );
        }
    });
    let own = view.steps.get(at).and_then(|s| s.time);
    PropertyGrid::new("props_frames_step").show(ui, |grid| {
        grid.row("Step time", |ui| {
            let mut ticks = own.unwrap_or(view.frametime) as f32;
            let opts = tick_field("Time of the step");
            if drag_field(ui, "frames_step_time", &mut ticks, &opts).committed {
                let mut steps = steps_of(view);
                if let Some(step) = steps.get_mut(at) {
                    step.1 = Some(ticks_of(ticks));
                }
                order(sink, steps, at);
            }
        });
    });
    if own.is_some()
        && ui
            .add(Button::new("Use the frame time"))
            .on_hover_text("Give the step the default time again")
            .clicked()
    {
        let mut steps = steps_of(view);
        if let Some(step) = steps.get_mut(at) {
            step.1 = None;
        }
        order(sink, steps, at);
    }
}

/// A field of ticks, from 1 to 72000.
fn tick_field(name: &str) -> DragFieldOpts<'_> {
    DragFieldOpts {
        name,
        speed: 0.25,
        range: 1.0..=MAX_TICKS,
        unit: "ticks",
        decimals: 0,
        snap: Some(1.0),
        ..DragFieldOpts::default()
    }
}

/// The whole ticks a field value stands for.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ticks_of(value: f32) -> u32 {
    value.round().clamp(1.0, MAX_TICKS) as u32
}

/// The picture of the texture fitted into a frame as wide as the panel.
fn preview(ui: &mut Ui, texture: &TextureView, images: &mut ImageCache) {
    let tokens = Tokens::of(ui.ctx());
    let width = ui.available_width();
    let (frame, _) = ui.allocate_exact_size(
        vec2(width, (width * 3.0 / 4.0).clamp(height::BAR, PREVIEW_MAX)),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(frame, radius::CONTROL, tokens.bg_input);
    let Some(image) = &texture.image else { return };
    let key = format!("texture:{}", texture.label);
    let Some(id) = images.texture(ui.ctx(), &key, image) else {
        return;
    };
    let [w, h] = image.size;
    #[allow(clippy::cast_precision_loss)]
    let picture = vec2(w as f32, h as f32);
    paint_image(
        ui.painter(),
        id,
        fit_centered(frame.shrink(space::XS), picture),
    );
}
