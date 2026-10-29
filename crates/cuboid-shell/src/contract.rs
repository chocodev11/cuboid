//! Public types and the application trait of this crate (W0.2 contract, completed by W0.4).

use std::path::PathBuf;
use std::time::Instant;

use cuboid_actions::EngineKey;
use thiserror::Error;

/// A key or mouse-button event in the vocabulary of the original, delivered in arrival order with
/// OS key repeat ignored. The shell maps the physical key and its location to the engine key:
/// keypad digits differ from top-row digits, modifiers have sides, and letters match by logical
/// key so that Ctrl+Z works wherever Z is printed.
///
/// spec: arch 10.3, arch 14.1, arch 1.2 (fact F1)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EngineKeyEvent {
    /// The engine key or mouse button.
    pub key: EngineKey,
    /// `true` for a press, `false` for a release.
    pub down: bool,
}

/// Window events the application reacts to. Pointer, wheel and text input reach the UI through
/// egui; key events through [`ShellApp::engine_key`].
///
/// spec: arch 6 (cuboid-shell), arch 14.1
#[derive(Clone, Debug, PartialEq)]
pub enum ShellEvent {
    /// The window gained or lost focus (texture reload check, frame-rate policy).
    Focus(bool),
    /// The client area changed, in physical pixels.
    Resized {
        /// New width.
        width: u32,
        /// New height.
        height: u32,
    },
    /// The display scale factor changed.
    ScaleFactor(f32),
    /// The window moved, or opened. The raw outer position in physical pixels, in the
    /// coordinates [`ShellOptions::position`] restores; a minimised Windows window reports
    /// -32000. [`WindowState::position`] filters that case.
    Moved {
        /// Outer left edge.
        x: i32,
        /// Outer top edge.
        y: i32,
    },
    /// Files were dropped on the window.
    FilesDropped(Vec<PathBuf>),
    /// The window's close button was pressed; the application decides whether to quit, by
    /// setting [`FrameRequest::quit`].
    CloseRequested,
}

/// The window state the application reads at the moment it needs it (the position to save before
/// a settings write, the buttons of the title bar).
///
/// spec: app 4.2, app 10.1, title bar 3
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowState {
    /// The outer position in physical pixels, in the coordinates [`ShellOptions::position`]
    /// restores. `None` while the window is minimised and where the platform reports none
    /// (Wayland): the application then keeps the position it stored last.
    pub position: Option<(i32, i32)>,
    /// The window is in borderless full screen. A full-screen client size is the monitor size and
    /// is not the size to store.
    pub fullscreen: bool,
    /// The window is maximized.
    pub maximized: bool,
    /// The system draws the title bar and the border.
    pub decorated: bool,
    /// The window has the keyboard focus.
    pub focused: bool,
}

impl Default for WindowState {
    /// A focused window with the system title bar, as the shell opens it by default.
    fn default() -> Self {
        Self {
            position: None,
            fullscreen: false,
            maximized: false,
            decorated: true,
            focused: true,
        }
    }
}

/// What the shell hands to the application for one frame.
///
/// The shell opens the egui pass before [`ShellApp::frame`] and closes it afterwards, so the
/// application only adds its UI to [`FrameCx::egui`] (`root_ui`, panels, painters).
///
/// spec: arch 6 (cuboid-shell), arch 14.1
pub struct FrameCx<'a> {
    /// The egui context to build the UI in.
    pub egui: &'a egui::Context,
    /// The shared device, queue and egui renderer.
    pub gpu: &'a egui_wgpu::RenderState,
    /// The UI scale `S`; one egui point is one canvas pixel.
    pub ui_scale: f32,
    /// The window position, size state, frame and focus.
    pub window: WindowState,
    /// The frame time.
    pub now: Instant,
}

/// The mouse cursor the application wants.
///
/// spec: arch 14.1, ui 2.3, ui 11
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum CursorKind {
    /// The default arrow.
    #[default]
    Arrow,
    /// The text insertion bar.
    Text,
    /// Horizontal resize (side grips).
    ResizeHorizontal,
    /// Vertical resize.
    ResizeVertical,
    /// Diagonal resize, top-left to bottom-right (dialog grips).
    ResizeNwSe,
    /// Diagonal resize, top-right to bottom-left.
    ResizeNeSw,
    /// No cursor; the tool cursor symbol is drawn instead.
    Hidden,
}

/// A window operation the application asks for, applied after the frame.
///
/// spec: title bar 3
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowCommand {
    /// Start the system move loop. Dropped unless the primary button is still down.
    Move,
    /// Maximize the window, or restore it when it is maximized.
    ToggleMaximize,
    /// Minimize the window.
    Minimize,
    /// Show or hide the system title bar and border.
    SetDecorations(bool),
}

/// The key (an [`egui::Id`] source) of a `u64` in the data of the egui context that counts the
/// renderers egui has had. A new painter starts without textures and egui sends none again by
/// itself: the shell raises the number and sends the font atlas, and whoever keeps egui textures
/// (`TextureHandle`s) uploads them again when the number differs from the one it saw.
/// `cuboid-ui` reads it under the same text, since it cannot depend on this crate.
pub const GPU_EPOCH: &str = "cuboid.gpu_epoch";

/// What the application asks of the shell after a frame.
///
/// spec: arch 6 (cuboid-shell), arch 10.4, title bar 3
#[derive(Clone, Debug, Default)]
pub struct FrameRequest {
    /// The earliest instant a redraw is due (a timer, an animation, a finished job);
    /// `None` sleeps until the next input.
    pub next_wake: Option<Instant>,
    /// A cursor to show instead of the one the widgets chose in the frame. [`CursorKind::Arrow`]
    /// asks for nothing: egui's cursor stays (text bar, resize, grab). A window edge always shows
    /// its resize cursor.
    pub cursor: CursorKind,
    /// A new window title, if it changed.
    pub title: Option<String>,
    /// Window operations, in order.
    pub window: Vec<WindowCommand>,
    /// Leave the event loop after this frame (the answer to [`ShellEvent::CloseRequested`]).
    pub quit: bool,
    /// The GPU device of the window is lost or cannot be trusted: after this frame the shell
    /// drops the painter and builds a new one with a new device. The application then sees
    /// another [`FrameCx::gpu`] and recreates what it holds on the device. egui's own textures
    /// (fonts, [`crate::GPU_EPOCH`]) are sent again by the shell.
    pub recreate_gpu: bool,
}

/// A milestone of the shell's start-up and first frames, reported to [`ShellApp::phase`] for the
/// application's start-up trace.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShellPhase {
    /// The event loop exists.
    EventLoop,
    /// The window exists, still hidden.
    Window,
    /// The graphics instance exists.
    Instance,
    /// The adapter is chosen; the device request follows.
    Adapter,
    /// The device, the egui renderer and the configured surface are ready. egui-wgpu does these in
    /// one call, so the shell cannot tell them apart.
    Gpu,
    /// The window was made visible.
    Visible,
    /// The application built a frame; its painting follows.
    Built,
    /// A frame was drawn to the window. Reported after every drawn frame.
    Presented,
}

/// The application as the shell sees it. The shell owns the event loop and the painter; it
/// contains no application logic.
///
/// [`ShellApp::frame`] is the application's periodic call and keeps running while the window is
/// minimised or unfocused (at most 10 times a second, and only when something changed or the
/// requested wake-up is due), so that backups and job polling go on. A minimised frame has an
/// empty screen and its shapes are dropped.
///
/// spec: arch 6 (cuboid-shell), arch 14.1
pub trait ShellApp {
    /// A key or mouse button changed state, in arrival order.
    fn engine_key(&mut self, ev: EngineKeyEvent);

    /// A window event: focus, resize, drop, close request, scale.
    fn window_event(&mut self, ev: ShellEvent);

    /// Builds the UI, renders the views and returns the wake-up request.
    fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest;

    /// The painter could not draw the frame: it panicked, which is what egui-wgpu does when the
    /// GPU device is gone, or the device could not be set up again. The frame is dropped. The
    /// shell builds a new painter and device before the next frame and asks for that frame after
    /// a short pause, so this is the application's chance to keep its work, and to answer with
    /// [`FrameRequest::quit`] when the failure repeats instead of looping. Nothing can be shown
    /// to the user before the next frame has been drawn.
    fn paint_failed(&mut self, _message: &str) {}

    /// A start-up milestone of the shell. It has no receiver because the graphics set-up runs where
    /// the application value is not at hand. The default ignores it.
    fn phase(_phase: ShellPhase)
    where
        Self: Sized,
    {
    }
}

/// Window options. The defaults are the original's first launch.
///
/// spec: app 1.1, app 4.2, ui 1.3, title bar 3
#[derive(Clone, Debug, PartialEq)]
pub struct ShellOptions {
    /// Window title before the first frame request.
    pub title: String,
    /// The system draws the title bar and the border. Off when the application draws its own.
    pub decorations: bool,
    /// Client size in physical pixels at the first launch.
    pub size: (u32, u32),
    /// Minimum client size at scale 1; the shell multiplies it by the UI scale.
    pub min_size: (u32, u32),
    /// Restored window position, subject to the 0 and 30 000 guards.
    pub position: Option<(i32, i32)>,
    /// A fixed UI scale instead of the one derived from the display dpi.
    pub ui_scale_override: Option<f32>,
    /// Ignore the stored window position (`--reset-window`, standing in for the Left Alt bypass).
    pub reset_window: bool,
    /// Device features the renderer can use. Each one is requested only when the adapter has it.
    pub optional_features: wgpu::Features,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            title: "cuboid".to_owned(),
            decorations: true,
            size: (1000, 700),
            min_size: (900, 700),
            position: None,
            ui_scale_override: None,
            reset_window: false,
            optional_features: wgpu::Features::empty(),
        }
    }
}

/// Why the shell could not run.
///
/// spec: arch 16, arch 13.9
#[derive(Debug, Error)]
pub enum ShellError {
    /// The event loop or the window could not be created.
    #[error("cannot create the window: {0}")]
    Window(String),
    /// No usable graphics adapter or device, or a failed read-back.
    #[error("no usable graphics adapter: {0}")]
    Gpu(String),
    /// A headless size or scale the device cannot draw.
    #[error("invalid headless parameters: {0}")]
    Invalid(String),
}

/// A picture of rows from top to bottom, as a frame is presented.
///
/// spec: arch 14.1 (Headless)
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RgbaImage {
    /// Width and height in pixels.
    pub size: (u32, u32),
    /// `width * height * 4` bytes, RGBA.
    pub pixels: Vec<u8>,
}

/// One scripted input of a headless frame.
///
/// spec: arch 14.1 (Headless), arch 19.3 (Input replay)
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptedInput {
    /// A key or mouse-button event.
    Key(EngineKeyEvent),
    /// The pointer moved to a position in canvas pixels.
    PointerMoved([f32; 2]),
    /// The wheel moved, in notches.
    Wheel(f32),
    /// Text was typed.
    Text(String),
    /// A window event. A headless target keeps its size: a scripted [`ShellEvent::Resized`]
    /// reaches the application only. [`ShellEvent::Moved`] sets [`WindowState::position`].
    Window(ShellEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The application is handed to the headless shell as a trait object.
    fn _shell_app_is_object_safe(_: &dyn ShellApp) {}

    #[test]
    fn a_window_opens_focused_with_the_system_title_bar() {
        let state = WindowState::default();
        assert!(state.decorated && state.focused);
        assert!(!state.maximized && !state.fullscreen && state.position.is_none());
    }

    #[test]
    fn first_launch_options_follow_app_1_1() {
        let opts = ShellOptions::default();
        assert_eq!(opts.size, (1000, 700));
        assert_eq!(opts.min_size, (900, 700));
        assert!(opts.position.is_none() && opts.ui_scale_override.is_none());
        assert!(opts.decorations);
    }
}
