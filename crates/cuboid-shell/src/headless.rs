//! Headless mode: full app frames into an offscreen texture, read back to the CPU.
//!
//! A frame takes the same path as in the window: scripted input reaches the application in
//! arrival order, [`ShellApp::frame`] builds the UI in the egui pass, and the egui renderer
//! draws the result. The adapter is a software one (WARP, lavapipe) where the machine has one, so
//! that the pixels do not depend on the GPU.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cuboid_actions::EngineKey;
use egui::{
    ClippedPrimitive, Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2,
    TexturesDelta, TouchPhase,
};
use egui_wgpu::{RenderState, ScreenDescriptor, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

use crate::{
    EngineKeyEvent, FrameCx, RgbaImage, ScriptedInput, ShellApp, ShellError, ShellEvent,
    WindowCommand, WindowState, event_loop::RENDERER, keys,
};

/// Time between two headless frames: egui animations advance at 60 Hz.
const FRAME_TIME: f64 = 1.0 / 60.0;

/// Creating an instance and a device on two threads at the same moment crashes the process on
/// Windows (an access violation in the DX12 backend of wgpu 30, about every other run), and
/// parallel tests do exactly that. The creation takes turns; the frames do not need to.
static GPU_INIT: Mutex<()> = Mutex::new(());

/// Full app frames rendered into an offscreen texture without a window or surface, with
/// scripted input through the same [`ShellApp::frame`] path. UI snapshot and replay tests use it.
///
/// spec: arch 6 (cuboid-shell), arch 14.1
pub struct Headless {
    gpu: RenderState,
    ctx: egui::Context,
    target: wgpu::Texture,
    size: (u32, u32),
    scale: f32,
    start: Instant,
    frames: u32,
    input: InputState,
    window: WindowState,
    /// The window commands the application asked for and nobody has taken yet.
    commands: Vec<WindowCommand>,
}

/// What egui-winit keeps between window events, for scripted input: the pointer, the held keys
/// and the clipboard. It turns scripted input into the egui events the window path delivers, so
/// that a replay drives the same text fields, shortcuts and selections.
///
/// spec: arch 14.1 (Headless), arch 14.5, arch 19.3
struct InputState {
    pointer: Pos2,
    /// Modifier keys currently held, either side.
    held: Vec<EngineKey>,
    /// The Super key is the command key (macOS), as egui-winit decides from the OS.
    mac: bool,
    /// What a scripted Ctrl+V pastes: the last text the application copied.
    clipboard: String,
}

/// A clipboard shortcut egui-winit turns into its own event instead of a key.
#[derive(Copy, Clone, Eq, PartialEq)]
enum Shortcut {
    Cut,
    Copy,
    Paste,
}

impl Shortcut {
    /// The shortcuts of egui-winit: Ctrl/Cmd+X, C, V, and the Windows alternatives.
    fn of(modifiers: Modifiers, key: Key) -> Option<Self> {
        let windows = cfg!(target_os = "windows");
        if modifiers.command_only() {
            match key {
                Key::X => return Some(Self::Cut),
                Key::C => return Some(Self::Copy),
                Key::V => return Some(Self::Paste),
                _ => {}
            }
        }
        match key {
            Key::Delete if windows && modifiers.shift => Some(Self::Cut),
            Key::Insert if windows && modifiers.ctrl => Some(Self::Copy),
            Key::Insert if windows && modifiers.shift => Some(Self::Paste),
            _ => None,
        }
    }
}

impl InputState {
    fn new(mac: bool) -> Self {
        Self {
            pointer: Pos2::ZERO,
            held: Vec::new(),
            mac,
            clipboard: String::new(),
        }
    }

    /// The modifiers the held keys make up; the command key is Control, or Super on macOS.
    fn modifiers(&self) -> Modifiers {
        let any = |keys: &[EngineKey]| self.held.iter().any(|held| keys.contains(held));
        let ctrl = any(&[EngineKey::LeftControl, EngineKey::RightControl]);
        let super_ = any(&[
            EngineKey::LeftWindows,
            EngineKey::RightWindows,
            EngineKey::LeftCommand,
            EngineKey::RightCommand,
        ]);
        Modifiers {
            alt: any(&[EngineKey::LeftAlt, EngineKey::RightAlt]),
            ctrl,
            shift: any(&[EngineKey::LeftShift, EngineKey::RightShift]),
            mac_cmd: self.mac && super_,
            command: if self.mac { super_ } else { ctrl },
        }
    }

    /// Appends the egui events of one scripted input.
    fn push(&mut self, item: &ScriptedInput, events: &mut Vec<Event>) {
        match item {
            ScriptedInput::Key(ev) => self.key(*ev, events),
            ScriptedInput::PointerMoved([x, y]) => {
                self.pointer = Pos2::new(*x, *y);
                events.push(Event::PointerMoved(self.pointer));
            }
            ScriptedInput::Wheel(notches) => events.push(Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: egui::vec2(0.0, *notches),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Wheel,
                modifiers: self.modifiers(),
            }),
            ScriptedInput::Text(text) => events.push(Event::Text(text.clone())),
            ScriptedInput::Window(ShellEvent::Focus(focused)) => {
                events.push(Event::WindowFocused(*focused));
            }
            ScriptedInput::Window(_) => {}
        }
    }

    /// A mouse button is a pointer event. A key updates the held modifiers first, as winit sends
    /// the modifier change before the key, and then becomes an `Event::Key` with egui's folding of
    /// the keypad, or a clipboard event for the shortcuts.
    fn key(&mut self, ev: EngineKeyEvent, events: &mut Vec<Event>) {
        if let Some(button) = pointer_button(ev.key) {
            events.push(Event::PointerButton {
                pos: self.pointer,
                button,
                pressed: ev.down,
                modifiers: self.modifiers(),
            });
            return;
        }
        let before = self.modifiers();
        self.held.retain(|held| *held != ev.key);
        if ev.down {
            self.held.push(ev.key);
        }
        let modifiers = self.modifiers();
        if modifiers != before {
            events.push(Event::ModifiersChanged(modifiers));
        }
        let Some(key) = keys::egui_key(ev.key) else {
            return;
        };
        match ev.down.then(|| Shortcut::of(modifiers, key)).flatten() {
            Some(Shortcut::Cut) => events.push(Event::Cut),
            Some(Shortcut::Copy) => events.push(Event::Copy),
            Some(Shortcut::Paste) if !self.clipboard.is_empty() => {
                events.push(Event::Paste(self.clipboard.clone()));
            }
            Some(Shortcut::Paste) => {}
            None => events.push(Event::Key {
                key,
                physical_key: Some(key),
                pressed: ev.down,
                repeat: false,
                modifiers,
            }),
        }
    }
}

impl fmt::Debug for Headless {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Headless")
            .field("size", &self.size)
            .field("scale", &self.scale)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

/// The first software adapter, else the first adapter.
fn software_first(
    adapters: &[wgpu::Adapter],
    _surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    adapters
        .iter()
        .find(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
        .or_else(|| adapters.first())
        .cloned()
        .ok_or_else(|| "no adapter found".to_owned())
}

impl Headless {
    /// A headless device and an offscreen target of `size` physical pixels at the given UI scale
    /// (egui points are `scale` pixels large). A size of 0, a side above the device limit
    /// (8192 on the default device) or a scale that is not a positive number is an error.
    pub fn new(size: (u32, u32), scale: f32) -> Result<Self, ShellError> {
        if size.0 == 0 || size.1 == 0 || !(scale.is_finite() && scale > 0.0) {
            return Err(ShellError::Invalid(format!(
                "size {}x{} at scale {scale}",
                size.0, size.1
            )));
        }
        let setup = WgpuSetupCreateNew {
            native_adapter_selector: Some(Arc::new(software_first)),
            ..WgpuSetupCreateNew::without_display_handle()
        };
        let config = WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..WgpuConfiguration::default()
        };
        let gpu = {
            let _turn = GPU_INIT.lock().unwrap_or_else(PoisonError::into_inner);
            let instance = pollster::block_on(config.wgpu_setup.new_instance());
            pollster::block_on(RenderState::create(&config, &instance, None, RENDERER))
                .map_err(|err| ShellError::Gpu(err.to_string()))?
        };
        let max_side = gpu.device.limits().max_texture_dimension_2d;
        if size.0 > max_side || size.1 > max_side {
            return Err(ShellError::Invalid(format!(
                "size {}x{} is above the device limit {max_side}",
                size.0, size.1
            )));
        }

        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless_target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let ctx = egui::Context::default();
        let input = InputState::new(ctx.os().is_mac());
        Ok(Self {
            gpu,
            ctx,
            target,
            size,
            scale,
            start: Instant::now(),
            frames: 0,
            input,
            window: WindowState::default(),
            commands: Vec::new(),
        })
    }

    /// The window commands of the frames since the last call, in order. A headless window has no
    /// frame to move, resize or minimise: the commands are only recorded.
    ///
    /// spec: title bar 3
    pub fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.commands)
    }

    /// Feeds the inputs to the application, runs one frame and reads the result back. The only
    /// error is a failed read-back (a lost device).
    pub fn frame(
        &mut self,
        app: &mut dyn ShellApp,
        input: &[ScriptedInput],
    ) -> Result<RgbaImage, ShellError> {
        let time = f64::from(self.frames) * FRAME_TIME;
        self.frames += 1;

        let max_side = self.gpu.device.limits().max_texture_dimension_2d;
        let mut raw = egui::RawInput {
            time: Some(time),
            max_texture_side: Some(max_side as usize),
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(self.size.0 as f32, self.size.1 as f32) / self.scale,
            )),
            ..egui::RawInput::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.scale);
        for item in input {
            self.feed(app, &mut raw, item);
        }

        self.ctx.begin_pass(raw);
        let request = app.frame(&mut FrameCx {
            egui: &self.ctx,
            gpu: &self.gpu,
            ui_scale: self.scale,
            window: self.window,
            now: self.start + Duration::from_secs_f64(time),
        });
        self.commands.extend(request.window);
        let mut out = self.ctx.end_pass();
        for command in &out.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.input.clipboard.clone_from(text);
            }
        }
        let primitives = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        self.render_and_read(&primitives, &mut out.textures_delta, out.pixels_per_point)
    }

    /// Hands one scripted input to the application and to egui.
    fn feed(&mut self, app: &mut dyn ShellApp, raw: &mut egui::RawInput, item: &ScriptedInput) {
        match item {
            ScriptedInput::Key(ev) => app.engine_key(*ev),
            ScriptedInput::Window(ev) => {
                app.window_event(ev.clone());
                match *ev {
                    ShellEvent::Focus(focused) => raw.focused = focused,
                    ShellEvent::Moved { x, y } => self.window.position = Some((x, y)),
                    _ => {}
                }
            }
            _ => {}
        }
        self.input.push(item, &mut raw.events);
    }

    /// Draws the egui primitives into the target and reads it back.
    fn render_and_read(
        &self,
        primitives: &[ClippedPrimitive],
        textures: &mut TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<RgbaImage, ShellError> {
        let (width, height) = self.size;
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("headless_frame"),
        });

        let user_buffers = {
            let mut renderer = self.gpu.renderer.write();
            for (id, deltas) in textures.set.drain() {
                for delta in deltas {
                    renderer.update_texture(device, queue, id, &delta);
                }
            }
            renderer.update_buffers(device, queue, &mut encoder, primitives, &screen)
        };
        {
            let view = self
                .target
                .create_view(&wgpu::TextureViewDescriptor::default());
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("headless_egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.gpu
                .renderer
                .read()
                .render(&mut pass.forget_lifetime(), primitives, &screen);
        }

        let unpadded = width * 4;
        let padded = wgpu::util::align_to(unpadded, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless_readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            self.target.size(),
        );
        queue.submit(user_buffers.into_iter().chain([encoder.finish()]));
        {
            let mut renderer = self.gpu.renderer.write();
            for id in textures.free.drain() {
                renderer.free_texture(&id);
            }
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                // The receiver outlives the poll below; a send can only fail after it is gone.
                sender.send(result).ok();
            });
        let gpu_error =
            |err: &dyn fmt::Display| ShellError::Gpu(format!("read-back failed: {err}"));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| gpu_error(&err))?;
        receiver
            .recv()
            .map_err(|err| gpu_error(&err))?
            .map_err(|err| gpu_error(&err))?;

        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|err| gpu_error(&err))?;
        let mut pixels = Vec::with_capacity(unpadded as usize * height as usize);
        for row in mapped.chunks_exact(padded as usize) {
            pixels.extend_from_slice(&row[..unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(RgbaImage {
            size: self.size,
            pixels,
        })
    }
}

/// The egui pointer button of a mouse engine key.
fn pointer_button(key: EngineKey) -> Option<PointerButton> {
    match key {
        EngineKey::Mouse0 => Some(PointerButton::Primary),
        EngineKey::Mouse1 => Some(PointerButton::Secondary),
        EngineKey::Mouse2 => Some(PointerButton::Middle),
        EngineKey::Mouse3 => Some(PointerButton::Extra1),
        EngineKey::Mouse4 => Some(PointerButton::Extra2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use cuboid_testkit::diff;
    use egui::Id;

    use super::*;
    use crate::pattern::{TestPattern, pattern_pixel};
    use crate::{EngineKeyEvent, FrameRequest, GPU_EPOCH};

    /// Draws the test pattern `offset` points from the top-left corner and records its input.
    struct PatternApp {
        pattern_size: (u32, u32),
        offset: Pos2,
        pattern: Option<TestPattern>,
        keys: Vec<EngineKeyEvent>,
        windows: Vec<ShellEvent>,
        /// The egui input and the window state of the last frame.
        egui_events: Vec<Event>,
        egui_modifiers: Modifiers,
        window: WindowState,
        /// The window commands every frame asks for.
        commands: Vec<WindowCommand>,
    }

    impl PatternApp {
        fn new(pattern_size: (u32, u32), offset: Pos2) -> Self {
            Self {
                pattern_size,
                offset,
                pattern: None,
                keys: Vec::new(),
                windows: Vec::new(),
                egui_events: Vec::new(),
                egui_modifiers: Modifiers::NONE,
                window: WindowState::default(),
                commands: Vec::new(),
            }
        }
    }

    impl ShellApp for PatternApp {
        fn engine_key(&mut self, ev: EngineKeyEvent) {
            self.keys.push(ev);
        }

        fn window_event(&mut self, ev: ShellEvent) {
            self.windows.push(ev);
        }

        fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest {
            self.window = cx.window;
            cx.egui.input(|input| {
                self.egui_events.clone_from(&input.events);
                self.egui_modifiers = input.modifiers;
            });
            let size = self.pattern_size;
            let pattern = self
                .pattern
                .get_or_insert_with(|| TestPattern::new(cx.gpu, size));
            cx.egui
                .root_ui(|ui| pattern.paint(ui.painter(), self.offset, cx.ui_scale));
            FrameRequest {
                window: self.commands.clone(),
                ..FrameRequest::default()
            }
        }
    }

    /// The frame the app should produce: opaque black with the pattern at `offset` pixels.
    fn expected_frame(frame: (u32, u32), pattern: (u32, u32), offset: (u32, u32)) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..frame.1 {
            for x in 0..frame.0 {
                let inside = x >= offset.0
                    && y >= offset.1
                    && x - offset.0 < pattern.0
                    && y - offset.1 < pattern.1;
                pixels.extend_from_slice(&if inside {
                    pattern_pixel(x - offset.0, y - offset.1)
                } else {
                    [0, 0, 0, 255]
                });
            }
        }
        pixels
    }

    /// A headless shell, printing the adapter it got.
    ///
    /// Without an adapter the test fails when the `CI` variable is set, where a green run must
    /// mean that a frame was rendered; elsewhere it is skipped with a notice (`None`). nextest
    /// hides the output of passing tests: run it with `--success-output final` to see the adapter.
    /// On CI the adapter must be a software one (WARP, lavapipe); the macOS runners have none and
    /// render on their GPU.
    fn headless_or_skip(size: (u32, u32), scale: f32) -> Option<Headless> {
        let ci = std::env::var_os("CI").is_some();
        match Headless::new(size, scale) {
            Ok(headless) => {
                let info = headless.gpu.adapter.get_info();
                let _ = writeln!(
                    std::io::stderr(),
                    "headless adapter: {} ({:?}, {:?})",
                    info.name,
                    info.device_type,
                    info.backend
                );
                if ci && cfg!(not(target_os = "macos")) {
                    assert_eq!(
                        info.device_type,
                        wgpu::DeviceType::Cpu,
                        "CI renders on a software adapter, got {}",
                        info.name
                    );
                }
                Some(headless)
            }
            Err(err) => {
                assert!(!ci, "no wgpu adapter under CI: {err}");
                // Like the oracle gating of the test kit: shown even for passing tests.
                let _ = writeln!(std::io::stderr(), "SKIPPED (no wgpu adapter): {err}");
                None
            }
        }
    }

    /// The pattern composited through egui reads back bit for bit, at scale 1 and at scale 1.5
    /// (where a point is 1.5 pixels and the image still maps texel to pixel).
    #[test]
    fn test_pattern_composites_through_egui_bit_exact() {
        // (scale, frame in pixels, pattern in pixels, offset in points, offset in pixels)
        let cases = [
            (1.0, (80, 60), (64, 48), (8.0, 4.0), (8, 4)),
            (1.5, (120, 90), (60, 45), (8.0, 4.0), (12, 6)),
        ];
        for (scale, frame, pattern, offset, pixel_offset) in cases {
            let Some(mut headless) = headless_or_skip(frame, scale) else {
                return;
            };
            let mut app = PatternApp::new(pattern, Pos2::new(offset.0, offset.1));
            let image = headless.frame(&mut app, &[]).expect("read-back");

            assert_eq!(image.size, frame);
            let expected = expected_frame(frame, pattern, pixel_offset);
            diff::bytes(&expected, &image.pixels)
                .unwrap_or_else(|mismatch| panic!("scale {scale}: {mismatch}"));

            // A second frame reuses the registered texture and still reads back the same.
            let again = headless.frame(&mut app, &[]).expect("read-back");
            diff::bytes(&expected, &again.pixels)
                .unwrap_or_else(|mismatch| panic!("scale {scale}, second frame: {mismatch}"));
        }
    }

    /// Control held on a platform where Control is the command key.
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };

    fn key(key: EngineKey, down: bool) -> ScriptedInput {
        ScriptedInput::Key(EngineKeyEvent { key, down })
    }

    /// What egui-winit pushes for a key event while `modifiers` are held.
    fn winit_key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        }
    }

    /// Scripted input reaches the application in arrival order before the frame is built, and
    /// reaches egui as the events the window path delivers.
    #[test]
    fn scripted_input_reaches_the_app_and_egui_in_arrival_order() {
        let Some(mut headless) = headless_or_skip((16, 16), 1.0) else {
            return;
        };
        let mut app = PatternApp::new((8, 8), Pos2::ZERO);
        let input = [
            ScriptedInput::Window(ShellEvent::Focus(true)),
            ScriptedInput::Window(ShellEvent::Moved { x: 40, y: -8 }),
            key(EngineKey::LeftControl, true),
            key(EngineKey::Keypad1, true),
            ScriptedInput::PointerMoved([3.0, 4.0]),
            key(EngineKey::Mouse0, true),
            key(EngineKey::Keypad1, false),
            ScriptedInput::Wheel(1.0),
            ScriptedInput::Text("a".to_owned()),
        ];
        headless.frame(&mut app, &input).expect("read-back");

        let events: Vec<_> = app.keys.iter().map(|ev| (ev.key, ev.down)).collect();
        let expected = [
            (EngineKey::LeftControl, true),
            (EngineKey::Keypad1, true),
            (EngineKey::Mouse0, true),
            (EngineKey::Keypad1, false),
        ];
        diff::sequence(&expected, &events).unwrap();
        let moved = ShellEvent::Moved { x: 40, y: -8 };
        assert_eq!(app.windows, [ShellEvent::Focus(true), moved]);
        assert_eq!(app.window.position, Some((40, -8)));

        let ctrl = CTRL;
        let expected_events = [
            Event::WindowFocused(true),
            Event::ModifiersChanged(ctrl),
            winit_key(Key::ControlLeft, true, ctrl),
            winit_key(Key::Num1, true, ctrl),
            Event::PointerMoved(Pos2::new(3.0, 4.0)),
            Event::PointerButton {
                pos: Pos2::new(3.0, 4.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: ctrl,
            },
            winit_key(Key::Num1, false, ctrl),
            Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 1.0),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Wheel,
                modifiers: ctrl,
            },
            Event::Text("a".to_owned()),
        ];
        assert_eq!(app.egui_events, expected_events);
        assert_eq!(app.egui_modifiers, ctrl);
    }

    /// The commands of the frames are recorded in order, and each call takes what came since the
    /// one before. The window itself stays as it is.
    #[test]
    fn window_commands_are_recorded_in_order_and_taken_once() {
        let Some(mut headless) = headless_or_skip((16, 16), 1.0) else {
            return;
        };
        let mut app = PatternApp::new((8, 8), Pos2::ZERO);
        app.commands = vec![WindowCommand::Move, WindowCommand::SetDecorations(false)];
        headless.frame(&mut app, &[]).expect("read-back");
        app.commands = vec![WindowCommand::ToggleMaximize, WindowCommand::Minimize];
        headless.frame(&mut app, &[]).expect("read-back");
        assert_eq!(
            headless.take_window_commands(),
            [
                WindowCommand::Move,
                WindowCommand::SetDecorations(false),
                WindowCommand::ToggleMaximize,
                WindowCommand::Minimize,
            ]
        );
        assert!(headless.take_window_commands().is_empty());
        assert_eq!(app.window, WindowState::default());
    }

    #[test]
    fn a_headless_shell_rejects_a_target_the_device_cannot_draw() {
        for (size, scale) in [
            ((0, 10), 1.0),
            ((10, 0), 1.0),
            ((10, 10), 0.0),
            ((10, 10), f32::NAN),
        ] {
            let result = Headless::new(size, scale);
            assert!(
                matches!(result, Err(ShellError::Invalid(_))),
                "{size:?} at {scale}"
            );
        }
    }

    // The input translation needs no device: egui runs without a renderer.

    /// The egui events of a script.
    fn events_of(input: &mut InputState, script: &[ScriptedInput]) -> Vec<Event> {
        let mut events = Vec::new();
        for item in script {
            input.push(item, &mut events);
        }
        events
    }

    #[test]
    fn keys_reach_egui_as_egui_winit_delivers_them() {
        let mut input = InputState::new(false);
        let shift = Modifiers::SHIFT;
        let events = events_of(
            &mut input,
            &[
                key(EngineKey::RightShift, true),
                key(EngineKey::Backspace, true),
                key(EngineKey::Backspace, false),
                key(EngineKey::RightShift, false),
                key(EngineKey::Keypad7, true),
                key(EngineKey::KeypadEnter, true),
                key(EngineKey::KeypadMultiply, true),
            ],
        );
        let expected = [
            Event::ModifiersChanged(shift),
            winit_key(Key::ShiftRight, true, shift),
            winit_key(Key::Backspace, true, shift),
            winit_key(Key::Backspace, false, shift),
            Event::ModifiersChanged(Modifiers::NONE),
            winit_key(Key::ShiftRight, false, Modifiers::NONE),
            // The keypad folds into the main-block keys, and the keypad multiply key has none.
            winit_key(Key::Num7, true, Modifiers::NONE),
            winit_key(Key::Enter, true, Modifiers::NONE),
        ];
        assert_eq!(events, expected);
        assert_eq!(input.modifiers(), Modifiers::NONE);
    }

    #[test]
    fn a_pointer_button_carries_the_held_modifiers() {
        let mut input = InputState::new(false);
        let events = events_of(
            &mut input,
            &[
                key(EngineKey::LeftShift, true),
                ScriptedInput::PointerMoved([5.0, 6.0]),
                key(EngineKey::Mouse1, true),
            ],
        );
        assert_eq!(
            events.last(),
            Some(&Event::PointerButton {
                pos: Pos2::new(5.0, 6.0),
                button: PointerButton::Secondary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            })
        );
    }

    #[test]
    fn clipboard_shortcuts_become_copy_cut_and_paste() {
        let mut input = InputState::new(false);
        let ctrl = CTRL;
        let events = events_of(
            &mut input,
            &[
                key(EngineKey::LeftControl, true),
                key(EngineKey::C, true),
                key(EngineKey::X, true),
                key(EngineKey::V, true),
            ],
        );
        // Nothing to paste yet: egui-winit swallows the shortcut.
        let expected = [
            Event::ModifiersChanged(ctrl),
            winit_key(Key::ControlLeft, true, ctrl),
            Event::Copy,
            Event::Cut,
        ];
        assert_eq!(events, expected);

        input.clipboard = "text".to_owned();
        let events = events_of(&mut input, &[key(EngineKey::V, true)]);
        assert_eq!(events, [Event::Paste("text".to_owned())]);
    }

    #[test]
    fn the_command_modifier_is_control_or_super_by_platform() {
        let held = |mac, held_key| {
            let mut input = InputState::new(mac);
            events_of(&mut input, &[key(held_key, true)]);
            input.modifiers()
        };
        let ctrl = held(false, EngineKey::LeftControl);
        assert!(ctrl.ctrl && ctrl.command && !ctrl.mac_cmd);
        let windows_key = held(false, EngineKey::LeftWindows);
        assert!(!windows_key.command && !windows_key.mac_cmd);
        let cmd = held(true, EngineKey::LeftCommand);
        assert!(cmd.command && cmd.mac_cmd && !cmd.ctrl);
    }

    /// Runs one egui pass over a text field with the scripted input.
    fn type_into(
        ctx: &egui::Context,
        input: &mut InputState,
        text: &mut String,
        script: &[ScriptedInput],
    ) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(200.0, 100.0),
            )),
            ..egui::RawInput::default()
        };
        for item in script {
            input.push(item, &mut raw.events);
        }
        ctx.begin_pass(raw);
        ctx.root_ui(|ui| {
            ui.add(egui::TextEdit::singleline(text).id(Id::unique("name")));
        });
        ctx.end_pass().textures_delta.clear();
    }

    /// A replay renames an element: it types, presses Backspace, then Shift+Home selects the line
    /// and the next text replaces it. Without the keys and the modifiers reaching egui, the field
    /// would end with "Xab" or "abc".
    #[test]
    fn a_scripted_text_field_edit_matches_the_window_path() {
        let ctx = egui::Context::default();
        let mut input = InputState::new(ctx.os().is_mac());
        let mut text = String::new();
        ctx.memory_mut(|memory| memory.request_focus(Id::unique("name")));
        type_into(&ctx, &mut input, &mut text, &[]);

        let typed = [
            ScriptedInput::Text("abc".to_owned()),
            key(EngineKey::Backspace, true),
            key(EngineKey::Backspace, false),
        ];
        type_into(&ctx, &mut input, &mut text, &typed);
        assert_eq!(text, "ab");

        let select = [
            key(EngineKey::LeftShift, true),
            key(EngineKey::Home, true),
            key(EngineKey::Home, false),
            key(EngineKey::LeftShift, false),
            ScriptedInput::Text("X".to_owned()),
        ];
        type_into(&ctx, &mut input, &mut text, &select);
        assert_eq!(text, "X");
    }

    /// A window that gets a new painter has a renderer without textures, and egui sends none
    /// again: the font atlas has to go in whole, and the epoch changes for the other textures.
    #[test]
    fn a_new_renderer_gets_the_font_atlas_and_the_epoch_changes() {
        let Some(mut first) = headless_or_skip((80, 60), 1.0) else {
            return;
        };
        let Some(second) = headless_or_skip((80, 60), 1.0) else {
            return;
        };
        let mut app = PatternApp::new((8, 8), Pos2::ZERO);
        first.frame(&mut app, &[]).expect("a frame");
        let font = egui::TextureId::default();
        assert!(first.gpu.renderer.read().texture(&font).is_some());
        assert!(second.gpu.renderer.read().texture(&font).is_none());
        let epoch =
            |ctx: &egui::Context| ctx.data(|data| data.get_temp::<u64>(Id::unique(GPU_EPOCH)));
        assert_eq!(epoch(&first.ctx), None);

        crate::event_loop::send_egui_textures(&first.ctx, &second.gpu);
        let size = {
            let renderer = second.gpu.renderer.read();
            let sent = renderer.texture(&font).and_then(|t| t.texture.as_ref());
            sent.map(|texture| (texture.width() as usize, texture.height() as usize))
        };
        let [width, height] = first.ctx.fonts(|fonts| fonts.font_image_size());
        assert_eq!(
            size,
            Some((width, height)),
            "the whole atlas is on the device"
        );
        assert_eq!(epoch(&first.ctx), Some(1));
        crate::event_loop::send_egui_textures(&first.ctx, &second.gpu);
        assert_eq!(epoch(&first.ctx), Some(2));
    }
}
