//! Start-up measurement. `--trace-startup` (or `CUBOID_TRACE_STARTUP=1`) writes one line per phase
//! with the milliseconds since the first instant of `main`, to the log and to stderr, and one
//! summary line when the start is complete. While the trace is on, `CUBOID_EXIT_AFTER=<seconds>`
//! quits through the normal quit path that long after the first full frame, so a script can
//! measure the start and the idle memory without any input; a value left in the environment does
//! nothing otherwise. Both cost a flag check per call when off.

use std::io::{self, Write as _};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use cuboid_shell::ShellPhase;

use crate::state::{Intent, State};

/// A step of the start-up, in the order the steps normally happen.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The settings file and the UI preferences are read.
    Settings,
    /// The state, the file threads and the UI are built; the shell starts.
    App,
    /// The event loop exists.
    EventLoop,
    /// The window exists, still hidden.
    Window,
    /// The graphics instance exists.
    Instance,
    /// The adapter is chosen; the device request follows.
    Adapter,
    /// The device, the egui renderer and the configured surface.
    Gpu,
    /// The window was made visible.
    Visible,
    /// The renderer exists: its pipelines are built.
    Renderer,
    /// The library thread starts reading the index of the global library.
    LibraryStart,
    /// The first UI pass is built: the fonts and icons are installed and the first layout has
    /// rasterised their glyphs.
    Ui,
    /// The first frame is built; painting and presenting follow.
    Built,
    /// The library thread has read the index; set on that thread, not when the UI polls it.
    LibraryDone,
    /// The first frame is on the window.
    FirstFrame,
    /// A file was named on the command line, and a frame with its document is on the window.
    /// Stays the last variant: [`Phase::COUNT`] counts on it.
    Document,
}

impl Phase {
    const COUNT: usize = Phase::Document as usize + 1;

    fn name(self) -> &'static str {
        match self {
            Phase::Settings => "settings",
            Phase::App => "app",
            Phase::EventLoop => "event_loop",
            Phase::Window => "window",
            Phase::Instance => "instance",
            Phase::Adapter => "adapter",
            Phase::Gpu => "gpu",
            Phase::Visible => "visible",
            Phase::Renderer => "renderer",
            Phase::LibraryStart => "library_start",
            Phase::Ui => "ui",
            Phase::Built => "built",
            Phase::LibraryDone => "library_done",
            Phase::FirstFrame => "first_frame",
            Phase::Document => "document",
        }
    }

    /// The phase a milestone of the shell is; a drawn frame is not one by itself.
    fn of_shell(phase: ShellPhase) -> Option<Phase> {
        Some(match phase {
            ShellPhase::EventLoop => Phase::EventLoop,
            ShellPhase::Window => Phase::Window,
            ShellPhase::Instance => Phase::Instance,
            ShellPhase::Adapter => Phase::Adapter,
            ShellPhase::Gpu => Phase::Gpu,
            ShellPhase::Visible => Phase::Visible,
            ShellPhase::Built => Phase::Built,
            ShellPhase::Presented => return None,
        })
    }
}

/// The phases reached so far.
struct Trace {
    start: Instant,
    /// A file was named on the command line, so its document is a phase of the start.
    document: bool,
    /// In the order they were reached.
    marks: Vec<(Phase, Duration)>,
    /// A frame with the document was built; the next drawn frame shows it.
    document_built: bool,
    summarised: bool,
    /// The logger writes its Info lines to stderr (`--debug`), so the raw line would be a second.
    echoed: bool,
}

fn millis(at: Duration) -> String {
    format!("{:.1}", at.as_secs_f64() * 1000.0)
}

impl Trace {
    fn new(start: Instant, document: bool, echoed: bool) -> Trace {
        Trace {
            start,
            document,
            marks: Vec::with_capacity(Phase::COUNT),
            document_built: false,
            summarised: false,
            echoed,
        }
    }

    /// Records `phase` the first time it is reached. Returns the lines to write: the phase, and
    /// the summary when it was the last one.
    fn mark(&mut self, phase: Phase, now: Instant) -> Vec<String> {
        if self.marks.iter().any(|(seen, _)| *seen == phase) {
            return Vec::new();
        }
        let at = now.saturating_duration_since(self.start);
        self.marks.push((phase, at));
        let mut lines = vec![format!("startup: {} {} ms", phase.name(), millis(at))];
        if self.marks.len() == Phase::COUNT - usize::from(!self.document) {
            lines.extend(self.summary());
        }
        lines
    }

    /// A frame was drawn. The first one is the first frame; once a frame with the document was
    /// built, the drawn frame is the first one with the document.
    fn presented(&mut self, now: Instant) -> Vec<String> {
        let mut lines = self.mark(Phase::FirstFrame, now);
        if self.document && self.document_built {
            lines.extend(self.mark(Phase::Document, now));
        }
        lines
    }

    /// The summary line with every phase reached, once.
    fn summary(&mut self) -> Option<String> {
        if std::mem::replace(&mut self.summarised, true) {
            return None;
        }
        let phases: Vec<String> = self
            .marks
            .iter()
            .map(|(phase, at)| format!("{}={}", phase.name(), millis(*at)))
            .collect();
        Some(format!("startup: summary {} (ms)", phases.join(" ")))
    }
}

static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();

fn wanted(flag: bool, env: Option<&str>) -> bool {
    flag || env == Some("1")
}

/// Starts the trace when `--trace-startup` or `CUBOID_TRACE_STARTUP=1` asks for it. `start` is
/// the first instant of `main`; `document` is true when a file was named on the command line;
/// `echo` is true when the logger writes its Info lines to stderr (`--debug`).
pub fn init(flag: bool, echo: bool, start: Instant, document: bool) {
    let env = std::env::var("CUBOID_TRACE_STARTUP");
    if wanted(flag, env.as_deref().ok()) {
        let _ = TRACE.set(Mutex::new(Trace::new(start, document, echo)));
    }
}

/// True while the start-up trace is on.
pub fn tracing() -> bool {
    TRACE.get().is_some()
}

/// Applies `change` to the trace, if there is one, and writes the lines it returns. They are
/// written under the lock, so they come in the order the phases happened.
fn update(change: impl FnOnce(&mut Trace, Instant) -> Vec<String>) {
    let Some(trace) = TRACE.get() else { return };
    let now = Instant::now();
    let mut trace = trace.lock().unwrap_or_else(PoisonError::into_inner);
    let raw = !(trace.echoed && log::log_enabled!(log::Level::Info));
    for line in change(&mut trace, now) {
        log::info!("{line}");
        if raw {
            let _ = writeln!(io::stderr(), "{line}");
        }
    }
}

/// Records `phase` the first time it is reached; any thread may call it.
pub fn mark(phase: Phase) {
    update(|trace, now| trace.mark(phase, now));
}

/// Hears a milestone of the shell.
pub fn shell(phase: ShellPhase) {
    match Phase::of_shell(phase) {
        Some(phase) => mark(phase),
        None => update(Trace::presented),
    }
}

/// Called after every built frame; `has_document` is asked only while tracing.
pub fn frame_built(has_document: impl FnOnce() -> bool) {
    if tracing() && has_document() {
        update(|trace, _| {
            trace.document_built = true;
            Vec::new()
        });
    }
}

/// Writes the summary of a start that did not reach every phase, when the program ends.
pub fn finish() {
    update(|trace, _| trace.summary().into_iter().collect());
}

/// What the scripted exit asks of a frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Exit {
    /// Nothing: the start-up work is not over, or the exit was asked for already.
    Idle,
    /// Counting; a frame must run at this instant.
    Wait(Instant),
    /// The time is up: quit now.
    Quit,
}

/// `CUBOID_EXIT_AFTER=<seconds>`: quits that long after the first full frame, which is the first
/// frame once the start-up work is over (a file named on the command line is open). Read only
/// while the trace is on. The quit asks about unsaved work like the close button does.
pub struct ExitAfter {
    delay: Duration,
    /// The first full frame plus the delay.
    due: Option<Instant>,
    fired: bool,
}

fn parse_seconds(text: &str) -> Option<Duration> {
    let seconds = text.trim().parse::<f64>().ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

impl ExitAfter {
    fn new(delay: Duration) -> ExitAfter {
        ExitAfter {
            delay,
            due: None,
            fired: false,
        }
    }

    /// Reads `CUBOID_EXIT_AFTER`. A value that is not a number of seconds is reported and ignored.
    pub fn from_env() -> Option<ExitAfter> {
        let text = std::env::var("CUBOID_EXIT_AFTER").ok()?;
        let delay = parse_seconds(&text);
        if delay.is_none() {
            log::warn!("CUBOID_EXIT_AFTER={text} is not a number of seconds; ignored");
        }
        delay.map(ExitAfter::new)
    }

    /// Asked after every built frame. The start-up work is over when no file is being opened.
    /// The quit is the one the close button asks for, run by the next frame. Returns the instant
    /// by which a frame must run.
    pub fn frame(&mut self, state: &mut State, now: Instant) -> Option<Instant> {
        match self.step(now, state.doc_job.is_none() && state.inbox.is_empty()) {
            Exit::Idle => None,
            Exit::Wait(due) => Some(due),
            Exit::Quit => {
                state.inbox.push(Intent::Quit);
                Some(now)
            }
        }
    }

    fn step(&mut self, now: Instant, settled: bool) -> Exit {
        if self.fired {
            return Exit::Idle;
        }
        if self.due.is_none() && settled {
            self.due = now.checked_add(self.delay);
            if self.due.is_none() {
                // A delay beyond the range of the clock never ends.
                self.fired = true;
                log::warn!(
                    "CUBOID_EXIT_AFTER: {} s is too long; not quitting",
                    self.delay.as_secs_f64()
                );
                return Exit::Idle;
            }
        }
        let Some(due) = self.due else {
            return Exit::Idle;
        };
        if now < due {
            return Exit::Wait(due);
        }
        self.fired = true;
        log::warn!(
            "CUBOID_EXIT_AFTER: {} s after the first full frame, quitting",
            self.delay.as_secs_f64()
        );
        Exit::Quit
    }
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind};
    use cuboid_settings::{Environment, Settings};

    use super::*;
    use crate::io_thread::IoThread;
    use crate::platform::Dirs;
    use crate::state::Dialog;

    const MS: Duration = Duration::from_millis(1);

    /// A trace and the instant it started at.
    fn started(document: bool) -> (Trace, Instant) {
        let start = Instant::now();
        (Trace::new(start, document, false), start)
    }

    #[test]
    fn a_phase_is_written_once_with_the_milliseconds_since_the_start() {
        let (mut trace, start) = started(false);
        let lines = trace.mark(Phase::Window, start + 120 * MS + MS / 2);
        assert_eq!(lines, ["startup: window 120.5 ms"]);
        assert!(trace.mark(Phase::Window, start + 300 * MS).is_empty());
        assert_eq!(trace.marks, [(Phase::Window, 120 * MS + MS / 2)]);
    }

    #[test]
    fn the_summary_comes_once_with_the_phases_in_the_order_they_were_reached() {
        let (mut trace, start) = started(false);
        // Not the declaration order: the library thread finishes before the first frame.
        let order = [
            Phase::Settings,
            Phase::App,
            Phase::EventLoop,
            Phase::Window,
            Phase::Instance,
            Phase::Adapter,
            Phase::Gpu,
            Phase::Visible,
            Phase::Renderer,
            Phase::LibraryStart,
            Phase::LibraryDone,
            Phase::Ui,
            Phase::Built,
        ];
        for (index, phase) in order.into_iter().enumerate() {
            let at = start + MS * u32::try_from(index).unwrap();
            assert_eq!(trace.mark(phase, at).len(), 1, "{phase:?}");
        }
        // Without a file the document is not expected: the first frame completes the start.
        let lines = trace.presented(start + 20 * MS);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0], "startup: first_frame 20.0 ms");
        let summary = &lines[1];
        assert!(
            summary.starts_with("startup: summary settings=0.0 app=1.0 "),
            "{summary}"
        );
        assert!(summary.contains("library_done=10.0 ui=11.0 built=12.0 first_frame=20.0"));
        assert!(trace.presented(start + 40 * MS).is_empty());
        assert!(trace.summary().is_none());
    }

    #[test]
    fn a_file_makes_the_document_a_phase_that_a_drawn_frame_with_it_completes() {
        let (mut trace, start) = started(true);
        for phase in [
            Phase::Settings,
            Phase::App,
            Phase::EventLoop,
            Phase::Window,
            Phase::Instance,
            Phase::Adapter,
            Phase::Gpu,
            Phase::Visible,
            Phase::Renderer,
            Phase::LibraryStart,
            Phase::Ui,
            Phase::Built,
            Phase::LibraryDone,
        ] {
            trace.mark(phase, start);
        }
        // The first frame has no document yet, so no summary.
        let lines = trace.presented(start + 20 * MS);
        assert_eq!(lines, ["startup: first_frame 20.0 ms"]);
        // A frame with the document was built, and the next drawn frame shows it.
        trace.document_built = true;
        let lines = trace.presented(start + 90 * MS);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0], "startup: document 90.0 ms");
        assert!(
            lines[1].contains("first_frame=20.0 document=90.0"),
            "{lines:?}"
        );
    }

    #[test]
    fn without_a_file_a_built_document_is_not_a_phase() {
        let (mut trace, start) = started(false);
        trace.document_built = true;
        trace.presented(start);
        assert_eq!(trace.marks.len(), 1);
    }

    #[test]
    fn finish_writes_the_summary_of_an_incomplete_start_once() {
        let (mut trace, start) = started(true);
        trace.mark(Phase::Settings, start + 5 * MS);
        let lines: Vec<String> = trace.summary().into_iter().collect();
        assert_eq!(lines, ["startup: summary settings=5.0 (ms)"]);
        assert!(trace.summary().is_none());
    }

    #[test]
    fn the_shell_milestones_are_phases_and_a_drawn_frame_is_not() {
        let shell = |phase| Phase::of_shell(phase).map(Phase::name);
        assert_eq!(shell(ShellPhase::EventLoop), Some("event_loop"));
        assert_eq!(shell(ShellPhase::Gpu), Some("gpu"));
        assert_eq!(shell(ShellPhase::Built), Some("built"));
        assert_eq!(shell(ShellPhase::Presented), None);
    }

    #[test]
    fn the_trace_is_asked_for_by_the_flag_or_by_the_value_one() {
        assert!(wanted(true, None));
        assert!(wanted(false, Some("1")));
        assert!(!wanted(false, None));
        assert!(!wanted(false, Some("0")));
        assert!(!wanted(false, Some("")));
    }

    #[test]
    fn the_exit_delay_is_a_number_of_seconds() {
        assert_eq!(parse_seconds("0"), Some(Duration::ZERO));
        assert_eq!(parse_seconds(" 12 "), Some(Duration::from_secs(12)));
        assert_eq!(parse_seconds("2.5"), Some(Duration::from_millis(2500)));
        for bad in ["", "soon", "-1", "NaN", "inf", "1e400", "5s"] {
            assert_eq!(parse_seconds(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_exit_waits_for_the_start_to_settle_and_then_for_the_delay() {
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::from_secs(3));
        // The delay does not run while the file is still opening.
        assert_eq!(exit.step(now, false), Exit::Idle);
        assert_eq!(exit.step(now + Duration::from_secs(9), false), Exit::Idle);
        let settled = now + Duration::from_secs(10);
        let due = settled + Duration::from_secs(3);
        assert_eq!(exit.step(settled, true), Exit::Wait(due));
        // The clock started at the first settled frame and keeps its deadline.
        assert_eq!(
            exit.step(settled + Duration::from_secs(2), true),
            Exit::Wait(due)
        );
        assert_eq!(exit.step(due, true), Exit::Quit);
        assert_eq!(exit.step(due + MS, true), Exit::Idle);
    }

    #[test]
    fn a_zero_delay_quits_at_the_first_settled_frame() {
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::ZERO);
        assert_eq!(exit.step(now, false), Exit::Idle);
        assert_eq!(exit.step(now, true), Exit::Quit);
        assert_eq!(exit.step(now, true), Exit::Idle);
    }

    #[test]
    fn a_delay_beyond_the_clock_never_quits_and_never_panics() {
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::MAX);
        assert_eq!(exit.step(now, false), Exit::Idle);
        assert_eq!(exit.step(now, true), Exit::Idle);
        assert_eq!(exit.step(now + Duration::from_secs(60), true), Exit::Idle);
    }

    fn state_in(dir: &std::path::Path) -> State {
        let env = Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        };
        let settings = Settings::first_run(&env);
        let io = IoThread::spawn().unwrap();
        State::new(Dirs::under(dir), settings, io, 1252, Instant::now())
    }

    /// Steps `state` until no job runs.
    fn settle(state: &mut State) {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            state.step(Instant::now());
            if !state.is_busy() {
                return;
            }
            assert!(Instant::now() < end, "jobs did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn an_idle_start_quits_through_the_normal_quit() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_in(dir.path());
        state.startup(None);
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::ZERO);
        assert_eq!(exit.frame(&mut state, now), Some(now));
        assert_eq!(state.inbox, [Intent::Quit]);
        assert_eq!(exit.frame(&mut state, now), None);
        // The next frame runs the intent like the one of the close button.
        state.step(now);
        assert!(state.quit);
    }

    #[test]
    fn a_dirty_document_gets_the_save_question_instead_of_quitting() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_in(dir.path());
        state.startup(None);
        state.run_command(Command::AddElement(ElementKind::Cube));
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::ZERO);
        assert_eq!(exit.frame(&mut state, now), Some(now));
        state.step(now);
        assert!(!state.quit);
        assert!(matches!(state.dialog, Some(Dialog::Unsaved { .. })));
    }

    #[test]
    fn the_delay_does_not_run_while_the_named_file_opens() {
        let maker_dir = tempfile::tempdir().unwrap();
        let start_dir = tempfile::tempdir().unwrap();
        let path = maker_dir.path().join("lamp.cubik");
        let mut maker = state_in(maker_dir.path());
        maker.run_command(Command::AddElement(ElementKind::Cube));
        maker.dispatch(Intent::SaveAs(Some(path.clone())));
        settle(&mut maker);
        assert!(path.is_file());

        let mut state = state_in(start_dir.path());
        state.startup(Some(path));
        let now = Instant::now();
        let mut exit = ExitAfter::new(Duration::ZERO);
        // The open is queued, then running: not settled yet.
        assert_eq!(exit.frame(&mut state, now), None);
        state.step(now);
        assert!(state.doc_job.is_some());
        assert_eq!(exit.frame(&mut state, now), None);
        settle(&mut state);
        let open = state.editor.path().unwrap();
        assert!(open.ends_with("lamp.cubik"), "{open:?}");
        assert_eq!(exit.frame(&mut state, now), Some(now));
        assert_eq!(state.inbox, [Intent::Quit]);
    }
}
