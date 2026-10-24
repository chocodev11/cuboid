#![forbid(unsafe_code)]
//! Layer L0. Engine keys, key names, binding strings, chords, dispatcher, held-key map,
//! `ActionId` and menu tables.
//!
//! Owns engine key names (menus 6.6), canonical chords (menus 6.1), dispatch (menus 6.2), the
//! held map (menus 6.4) and the action registry and menu data (menus 1-3, 5). Must not depend
//! on egui, winit or core.
//!
//! The crate is pure data and state machines: the shell feeds [`Dispatcher`] with engine key
//! events, the UI reads [`MENUS`] and the owner of the clock runs the returned actions 0.05 s
//! later (menus 1.5).

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod binding;
mod contract;
mod dispatch;
mod held;
mod keys;
mod menus;
mod registry;
#[cfg(test)]
mod spec;

pub use binding::{Chord, canonical_binding, canonical_shortcut, parse_binding};
pub use contract::*;
pub use dispatch::Dispatcher;
pub use held::HeldMap;
pub use keys::EngineKey;
pub use menus::{MENUS, PASTE_OBJECTS, PASTE_VOXELS, RECENT_FILES_FOOTER};
pub use registry::ACTIONS;
