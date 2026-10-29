//! The application as the shell sees it: window events in, one guarded frame out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cuboid_shell::{EngineKeyEvent, FrameCx, FrameRequest, ShellApp, ShellEvent};
use cuboid_ui::{FontSet, Prefs};

use crate::crash::{self, PanicGuard};
use crate::frontend::Frontend;
use crate::state::{Intent, State};

/// This many paint failures within [`PAINT_FAILURE_SPAN`] end the program. The shell tries a new
/// device every half second, so a GPU that was just reset has a few seconds to come back.
const PAINT_FAILURES: usize = 8;
const PAINT_FAILURE_SPAN: Duration = Duration::from_secs(30);

pub struct App {
    pub state: State,
    guard: PanicGuard,
    /// Counts the frames the shell could not paint.
    paint_guard: PanicGuard,
    pub front: Frontend,
    /// The client size of the last resize event, kept for the settings.
    resized: Option<(u32, u32)>,
}

/// Runs the app logic of a frame under the panic boundary. Only a frame that ran to the end
/// makes its document the last good state (never in safe mode). After a panic the frame is
/// dropped, the document goes back to its last good state, that state is kept in the recovery
/// folder and a toast tells the user; repeated panics open the recovery dialog. True when `body`
/// finished.
fn run_guarded(
    guard: &mut PanicGuard,
    state: &mut State,
    now: Instant,
    body: impl FnOnce(&mut State),
) -> bool {
    let frame = || {
        body(state);
        if !state.safe_mode {
            state.editor.commit_frame(now);
        }
    };
    match guard.run(frame) {
        Ok(()) => true,
        Err(panicked) => {
            state.now = now;
            state.recover_from_panic(&panicked);
            false
        }
    }
}

impl App {
    /// An app with the default preferences and no stored UI file.
    #[cfg(test)]
    pub fn new(state: State) -> App {
        App::with_ui(state, Prefs::default(), FontSet::default(), PathBuf::new())
    }

    pub fn with_ui(state: State, prefs: Prefs, fonts: FontSet, prefs_path: PathBuf) -> App {
        App {
            state,
            guard: PanicGuard::default(),
            paint_guard: PanicGuard::new(PAINT_FAILURES, PAINT_FAILURE_SPAN),
            front: Frontend::new(prefs, fonts, prefs_path),
            resized: None,
        }
    }

    /// See [`run_guarded`].
    #[cfg(test)]
    pub fn guarded(&mut self, now: Instant, body: impl FnOnce(&mut State)) -> bool {
        run_guarded(&mut self.guard, &mut self.state, now, body)
    }
}

impl ShellApp for App {
    fn engine_key(&mut self, _ev: EngineKeyEvent) {
        // The shortcuts are the keymap of the UI, which reads egui's input.
    }

    fn window_event(&mut self, ev: ShellEvent) {
        match ev {
            ShellEvent::CloseRequested => self.state.inbox.push(Intent::Quit),
            ShellEvent::Resized { width, height } => self.resized = Some((width, height)),
            // egui receives the drop as well, and the UI asks its own question before it opens.
            ShellEvent::FilesDropped(_)
            | ShellEvent::Focus(_)
            | ShellEvent::ScaleFactor(_)
            | ShellEvent::Moved { .. } => {}
        }
    }

    fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest {
        let now = cx.now;
        if !cx.window.fullscreen {
            self.state.window.position = cx.window.position.or(self.state.window.position);
            self.state.window.size = self.resized.or(self.state.window.size);
        }
        let App {
            state,
            guard,
            front,
            ..
        } = self;
        let cx: &FrameCx<'_> = cx;
        let finished = run_guarded(guard, state, now, |state| front.frame(state, cx));
        if !finished {
            // egui holds a half-built pass; the shell should not show it.
            cx.egui.request_discard("the frame panicked");
        }
        if std::mem::take(&mut state.reset_guard) {
            guard.reset();
        }
        let mut wake = state.next_wake(now);
        if front.more {
            wake = now;
        }
        FrameRequest {
            next_wake: Some(wake),
            title: front.take_title(),
            window: front.take_window_commands(),
            quit: state.quit,
            recreate_gpu: std::mem::take(&mut state.recreate_gpu),
            ..FrameRequest::default()
        }
    }

    /// Outside the frame boundary: the shell's painter failed, and nothing in the frame logic
    /// was wrong. A panic here must not leave the program, so the step is caught as well; when
    /// the failures repeat, the program ends even then.
    fn paint_failed(&mut self, message: &str) {
        let repeated = self.paint_guard.note_panic(Instant::now());
        let kept = crash::catch("keeping the work after a paint failure", || {
            self.state.paint_failed(message, repeated);
        });
        if kept.is_none() && repeated {
            self.state.quit = true;
        }
    }
}
