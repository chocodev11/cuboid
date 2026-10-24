#![forbid(unsafe_code)]
//! Layer L6. Binary `cuboid`: wiring, jobs, I/O thread, backups, lifecycle, crash handling,
//! platform integration and logging.
//!
//! Owns composition, flows, jobs and OS integration. Must not contain format or model logic.
//!
//! | Module | Content |
//! |---|---|
//! | [`lifecycle`] | command line, start-up order, shut-down |
//! | [`app`] | the `ShellApp`: events in, one guarded frame out |
//! | [`state`], [`flows`] | application state, intents, toasts, dialogs, open/save/close/recovery |
//! | [`editor`] | the document, the unsaved flag and the last good state |
//! | [`jobs`], [`io_thread`] | job handles with progress and cancel; the FIFO file thread |
//! | [`backup`] | autosave pairs in the recovery folder and the start-up scan |
//! | [`crash`], [`logging`] | panic hook, crash reports, the panic boundary; rotating log file |
//! | [`frontend`], [`requests`], [`uimodel`], [`render_glue`] | the window each frame: UI, requests, scene, renderer; the GPU probe |
//! | [`prompts`], [`ui_setup`] | the two questions only the app asks; the font and the stored UI preferences |

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod app;
mod backup;
mod crash;
#[cfg(test)]
mod e2e;
mod editor;
mod flows;
mod frontend;
mod io_thread;
mod jobs;
mod lifecycle;
mod logging;
mod platform;
mod prompts;
mod render_glue;
mod requests;
mod state;
mod ui_setup;
mod uimodel;

#[cfg(test)]
mod tests;

fn main() -> std::process::ExitCode {
    lifecycle::run(lifecycle::Args::parse(std::env::args().skip(1)))
}
