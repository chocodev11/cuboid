//! The winit event loop: the window, egui-winit input, egui-wgpu presentation and frame pacing.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use egui_wgpu::winit::Painter;
use egui_wgpu::{RenderState, RendererOptions, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::window::{Fullscreen, ResizeDirection, Window, WindowId};

use crate::{
    CursorKind, FrameCx, ShellApp, ShellError, ShellEvent, ShellOptions, WindowCommand,
    WindowState, keys, window,
};

/// egui draws the UI only: no multisampling, and no dithering, which would alter the texels of
/// the view textures (fact F4).
pub(crate) const RENDERER: RendererOptions = RendererOptions {
    msaa_samples: 1,
    depth_stencil_format: None,
    dithering: false,
    predictable_texture_filtering: false,
};

/// An unfocused or minimised window runs at most 10 frames a second.
///
/// spec: app 4.5, ui 1.3
const UNFOCUSED_INTERVAL: Duration = Duration::from_millis(100);

/// What the event loop does when it has handled its events.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Pacing {
    /// Run a frame now.
    Redraw,
    /// Sleep until then, or until the next event.
    Until(Instant),
    /// Sleep until the next event.
    Idle,
}

/// Frames happen only when something changed or a wake-up is due; a focused window is paced by
/// the presentation (vsync), an unfocused or minimised one by [`UNFOCUSED_INTERVAL`].
///
/// spec: arch 10.4, app 4.5
fn pacing(
    changed: bool,
    wake: Option<Instant>,
    last_frame: Instant,
    focused: bool,
    now: Instant,
) -> Pacing {
    if !changed && wake.is_none_or(|at| at > now) {
        return wake.map_or(Pacing::Idle, Pacing::Until);
    }
    let earliest = if focused {
        now
    } else {
        last_frame + UNFOCUSED_INTERVAL
    };
    if earliest <= now {
        Pacing::Redraw
    } else {
        Pacing::Until(earliest)
    }
}

/// When the next frame is due. A minimised window keeps its frames, paced like an unfocused one,
/// because [`ShellApp::frame`] is the only periodic call of the application (backups, job
/// polling).
///
/// spec: app 1.1, app 12.1, app 4.5
struct Schedule {
    /// Something changed since the last frame.
    changed: bool,
    /// The wake-up the last frame asked for.
    wake: Option<Instant>,
    last_frame: Instant,
    focused: bool,
    minimised: bool,
}

impl Schedule {
    fn new(now: Instant) -> Self {
        Self {
            changed: false,
            wake: None,
            last_frame: now,
            focused: true,
            minimised: false,
        }
    }

    /// Takes the window events that change the pacing: focus, and a resize to nothing (minimised).
    fn apply(&mut self, event: &ShellEvent) {
        match *event {
            ShellEvent::Focus(focused) => self.focused = focused,
            ShellEvent::Resized { width, height } => {
                self.minimised = width == 0 || height == 0;
                self.changed = true;
            }
            _ => {}
        }
    }

    fn next(&self, now: Instant) -> Pacing {
        let foreground = self.focused && !self.minimised;
        pacing(self.changed, self.wake, self.last_frame, foreground, now)
    }

    fn frame_done(&mut self, now: Instant, wake: Option<Instant>) {
        self.changed = false;
        self.wake = wake;
        self.last_frame = now;
    }
}

/// Tells the schedule and the application about a window event.
fn deliver(app: &mut impl ShellApp, schedule: &mut Schedule, event: ShellEvent) {
    schedule.apply(&event);
    app.window_event(event);
}

fn cursor_icon(kind: CursorKind) -> egui::CursorIcon {
    match kind {
        CursorKind::Arrow => egui::CursorIcon::Default,
        CursorKind::Text => egui::CursorIcon::Text,
        CursorKind::ResizeHorizontal => egui::CursorIcon::ResizeHorizontal,
        CursorKind::ResizeVertical => egui::CursorIcon::ResizeVertical,
        CursorKind::ResizeNwSe => egui::CursorIcon::ResizeNwSe,
        CursorKind::ResizeNeSw => egui::CursorIcon::ResizeNeSw,
        CursorKind::Hidden => egui::CursorIcon::None,
    }
}

/// The cursor that replaces the one egui chose in the frame: the resize cursor of a window edge,
/// else the one the application asks for. `None` keeps egui's.
fn cursor_override(
    edge: Option<ResizeDirection>,
    requested: CursorKind,
) -> Option<egui::CursorIcon> {
    let kind = edge.map_or(requested, window::resize_cursor);
    (kind != CursorKind::Arrow).then(|| cursor_icon(kind))
}

/// Everything that exists once the window is open.
struct Win {
    window: Arc<Window>,
    egui: egui_winit::State,
    painter: Painter,
    gpu: RenderState,
    ui_scale: f32,
}

impl Win {
    /// The resize edge under the pointer. A window with the system frame, a maximized one and a
    /// full-screen one have none: the system or nothing resizes them.
    ///
    /// spec: title bar 2
    fn edge_at(&self, pointer: Option<PhysicalPosition<f64>>) -> Option<ResizeDirection> {
        let window = &self.window;
        let resizable = !window.is_decorated() && !window.is_maximized();
        let band = if resizable && window.fullscreen().is_none() {
            window::EDGE_BAND * f64::from(self.ui_scale)
        } else {
            0.0
        };
        window::resize_edge(pointer?, window.inner_size(), band)
    }
}

struct Shell<A> {
    app: A,
    opts: ShellOptions,
    display: OwnedDisplayHandle,
    ctx: egui::Context,
    win: Option<Win>,
    /// Why the loop stopped early; returned by [`run`].
    failure: Option<ShellError>,
    schedule: Schedule,
    alt: bool,
    dropped: Vec<PathBuf>,
    /// The pointer in the client area, `None` outside the window.
    pointer: Option<PhysicalPosition<f64>>,
    /// The primary mouse button is down.
    primary: bool,
}

/// Runs the event loop until the application quits. Wake-ups follow [`FrameRequest::next_wake`];
/// frames are capped at the display rate while focused and 10 Hz while unfocused or minimised, and
/// happen only when something changed or a wake-up is due.
///
/// [`FrameRequest::next_wake`]: crate::FrameRequest::next_wake
///
/// spec: arch 6 (cuboid-shell), arch 10.4, arch 14.1
pub fn run(app: impl ShellApp + 'static, opts: ShellOptions) -> Result<(), ShellError> {
    let event_loop = EventLoop::new().map_err(|err| ShellError::Window(err.to_string()))?;
    let mut shell = Shell {
        app,
        opts,
        display: event_loop.owned_display_handle(),
        ctx: egui::Context::default(),
        win: None,
        failure: None,
        schedule: Schedule::new(Instant::now()),
        alt: false,
        dropped: Vec::new(),
        pointer: None,
        primary: false,
    };
    event_loop
        .run_app(&mut shell)
        .map_err(|err| ShellError::Window(err.to_string()))?;
    shell.failure.map_or(Ok(()), Err)
}

impl<A: ShellApp> Shell<A> {
    /// Follows the pointer and the primary button. A press on a window edge starts the system
    /// resize and is kept from egui: true then.
    ///
    /// spec: title bar 2
    fn on_pointer(&mut self, event: &WindowEvent) -> bool {
        let Some(win) = &self.win else { return false };
        match *event {
            WindowEvent::CursorMoved { position, .. } => self.pointer = Some(position),
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.primary = state.is_pressed();
                if let Some(direction) = win.edge_at(self.pointer).filter(|_| self.primary) {
                    if let Err(err) = win.window.drag_resize_window(direction) {
                        log::warn!("cannot resize the window: {err}");
                    }
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Creates the window, the egui input state and the painter with its device.
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<Win, ShellError> {
        let position = self
            .opts
            .position
            .filter(|_| !self.opts.reset_window)
            .and_then(window::restorable_position);
        let mut attributes = Window::default_attributes()
            .with_title(self.opts.title.as_str())
            .with_decorations(self.opts.decorations)
            .with_visible(false);
        // The shadow and the round corners that the system frame would give an undecorated window.
        #[cfg(windows)]
        {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attributes = attributes
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round);
        }
        if let Some(position) = position {
            attributes = attributes.with_position(position);
        }
        let window = event_loop
            .create_window(attributes)
            .map_err(|err| ShellError::Window(err.to_string()))?;

        // The scale depends on the monitor the window landed on, so the size comes after it.
        let scale_factor = window.scale_factor();
        let ui_scale = window::ui_scale(scale_factor, self.opts.ui_scale_override);
        let (size, min_size) = (self.opts.size, self.opts.min_size);
        window.set_min_inner_size(Some(window::min_client_size(min_size, ui_scale)));
        // The OS may adjust the size; the resize event reports what the window got.
        let _ = window.request_inner_size(window::initial_client_size(size, min_size, ui_scale));
        let window = Arc::new(window);

        // One egui point is one canvas pixel: pixels_per_point = zoom * scale factor = S.
        self.ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
            options.zoom_factor = window::zoom_factor(ui_scale, scale_factor);
        });

        let mut setup = WgpuSetupCreateNew::from_display_handle(self.display.clone());
        let base = Arc::clone(&setup.device_descriptor);
        let wanted = self.opts.optional_features;
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |= wanted & adapter.features();
            descriptor
        });
        let config = WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..WgpuConfiguration::default()
        };
        let mut painter =
            pollster::block_on(Painter::new(self.ctx.clone(), config, false, RENDERER));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(Arc::clone(&window))))
            .map_err(|err| ShellError::Gpu(err.to_string()))?;
        let gpu = painter
            .render_state()
            .ok_or_else(|| ShellError::Gpu("the painter has no render state".to_owned()))?;
        if gpu.target_format.is_srgb() {
            log::error!(
                "the surface format {:?} is sRGB; the pipeline expects gamma space (architecture 13.1)",
                gpu.target_format
            );
        }

        let egui = egui_winit::State::new(
            self.ctx.clone(),
            ViewportId::ROOT,
            &*window,
            Some(window::to_f32(scale_factor)),
            None,
            painter.max_texture_side(),
        );
        Ok(Win {
            window,
            egui,
            painter,
            gpu,
            ui_scale,
        })
    }

    /// Builds one frame and presents it. A minimised window has nothing to present: the frame
    /// still runs, its textures stay current and its shapes are dropped.
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Self {
            app,
            ctx,
            win,
            schedule,
            pointer,
            primary,
            ..
        } = self;
        let Some(win) = win else { return };
        let size = win.window.inner_size();
        let minimised = size.width == 0 || size.height == 0;

        let now = Instant::now();
        ctx.begin_pass(win.egui.take_egui_input(&win.window));
        let request = app.frame(&mut FrameCx {
            egui: ctx,
            gpu: &win.gpu,
            ui_scale: win.ui_scale,
            window: WindowState {
                position: window::reported_position(win.window.outer_position().ok(), minimised),
                fullscreen: win.window.fullscreen().is_some(),
                maximized: win.window.is_maximized(),
                decorated: win.window.is_decorated(),
                focused: win.window.has_focus(),
            },
            now,
        });
        // A held button belongs to the press that started it: only a hover shows the edge.
        let edge = win.edge_at(*pointer).filter(|_| !*primary);
        if let Some(icon) = cursor_override(edge, request.cursor) {
            ctx.set_cursor_icon(icon);
        }
        let mut out = ctx.end_pass();
        win.egui
            .handle_platform_output(&win.window, out.platform_output);
        if minimised {
            let mut renderer = win.gpu.renderer.write();
            for (id, deltas) in out.textures_delta.set.drain() {
                for delta in deltas {
                    renderer.update_texture(&win.gpu.device, &win.gpu.queue, id, &delta);
                }
            }
        } else {
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            win.painter.paint_and_update_textures(
                ViewportId::ROOT,
                out.pixels_per_point,
                [0.0, 0.0, 0.0, 1.0],
                &primitives,
                &mut out.textures_delta,
                Vec::new(),
                &win.window,
            );
        }
        // A frame the painter skipped (lost surface) leaves its frees behind.
        let mut renderer = win.gpu.renderer.write();
        for id in out.textures_delta.free.drain() {
            renderer.free_texture(&id);
        }
        drop(renderer);

        if let Some(title) = &request.title {
            win.window.set_title(title);
        }
        run_commands(&win.window, &request.window, *primary);
        let egui_wake = out
            .viewport_output
            .get(&ViewportId::ROOT)
            .and_then(|viewport| now.checked_add(viewport.repaint_delay));
        let wake = [request.next_wake, egui_wake].into_iter().flatten().min();
        schedule.frame_done(now, wake);
        if request.quit {
            event_loop.exit();
        }
    }
}

/// Carries out the window commands of a frame, in order. A move needs the primary button down:
/// without it the system move loop would follow the pointer until the next click.
///
/// spec: title bar 3
fn run_commands(window: &Window, commands: &[WindowCommand], primary: bool) {
    for &command in commands {
        match command {
            WindowCommand::Move if primary => {
                if let Err(err) = window.drag_window() {
                    log::warn!("cannot move the window: {err}");
                }
            }
            WindowCommand::Move => {}
            WindowCommand::ToggleMaximize => window.set_maximized(!window.is_maximized()),
            WindowCommand::Minimize => window.set_minimized(true),
            WindowCommand::SetDecorations(on) => window.set_decorations(on),
        }
    }
}

/// Maps a keyboard event to the engine and handles the full-screen keys. The synthetic events
/// winit sends when focus changes pass through: they keep the held keys true.
fn on_key(app: &mut impl ShellApp, window: &Window, alt: bool, event: &KeyEvent) {
    let Some(engine) = keys::engine_event(
        event.physical_key,
        &event.logical_key,
        event.location,
        event.state,
        event.repeat,
    ) else {
        return;
    };
    if engine.down && keys::toggles_fullscreen(engine.key, alt) {
        let next = window
            .fullscreen()
            .is_none()
            .then_some(Fullscreen::Borderless(None));
        window.set_fullscreen(next);
    }
    app.engine_key(engine);
}

impl<A: ShellApp> ApplicationHandler for Shell<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(win) => {
                win.window.set_visible(true);
                self.schedule.focused = win.window.has_focus();
                self.schedule.changed = true;
                if let Ok(position) = win.window.outer_position() {
                    self.app.window_event(ShellEvent::Moved {
                        x: position.x,
                        y: position.y,
                    });
                }
                self.win = Some(win);
            }
            Err(err) => {
                self.failure = Some(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            return self.redraw(event_loop);
        }
        if self.on_pointer(&event) {
            return;
        }
        let Some(win) = self.win.as_mut() else { return };
        self.schedule.changed |= win.egui.on_window_event(&win.window, &event).repaint;
        match event {
            WindowEvent::CloseRequested => self.app.window_event(ShellEvent::CloseRequested),
            WindowEvent::Moved(position) => self.app.window_event(ShellEvent::Moved {
                x: position.x,
                y: position.y,
            }),
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    win.painter
                        .on_window_resized(ViewportId::ROOT, width, height);
                }
                let resized = ShellEvent::Resized {
                    width: size.width,
                    height: size.height,
                };
                deliver(&mut self.app, &mut self.schedule, resized);
            }
            WindowEvent::Focused(focused) => {
                deliver(
                    &mut self.app,
                    &mut self.schedule,
                    ShellEvent::Focus(focused),
                );
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                // The canvas scale stays as computed at start-up (ui 1.2): keep the physical
                // size and the point size, whatever the new monitor reports.
                if let Err(err) = inner_size_writer.request_inner_size(win.window.inner_size()) {
                    log::warn!("cannot keep the window size on a scale change: {err}");
                }
                self.ctx.options_mut(|options| {
                    options.zoom_factor = window::zoom_factor(win.ui_scale, scale_factor);
                });
                self.app
                    .window_event(ShellEvent::ScaleFactor(window::to_f32(scale_factor)));
            }
            WindowEvent::DroppedFile(path) => self.dropped.push(path),
            WindowEvent::ModifiersChanged(modifiers) => self.alt = modifiers.state().alt_key(),
            WindowEvent::KeyboardInput { event, .. } => {
                on_key(&mut self.app, &win.window, self.alt, &event);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(engine) = keys::mouse_event(button, state) {
                    self.app.engine_key(engine);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.win.as_ref().map(|win| Arc::clone(&win.window)) else {
            return;
        };
        if !self.dropped.is_empty() {
            let files = std::mem::take(&mut self.dropped);
            self.app.window_event(ShellEvent::FilesDropped(files));
        }
        let mut next = self.schedule.next(Instant::now());
        if next == Pacing::Redraw && self.schedule.minimised {
            // A minimised window may get no paint message: the frame runs from here.
            self.redraw(event_loop);
            next = self.schedule.next(Instant::now());
        }
        let flow = match next {
            Pacing::Redraw => {
                window.request_redraw();
                ControlFlow::Wait
            }
            Pacing::Until(at) => ControlFlow::WaitUntil(at),
            Pacing::Idle => ControlFlow::Wait,
        };
        event_loop.set_control_flow(flow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMISED: ShellEvent = ShellEvent::Resized {
        width: 0,
        height: 0,
    };

    #[test]
    fn a_focused_window_redraws_at_once_when_something_changed() {
        let now = Instant::now();
        assert_eq!(pacing(true, None, now, true, now), Pacing::Redraw);
    }

    #[test]
    fn an_unfocused_window_redraws_at_most_ten_times_a_second() {
        let last = Instant::now();
        let ms = Duration::from_millis;
        assert_eq!(
            pacing(true, None, last, false, last + ms(30)),
            Pacing::Until(last + ms(100))
        );
        assert_eq!(
            pacing(true, None, last, false, last + ms(100)),
            Pacing::Redraw
        );
    }

    #[test]
    fn nothing_changed_sleeps_until_the_wake_up_or_the_next_event() {
        let now = Instant::now();
        let later = now + Duration::from_millis(5);
        assert_eq!(pacing(false, None, now, true, now), Pacing::Idle);
        assert_eq!(
            pacing(false, Some(later), now, true, now),
            Pacing::Until(later)
        );
    }

    #[test]
    fn a_due_wake_up_redraws_even_when_nothing_changed() {
        let now = Instant::now();
        assert_eq!(pacing(false, Some(now), now, true, now), Pacing::Redraw);
    }

    /// The instants at which `schedule` runs a frame between `start` and `end`, driven like
    /// `about_to_wait` does; each frame asks for the next wake-up `wake_after` later.
    fn frame_times(
        schedule: &mut Schedule,
        start: Instant,
        end: Instant,
        wake_after: Duration,
    ) -> Vec<Instant> {
        let mut frames = Vec::new();
        let mut now = start;
        while now <= end {
            match schedule.next(now) {
                Pacing::Redraw => {
                    frames.push(now);
                    schedule.frame_done(now, Some(now + wake_after));
                }
                Pacing::Until(at) => now = at,
                Pacing::Idle => break,
            }
        }
        frames
    }

    #[test]
    fn a_minimised_window_still_runs_a_frame_at_each_wake_up() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        schedule.frame_done(start, None);
        schedule.apply(&MINIMISED);
        assert!(schedule.minimised);

        // The resize to nothing runs one frame (10 Hz at most), then the app's wake-ups follow.
        let minute = Duration::from_secs(60);
        let frames = frame_times(&mut schedule, start, start + 5 * minute, minute);
        assert_eq!(frames.len(), 5, "{frames:?}");
        assert_eq!(frames[0], start + UNFOCUSED_INTERVAL);
        for pair in frames.windows(2) {
            assert_eq!(pair[1] - pair[0], minute);
        }
    }

    #[test]
    fn a_minimised_window_without_a_wake_up_sleeps_and_restoring_it_redraws() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        schedule.apply(&MINIMISED);
        schedule.frame_done(start, None);
        assert_eq!(schedule.next(start + Duration::from_secs(1)), Pacing::Idle);

        schedule.apply(&ShellEvent::Resized {
            width: 1000,
            height: 700,
        });
        assert!(!schedule.minimised);
        let later = start + Duration::from_secs(2);
        assert_eq!(schedule.next(later), Pacing::Redraw);
    }

    #[test]
    fn a_minimised_focused_window_is_paced_like_an_unfocused_one() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        schedule.apply(&ShellEvent::Focus(true));
        schedule.apply(&MINIMISED);
        let soon = start + Duration::from_millis(30);
        assert_eq!(
            schedule.next(soon),
            Pacing::Until(start + UNFOCUSED_INTERVAL)
        );
    }

    #[test]
    fn the_cursor_of_the_widgets_stays_unless_an_edge_or_the_application_overrides_it() {
        use egui::CursorIcon;

        assert_eq!(cursor_override(None, CursorKind::Arrow), None);
        assert_eq!(
            cursor_override(None, CursorKind::Text),
            Some(CursorIcon::Text)
        );
        assert_eq!(
            cursor_override(None, CursorKind::Hidden),
            Some(CursorIcon::None)
        );
        // The edge wins over a request, and over the widgets.
        assert_eq!(
            cursor_override(Some(ResizeDirection::West), CursorKind::Arrow),
            Some(CursorIcon::ResizeHorizontal)
        );
        assert_eq!(
            cursor_override(Some(ResizeDirection::NorthEast), CursorKind::Text),
            Some(CursorIcon::ResizeNeSw)
        );
    }
}
