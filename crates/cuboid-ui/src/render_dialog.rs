//! The PNG render dialog and the toast that carries a button.
//!
//! The dialog collects size, background, camera, shading and anti-aliasing; the app draws the
//! picture with the renderer's offscreen path over several frames and writes the file on its I/O
//! thread. Nothing here waits: the dialog shows a spinner until [`Ui::render_finished`](crate::Ui)
//! is called.

use std::path::PathBuf;

use cuboid_ui_kit::metrics::{TOAST_WIDTH, height, space};
use cuboid_ui_kit::{
    Button, DragFieldOpts, Icon, Level, Modal, ModalButton, PropertyGrid, Tokens, Type,
    color_swatch, drag_field, dropdown, rgb_color, segmented, text, text_field, toggle,
};
use egui::{Align2, Area, Context, Frame, Id, Order, Ui, Vec2, vec2};

use crate::common::Sink;
use crate::dialogs::{fixed_body, status_line};
use crate::export_dialog::{Location, working};
use crate::request::UiRequest;

/// Height of the body of the dialog, the same in both stages.
const BODY: f32 = height::ROW * 11.0;
/// Width of the hex field.
const HEX_WIDTH: f32 = 80.0;
/// Width of the number fields.
const FIELD_WIDTH: f32 = 88.0;

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

    /// Draws the dialog. Enter renders; Esc leaves, and stops the render that runs.
    pub fn show(&mut self, ctx: &Context, sink: &mut Sink) -> RenderEvent {
        let tokens = Tokens::of(ctx);
        let setup = self.stage == Stage::Setup;
        let all = [
            ModalButton::new("Cancel"),
            ModalButton::primary("Render").enabled(self.location.is_complete()),
        ];
        let buttons = if setup { &all[..] } else { &all[..1] };
        let outcome = Modal::new("render_dialog", "Render a picture")
            .width(520.0)
            .buttons(buttons)
            .show(ctx, |ui| {
                fixed_body(ui, "render_body", BODY, |ui| {
                    if setup {
                        self.setup(ui, &tokens, sink);
                    } else {
                        working(ui, format!("Rendering {}...", self.path().display()));
                    }
                });
            });
        if setup && (outcome.button == Some(1) || outcome.confirmed) {
            self.start(sink);
            return RenderEvent::None;
        }
        if outcome.button == Some(0) || outcome.cancelled {
            if self.stage == Stage::Rendering {
                sink.push(UiRequest::Render(RenderRequest::Cancel));
            }
            return RenderEvent::Close;
        }
        RenderEvent::None
    }

    /// Swatches and a hex field; a colour picker is a popup, which would need a second layer.
    fn color_choice(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = space::XS;
        for (name, rgb) in SWATCHES {
            let chip = color_swatch(
                ui,
                rgb_color(rgb),
                Vec2::splat(height::CONTROL),
                self.color == rgb,
            );
            if chip.on_hover_text(name).clicked() {
                self.color = rgb;
                self.hex = hex_of(rgb);
            }
        }
        ui.add_space(space::SM);
        let size = vec2(HEX_WIDTH, height::CONTROL);
        let edit = ui
            .allocate_ui(size, |ui| {
                text_field(ui, "render_hex", &mut self.hex, "#rrggbb")
            })
            .inner;
        if edit.changed()
            && let Some(rgb) = parse_hex(&self.hex)
        {
            self.color = rgb;
        }
    }

    /// The width and the height of a custom size.
    fn custom_size(&mut self, ui: &mut Ui) {
        for (index, name) in ["Width", "Height"].into_iter().enumerate() {
            if index == 1 {
                ui.label(text(Type::Label, "x"));
            }
            let mut edge = f32::from(u16::try_from(self.settings.size[index]).unwrap_or(u16::MAX));
            let opts = DragFieldOpts {
                name,
                speed: 1.0,
                decimals: 0,
                range: edge_range(),
                snap: Some(1.0),
                unit: "px",
                width: Some(FIELD_WIDTH),
                ..DragFieldOpts::default()
            };
            if drag_field(ui, ("render_edge", index), &mut edge, &opts).changed {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let value = edge.round().max(0.0) as u32;
                self.settings.size[index] = value;
            }
        }
    }

    fn setup(&mut self, ui: &mut Ui, tokens: &Tokens, sink: &mut Sink) {
        PropertyGrid::new("render_choices").show(ui, |grid| {
            grid.row("Size", |ui| {
                let mut labels: Vec<String> =
                    PRESETS.iter().map(|edge| format!("{edge}")).collect();
                labels.push("Custom".to_owned());
                let names: Vec<&str> = labels.iter().map(String::as_str).collect();
                if segmented(ui, &mut self.size_choice, &names)
                    && let Some(&edge) = PRESETS.get(self.size_choice)
                {
                    self.settings.size = [edge, edge];
                }
            });
            if self.size_choice == PRESETS.len() {
                grid.row("Custom size", |ui| self.custom_size(ui));
            }
            grid.row("Background", |ui| {
                let mut at = usize::from(!self.transparent);
                if segmented(ui, &mut at, &["Transparent", "Colour"]) {
                    self.transparent = at == 0;
                }
            });
            if !self.transparent {
                grid.row("Colour", |ui| self.color_choice(ui));
            }
            grid.row("Camera", |ui| {
                let labels = RenderCamera::ALL.map(RenderCamera::label);
                let mut at = RenderCamera::ALL
                    .iter()
                    .position(|c| *c == self.settings.camera)
                    .unwrap_or(0);
                if dropdown(ui, "render_camera", &mut at, &labels).changed() {
                    self.settings.camera = RenderCamera::ALL[at.min(RenderCamera::ALL.len() - 1)];
                }
            });
            grid.row("Shading", |ui| {
                let mut at = usize::from(self.settings.shading == RenderShading::ItemGui);
                if segmented(ui, &mut at, &["Current mode", "Minecraft item GUI"]) {
                    self.settings.shading = if at == 0 {
                        RenderShading::Current
                    } else {
                        RenderShading::ItemGui
                    };
                }
            });
            grid.row("Smoothing", |ui| {
                toggle(
                    ui,
                    &mut self.settings.msaa,
                    "MSAA (renders at twice the size)",
                );
            });
        });
        ui.add_space(space::MD);
        self.location.show(ui, "png", sink);
        if let Some(error) = &self.error {
            ui.add_space(space::XS);
            status_line(ui, Icon::Error, tokens.danger, error.clone());
        }
    }
}

/// The range of a custom edge in the field.
fn edge_range() -> std::ops::RangeInclusive<f32> {
    f32::from(u16::try_from(RENDER_EDGE.0).unwrap_or(0))
        ..=f32::from(u16::try_from(RENDER_EDGE.1).unwrap_or(u16::MAX))
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

    /// Draws the toast; true while it stays. The button's request goes to `sink`. It sits under
    /// the top bar, so it never lands on the stack of plain toasts in the bottom corner.
    pub fn show(&mut self, ctx: &Context, sink: &mut Sink) -> bool {
        let now = ctx.input(|i| i.time);
        let born = *self.born.get_or_insert(now);
        if now - born > LIFETIME {
            return false;
        }
        let tokens = Tokens::of(ctx);
        let (tone, icon) = match self.level {
            Level::Info => (tokens.text_weak, Icon::Info),
            Level::Success => (tokens.ok, Icon::CheckCircle),
            Level::Warning => (tokens.warn, Icon::Warning),
            Level::Error => (tokens.danger, Icon::Error),
        };
        let mut keep = true;
        let offset = vec2(-space::LG, height::ACTION * 2.0 + space::XL);
        Area::new(Id::unique("cuboid_action_toast"))
            .anchor(Align2::RIGHT_TOP, offset)
            .order(Order::Tooltip)
            .show(ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(TOAST_WIDTH);
                    status_line(ui, icon, tone, self.text.clone());
                    ui.horizontal(|ui| {
                        if ui.add(Button::new(&self.label)).clicked() {
                            sink.push(self.request.clone());
                            keep = false;
                        }
                        if ui.add(Button::new("Dismiss").ghost()).clicked() {
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
