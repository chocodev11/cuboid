//! The PNG render dialog and the toast that carries a button.
//!
//! The dialog collects size, background, camera, shading and anti-aliasing; the app draws the
//! picture with the renderer's offscreen path over several frames and writes the file on its I/O
//! thread. Nothing here waits: the dialog shows a spinner until [`Ui::render_finished`](crate::Ui)
//! is called.

use std::path::PathBuf;

use cuboid_ui_kit::{Icon, Level, RADIUS_LG, Tokens};
use egui::{Align2, Area, Context, Id, Modal, Order, RichText, Stroke, Ui, vec2};

use crate::common::Sink;
use crate::export_dialog::{Location, modal_dismissed};
use crate::request::UiRequest;

/// Where the picture is taken from.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum RenderCamera {
    /// The view that is active in the window.
    #[default]
    Current,
    /// Looking at the front.
    Front,
    /// Looking from the side.
    Side,
    /// Looking down.
    Top,
    /// A three-quarter view from above.
    Iso,
}

impl RenderCamera {
    /// All cameras in list order.
    pub const ALL: [RenderCamera; 5] = [
        RenderCamera::Current,
        RenderCamera::Front,
        RenderCamera::Side,
        RenderCamera::Top,
        RenderCamera::Iso,
    ];

    /// Button label.
    pub fn label(self) -> &'static str {
        match self {
            RenderCamera::Current => "Current view",
            RenderCamera::Front => "Front",
            RenderCamera::Side => "Side",
            RenderCamera::Top => "Top",
            RenderCamera::Iso => "Iso",
        }
    }
}

/// How faces are lit in the picture.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum RenderShading {
    /// The shading mode of the window.
    #[default]
    Current,
    /// Lit like an item in the Minecraft inventory.
    ItemGui,
}

/// What is behind the model.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum RenderBackground {
    /// Transparent pixels.
    #[default]
    Transparent,
    /// A flat colour.
    Color([u8; 3]),
}

/// The choices of one render.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RenderSettings {
    /// Width and height of the picture in pixels.
    pub size: [u32; 2],
    /// The background.
    pub background: RenderBackground,
    /// The camera.
    pub camera: RenderCamera,
    /// The lighting.
    pub shading: RenderShading,
    /// Render at twice the size and reduce it, for smooth edges.
    pub msaa: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            size: [1024, 1024],
            background: RenderBackground::Transparent,
            camera: RenderCamera::Current,
            shading: RenderShading::Current,
            msaa: true,
        }
    }
}

/// Smallest and largest edge a custom size may have.
pub const RENDER_EDGE: (u32, u32) = (16, 8192);

/// What the dialog asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderRequest {
    /// Draw the picture and write it to `path`.
    Start {
        /// The choices.
        settings: RenderSettings,
        /// The PNG file.
        path: PathBuf,
    },
    /// Stop the render that runs.
    Cancel,
}

const PRESETS: [u32; 3] = [512, 1024, 2048];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Stage {
    Setup,
    Rendering,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum RenderEvent {
    None,
    Close,
}

pub(crate) struct RenderDialog {
    pub settings: RenderSettings,
    pub location: Location,
    stage: Stage,
    error: Option<String>,
    /// Index into the presets, or the length for a custom size.
    size_choice: usize,
    transparent: bool,
    color: [u8; 3],
    /// The colour as typed.
    hex: String,
}

/// Background colours to pick from.
const SWATCHES: [(&str, [u8; 3]); 6] = [
    ("White", [255, 255, 255]),
    ("Light grey", [200, 200, 200]),
    ("Dark grey", [48, 48, 52]),
    ("Black", [0, 0, 0]),
    ("Sky", [135, 190, 235]),
    ("Grass", [110, 170, 80]),
];

fn hex_of([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#rrggbb` or `rrggbb`.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

impl RenderDialog {
    pub fn new(location: Location, last: RenderSettings) -> Self {
        let size_choice = PRESETS
            .iter()
            .position(|&edge| last.size == [edge, edge])
            .unwrap_or(PRESETS.len());
        let (transparent, color) = match last.background {
            RenderBackground::Transparent => (true, [255, 255, 255]),
            RenderBackground::Color(color) => (false, color),
        };
        Self {
            settings: last,
            location,
            stage: Stage::Setup,
            error: None,
            size_choice,
            transparent,
            color,
            hex: hex_of(color),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.location.path("png")
    }

    /// The render failed; the user can change the choices and try again.
    pub fn failed(&mut self, text: String) {
        self.stage = Stage::Setup;
        self.error = Some(text);
    }

    fn start(&mut self, sink: &mut Sink) {
        self.settings.background = if self.transparent {
            RenderBackground::Transparent
        } else {
            RenderBackground::Color(self.color)
        };
        self.settings.size = self
            .settings
            .size
            .map(|edge| edge.clamp(RENDER_EDGE.0, RENDER_EDGE.1));
        self.stage = Stage::Rendering;
        self.error = None;
        sink.push(UiRequest::Render(RenderRequest::Start {
            settings: self.settings,
            path: self.path(),
        }));
    }

    pub fn show(&mut self, ctx: &Context, sink: &mut Sink) -> RenderEvent {
        let tokens = Tokens::of(ctx);
        let mut event = RenderEvent::None;
        let modal = Modal::new(Id::unique("cuboid_render_dialog")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                Icon::Image.paint(ui.painter(), rect, tokens.accent);
                ui.label(
                    RichText::new("Render a picture")
                        .size(16.0)
                        .strong()
                        .color(tokens.text_strong),
                );
            });
            ui.add_space(8.0);
            match self.stage {
                Stage::Setup => event = self.setup(ui, &tokens, sink),
                Stage::Rendering => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(format!("Rendering {}...", self.path().display()));
                    });
                    ui.add_space(12.0);
                    if ui.button("Cancel").clicked() {
                        sink.push(UiRequest::Render(RenderRequest::Cancel));
                        event = RenderEvent::Close;
                    }
                }
            }
        });
        if event == RenderEvent::None && modal_dismissed(ctx, &modal) {
            if self.stage == Stage::Rendering {
                sink.push(UiRequest::Render(RenderRequest::Cancel));
            }
            event = RenderEvent::Close;
        }
        event
    }

    /// Swatches and a hex field; a colour picker is a popup, which a modal does not pass clicks to.
    fn color_choice(&mut self, ui: &mut Ui) {
        for (name, rgb) in SWATCHES {
            let [r, g, b] = rgb;
            let chip = cuboid_ui_kit::color_swatch(
                ui,
                egui::Color32::from_rgb(r, g, b),
                vec2(20.0, 20.0),
                self.color == rgb,
            );
            if chip.on_hover_text(name).clicked() {
                self.color = rgb;
                self.hex = hex_of(rgb);
            }
        }
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.hex)
                .desired_width(72.0)
                .hint_text("#rrggbb"),
        );
        if edit.changed()
            && let Some(rgb) = parse_hex(&self.hex)
        {
            self.color = rgb;
        }
    }

    fn setup(&mut self, ui: &mut Ui, tokens: &Tokens, sink: &mut Sink) -> RenderEvent {
        let mut event = RenderEvent::None;
        egui::Grid::new("cuboid_render_choices")
            .num_columns(2)
            .spacing(vec2(8.0, 8.0))
            .show(ui, |ui| {
                ui.label("Size");
                ui.vertical(|ui| {
                    let mut labels: Vec<String> =
                        PRESETS.iter().map(|edge| format!("{edge}")).collect();
                    labels.push("Custom".to_owned());
                    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
                    if cuboid_ui_kit::segmented(ui, &mut self.size_choice, &names)
                        && let Some(&edge) = PRESETS.get(self.size_choice)
                    {
                        self.settings.size = [edge, edge];
                    }
                    if self.size_choice == PRESETS.len() {
                        ui.horizontal(|ui| {
                            let range = RENDER_EDGE.0..=RENDER_EDGE.1;
                            ui.add(
                                egui::DragValue::new(&mut self.settings.size[0])
                                    .range(range.clone())
                                    .suffix(" px"),
                            );
                            ui.label("x");
                            ui.add(
                                egui::DragValue::new(&mut self.settings.size[1])
                                    .range(range)
                                    .suffix(" px"),
                            );
                        });
                    }
                });
                ui.end_row();

                ui.label("Background");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.transparent, true, "Transparent");
                    ui.selectable_value(&mut self.transparent, false, "Colour");
                    if !self.transparent {
                        self.color_choice(ui);
                    }
                });
                ui.end_row();

                ui.label("Camera");
                ui.horizontal_wrapped(|ui| {
                    for camera in RenderCamera::ALL {
                        ui.selectable_value(&mut self.settings.camera, camera, camera.label());
                    }
                });
                ui.end_row();

                ui.label("Shading");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings.shading,
                        RenderShading::Current,
                        "Current mode",
                    );
                    ui.selectable_value(
                        &mut self.settings.shading,
                        RenderShading::ItemGui,
                        "Minecraft item GUI",
                    );
                });
                ui.end_row();

                ui.label("Smoothing");
                ui.checkbox(&mut self.settings.msaa, "MSAA (renders at twice the size)");
                ui.end_row();
            });
        ui.add_space(8.0);
        self.location.show(ui, "png", sink);
        if let Some(error) = &self.error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).color(tokens.danger));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let go = ui.add_enabled(
                self.location.is_complete(),
                egui::Button::new(RichText::new("Render").color(tokens.on_accent))
                    .fill(tokens.accent)
                    .stroke(Stroke::NONE)
                    .min_size(vec2(96.0, 28.0)),
            );
            if go.clicked() {
                self.start(sink);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(vec2(80.0, 28.0)))
                .clicked()
            {
                event = RenderEvent::Close;
            }
        });
        event
    }
}

/// A message in the corner with one button, for results the user may act on.
pub(crate) struct ActionToast {
    level: Level,
    text: String,
    label: String,
    request: UiRequest,
    born: Option<f64>,
}

/// Seconds an action toast stays.
const LIFETIME: f64 = 12.0;

impl ActionToast {
    pub fn new(level: Level, text: String, label: String, request: UiRequest) -> Self {
        Self {
            level,
            text,
            label,
            request,
            born: None,
        }
    }

    /// The path the render opens a folder for, for tests.
    #[cfg(test)]
    pub fn target(&self) -> Option<&std::path::Path> {
        match &self.request {
            UiRequest::OpenFolder(path) => Some(path),
            _ => None,
        }
    }

    /// Draws the toast; true while it stays. The button's request goes to `sink`.
    pub fn show(&mut self, ctx: &Context, sink: &mut Sink) -> bool {
        let now = ctx.input(|i| i.time);
        let born = *self.born.get_or_insert(now);
        if now - born > LIFETIME {
            return false;
        }
        let tokens = Tokens::of(ctx);
        let (accent, icon) = match self.level {
            Level::Info => (tokens.accent, Icon::Info),
            Level::Success => (tokens.ok, Icon::Check),
            Level::Warning => (tokens.warn, Icon::Warning),
            Level::Error => (tokens.danger, Icon::Error),
        };
        let mut keep = true;
        Area::new(Id::unique("cuboid_action_toast"))
            .anchor(Align2::RIGHT_TOP, vec2(-16.0, 84.0))
            .order(Order::Tooltip)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                egui::Frame::new()
                    .fill(tokens.bg_raised)
                    .stroke(Stroke::new(1.0, tokens.border_strong))
                    .corner_radius(RADIUS_LG)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                            icon.paint(ui.painter(), rect, accent);
                            ui.label(RichText::new(&self.text).color(tokens.text));
                        });
                        ui.horizontal(|ui| {
                            if ui.button(&self.label).clicked() {
                                sink.push(self.request.clone());
                                keep = false;
                            }
                            if ui.button("Dismiss").clicked() {
                                keep = false;
                            }
                        });
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        keep
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn dialog() -> RenderDialog {
        RenderDialog::new(
            Location::new(Path::new("out"), "chair"),
            RenderSettings::default(),
        )
    }

    #[test]
    fn hex_colours_parse_and_print() {
        assert_eq!(parse_hex("#0a1B2c"), Some([10, 27, 44]));
        assert_eq!(parse_hex("ffffff"), Some([255, 255, 255]));
        for bad in ["", "#12345", "#1234567", "#gg0000", "#12é456"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(hex_of([10, 27, 44]), "#0a1b2c");
    }

    #[test]
    fn the_last_size_selects_its_preset() {
        assert_eq!(dialog().size_choice, 1);
        let custom = RenderSettings {
            size: [300, 200],
            ..RenderSettings::default()
        };
        let d = RenderDialog::new(Location::new(Path::new("out"), "a"), custom);
        assert_eq!(d.size_choice, PRESETS.len());
    }

    #[test]
    fn starting_clamps_the_size_and_asks_for_a_render() {
        let mut d = dialog();
        d.settings.size = [4, 99_999];
        d.transparent = false;
        d.color = [10, 20, 30];
        let mut sink = Sink::default();
        d.start(&mut sink);
        let [UiRequest::Render(RenderRequest::Start { settings, path })] = sink.requests.as_slice()
        else {
            panic!("expected one render request, got {:?}", sink.requests);
        };
        assert_eq!(settings.size, [RENDER_EDGE.0, RENDER_EDGE.1]);
        assert_eq!(settings.background, RenderBackground::Color([10, 20, 30]));
        assert_eq!(path, &Path::new("out").join("chair.png"));
        assert_eq!(d.stage, Stage::Rendering);
        d.failed("no memory".into());
        assert_eq!(d.stage, Stage::Setup);
        assert_eq!(d.error.as_deref(), Some("no memory"));
    }

    #[test]
    fn the_action_toast_sends_its_request_once() {
        let ctx = Context::default();
        let mut toast = ActionToast::new(
            Level::Success,
            "Rendered".into(),
            "Open folder".into(),
            UiRequest::OpenFolder(PathBuf::from("out")),
        );
        assert_eq!(toast.target(), Some(Path::new("out")));
        let mut sink = Sink::default();
        let mut alive = true;
        ctx.run_pass(egui::RawInput::default(), |ctx| {
            alive = toast.show(ctx, &mut sink);
        })
        .drop_without_applying_deltas();
        assert!(alive);
        assert!(sink.requests.is_empty());
    }
}
