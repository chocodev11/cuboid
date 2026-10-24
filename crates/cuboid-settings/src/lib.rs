#![forbid(unsafe_code)]
//! Layer L1. Settings schema with the original's keys, defaults, load and save, legacy import
//! and recent files.
//!
//! Owns the data and persistence of app 10 and 11. Must not call OS APIs; the app passes legacy
//! values in.
//!
//! [`Settings`] holds the 54 keys of the original's `cubikdata.json` under their own names, plus
//! the port's [`Cuboid`] section, and is stored as one pretty JSON file ([`Settings::load`],
//! [`Settings::save`]). First-run values come from [`Settings::first_run`]. A file that exists
//! but lacks a key gets the original's field value for that key, which differs for a few keys
//! (menus 9.1); an unusable value takes the first-run value of its key and the other keys are
//! kept (app 0.2 D10).

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod cuboid;
mod json;
pub mod locations;
pub mod recent;
pub mod schema;
pub mod store;

pub use cuboid::Cuboid;
pub use locations::{Location, LocationKind, Locations};
pub use recent::{RecentFile, RecentFiles};
pub use schema::{DEFAULT_SIGNATURE, Environment, Settings, THEMES};
pub use store::{FILE_NAME, Legacy, Loaded, Origin, ParseError, SettingsError};

/// Captures the log lines of the current test thread.
#[cfg(test)]
mod test_log {
    use std::cell::RefCell;
    use std::sync::Once;

    use log::{LevelFilter, Log, Metadata, Record};

    thread_local! {
        static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    struct Capture;

    impl Log for Capture {
        fn enabled(&self, _: &Metadata) -> bool {
            true
        }

        fn log(&self, record: &Record) {
            LINES.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }

        fn flush(&self) {}
    }

    /// Starts capturing and drops the lines logged so far on this thread.
    pub(crate) fn capture() {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            log::set_logger(&Capture).expect("no other logger in the tests");
            log::set_max_level(LevelFilter::Trace);
        });
        LINES.with_borrow_mut(Vec::clear);
    }

    /// The lines logged on this thread since [`capture`].
    pub(crate) fn lines() -> Vec<String> {
        LINES.with_borrow_mut(std::mem::take)
    }
}
