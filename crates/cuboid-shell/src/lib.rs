#![forbid(unsafe_code)]
//! Layer L5. The winit event loop, egui-winit state, egui-wgpu painter, engine key mapping,
//! window rules and headless mode.
//!
//! Owns the platform window, input translation, painter, presentation and headless frames.
//! Must not contain application logic.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod backend;
pub mod contract;
mod event_loop;
mod headless;
pub mod keys;
pub mod pattern;
pub mod window;

pub use backend::{BackendPlan, instance_flags};
pub use contract::*;
pub use event_loop::run;
pub use headless::Headless;
