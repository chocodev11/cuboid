#![forbid(unsafe_code)]
//! Layer L6. The MCP server of the agent toolkit (spec: `docs/design/agent_toolkit_r1.md`): the
//! protocol in both eras of MCP, the project paths and model handles, the tools and the ops of
//! `model_edit`, the fix mapping of the validator, the [`Renderer`] trait with the contact sheet,
//! and the client snippets and self-check of `cuboid mcp`.
//!
//! The server is synchronous: one thread reads a message per line from a stream, answers it and
//! reads the next, so the thread that owns the GPU can run it. It has no network, no shell and no
//! scripting; every path of a tool goes through [`paths::Root::resolve`].

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod brief;
pub mod check;
pub mod connect;
pub mod edit;
pub mod error;
pub mod fixes;
pub mod history;
pub mod inspect;
pub mod paths;
pub mod picture;
pub mod project;
pub mod protocol;
pub mod render;
pub mod schema;
pub mod select;
pub mod server;
pub mod texture;
pub mod tools;
pub mod views;

pub use connect::{CheckReport, CheckStep, Snippets, self_check, snippets};
pub use error::ToolError;
pub use fixes::{FixSummary, apply_fixes, fixable};
pub use paths::Root;
pub use picture::Picture;
pub use protocol::{ServeError, serve};
pub use render::{Camera, NoRenderer, RenderError, Renderer};
pub use server::{Options, Output, Server};
