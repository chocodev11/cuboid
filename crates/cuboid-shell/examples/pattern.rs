//! Opens the shell with the test pattern and prints every engine key and window event, so the
//! key mapping (keypad against top-row digits, left against right modifiers) can be tried by
//! hand: `cargo run -p cuboid-shell --example pattern`. Alt+Enter and F11 toggle full screen.

use cuboid_shell::pattern::TestPattern;
use cuboid_shell::{
    EngineKeyEvent, FrameCx, FrameRequest, ShellApp, ShellError, ShellEvent, ShellOptions, run,
};

#[derive(Default)]
struct Demo {
    pattern: Option<TestPattern>,
    title_sent: bool,
    quit: bool,
}

impl ShellApp for Demo {
    fn engine_key(&mut self, ev: EngineKeyEvent) {
        let state = if ev.down { "down" } else { "up" };
        eprintln!("{:?} {state}", ev.key);
    }

    fn window_event(&mut self, ev: ShellEvent) {
        eprintln!("{ev:?}");
        self.quit |= ev == ShellEvent::CloseRequested;
    }

    fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest {
        let pattern = self
            .pattern
            .get_or_insert_with(|| TestPattern::new(cx.gpu, (256, 256)));
        cx.egui
            .root_ui(|ui| pattern.paint(ui.painter(), egui::pos2(16.0, 16.0), cx.ui_scale));
        let title =
            (!self.title_sent).then(|| format!("cuboid shell spike, scale {}", cx.ui_scale));
        self.title_sent = true;
        FrameRequest {
            title,
            quit: self.quit,
            ..FrameRequest::default()
        }
    }
}

fn main() -> Result<(), ShellError> {
    run(Demo::default(), ShellOptions::default())
}
