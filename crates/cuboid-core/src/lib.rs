#![forbid(unsafe_code)]
//! Layer L4. The `Core` facade: commands as data, history, clipboards, the interaction state
//! machine, tools, `Compat` and `Clock`.
//!
//! Owns commands, guards, undo/redo (tools 15), clipboards (tools 11) and the interaction
//! lifecycles of tools 8-9 and 12-13. Must not depend on egui, winit or wgpu.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod clipboard;
pub mod clock;
pub mod commands;
pub mod compat;
pub mod contract;
pub mod core;
pub mod history;
pub mod interact;
pub mod query;
pub mod start;
pub mod status;
pub mod tools;

pub use crate::core::Core;
pub use clipboard::{ClipboardKind, Clipboards, ObjectClipboard};
pub use clock::Clock;
pub use commands::{
    AlignEdge, AnimationLoop, AutoUvStyle, Axis, BoneChannel, CmdOutcome, ColliderAction, Command,
    CommandDef, CommandKind, DisplayContext, Done, Effects, ElementKind, FaceId, Guard, KeyAt,
    KeyInterp, MeshShape, Refresh, Refusal, SelectOp, Target, UvAxis,
};
pub use compat::Compat;
pub use contract::{Button, CustomTool, Stale, StrokeStep, SwapKind};
pub use history::{Applied, Entry, History, MAX_BYTES, MAX_ENTRIES, UndoCallback, Weigh};
pub use interact::{
    ButtonState, CursorSprite, InputFrame, Interaction, Modifiers, SelectMode, SnapSettings,
    TickOutput, ToolCursor, ViewInput,
};
pub use query::{
    AppQuery, FaceProps, ObjectKind, ObjectProps, OutlinerRow, SelectionSummary, outliner,
};
pub use start::StartQueue;
pub use status::StatusLine;
pub use tools::{
    PaintOptions, PaintTool, PaintToolKind, ToolColors, ToolCx, ToolSizes, VoxelOptions, VoxelTool,
    VoxelToolKind,
};
