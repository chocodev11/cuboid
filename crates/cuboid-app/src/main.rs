#![forbid(unsafe_code)]
// Release builds are a Windows GUI program: no console window opens. `cuboid-term` is the console
// twin. Tests stay console programs in every profile; rustc applies the attribute to them too.
#![cfg_attr(not(any(debug_assertions, test)), windows_subsystem = "windows")]
//! Layer L7. Binary `cuboid`: wiring, jobs, I/O thread, backups, lifecycle, crash handling,
//! platform integration and logging.
//!
//! Owns composition, flows, jobs and OS integration. Must not contain format or model logic.
//!
//! | Module | Content |
//! |---|---|
//! | [`lifecycle`] | command line, start-up order, shut-down |
//! | [`startup`] | start-up phase timings (`--trace-startup`) and the scripted exit (`CUBOID_EXIT_AFTER`, only with the trace on) |
//! | [`app`] | the `ShellApp`: events in, one guarded frame out |
//! | [`state`], [`flows`] | application state, intents, toasts, dialogs, open/save/close/recovery |
//! | [`editor`] | the document, the unsaved flag and the last good state |
//! | [`preset`] | the preset of a document (Cubik or Minecraft version): defaults, open rules, validation target |
//! | [`jobs`], [`io_thread`] | job handles with progress and cancel; the FIFO file thread |
//! | [`backup`] | autosave pairs in the recovery folder and the start-up scan |
//! | [`crash`], [`logging`] | panic hook, crash reports, the panic boundary; rotating log file |
//! | [`frontend`], [`requests`], [`uimodel`], [`render_glue`] | the window each frame: UI, requests, scene, renderer; the GPU probe |
//! | [`library`], [`thumbs`] | the asset library: scopes, import and edit jobs, the view for the UI; 160 px pictures of models |
//! | [`prompts`], [`ui_setup`] | the two questions only the app asks; the font and the stored UI preferences |
//! | [`mcp`], [`mcp_render`] | `cuboid mcp`: the stdio server of the agent toolkit, run before any window exists; its offscreen renderer |

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
mod library;
mod lifecycle;
mod logging;
mod mcp;
mod mcp_render;
mod platform;
mod preset;
mod prompts;
mod reload;
mod render_glue;
mod requests;
mod startup;
mod state;
mod thumbs;
mod ui_setup;
mod uimodel;
mod validation;

#[cfg(test)]
mod tests;

fn main() -> std::process::ExitCode {
    // The start-up trace counts from here.
    let start = std::time::Instant::now();
    lifecycle::run(lifecycle::Args::parse(std::env::args().skip(1)), start)
}
