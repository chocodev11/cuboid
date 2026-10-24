#![forbid(unsafe_code)]
//! Layer L5. The modern window: menu bar with mode tabs, outliner, properties, viewport area,
//! status bar, command palette, keymap presets, start screen, preferences, undo history and the
//! recovery dialog (roadmap_lean 6).
//!
//! The crate owns everything visible in the window and nothing else. It never touches the
//! document: the app describes the document each frame with [`UiCx`] and applies the
//! [`UiRequest`]s that come back in [`UiOutput`]. Long work (open, save, export) is only ever
//! requested here; the app runs it on its I/O thread and shows progress through
//! [`UiCx::jobs`].
//!
//! # Entry point
//!
//! ```no_run
//! use cuboid_ui::{FontSet, Prefs, RecentEntry, Ui, UiCx, UiRequest};
//!
//! // Once: load the stored text (any failure gives the defaults) and build the UI.
//! let mut ui = Ui::new(Prefs::from_json("{}"), FontSet::default());
//!
//! // Every frame, inside the shell's `ShellApp::frame` (`cx.egui` is the `egui::Context`):
//! # fn frame(ui: &mut Ui, ctx: &egui::Context, recent: &[RecentEntry]) {
//! let cx = UiCx::empty(recent); // or `UiCx { doc: Some(..), views, .. }` for an open document
//! let out = ui.frame(ctx, &cx);
//! for request in out.requests {
//!     match request {
//!         UiRequest::Save => { /* snapshot the document, hand it to the I/O thread */ }
//!         UiRequest::Open(_path) => { /* load on the I/O thread, then swap the document */ }
//!         UiRequest::Quit => { /* FrameRequest { quit: true, .. } */ }
//!         _ => {}
//!     }
//! }
//! // `out.viewports` tells where each view was drawn: resize the view textures to `size_px`.
//! # }
//! ```
//!
//! * [`Ui::new`] takes the stored [`Prefs`] (see [`Prefs::from_json`] and [`Prefs::to_json`]; the
//!   app stores the text, for example in `ui.json` next to `settings.json`, with an atomic write)
//!   and the interface fonts. egui is built without bundled fonts: pass Open Sans from
//!   `cuboid-assets` in [`FontSet::ui`]; without it a system font is looked up once.
//! * [`Ui::frame`] builds the whole window on an `egui::Context` (it opens the root `Ui`
//!   itself); [`Ui::show`] does the same inside a root `Ui` you already hold, for tests.
//! * [`UiCx`] is the read-only description of the frame: the open document as [`DocView`]
//!   (`None` shows the start screen), the recent files, running jobs, one [`ViewportFrame`] per
//!   scene view (the egui texture id of the render target) and which actions are currently
//!   enabled. Build the `nodes` and `props` slices from the core only when the document
//!   revision changed and keep them between frames.
//! * [`UiOutput::requests`] are applied in order after the frame: [`UiRequest::Action`] carries
//!   every `cuboid_actions::ActionId` (menus, palette, keys), [`UiRequest::Edit`] the direct
//!   edits of the panels ([`Edit`]), [`UiRequest::Viewport`] the pointer input over a view
//!   ([`ViewportEvent`]: orbit, pan, zoom, press, drag, release, named views), and the file
//!   requests [`UiRequest::OpenDialog`], [`UiRequest::Open`], [`UiRequest::Save`],
//!   [`UiRequest::SaveAs`], [`UiRequest::ExportDialog`], [`UiRequest::NewDocument`] and
//!   [`UiRequest::Quit`]. A request that would discard unsaved changes goes through a
//!   confirmation dialog first; its "Save" answer arrives as [`UiRequest::SaveThen`].
//! * [`UiOutput::viewports`] says where each view was drawn ([`ViewportPlacement`]); the app
//!   sizes the view textures from `size_px` and maps view pixels through `rect`.
//!   [`UiOutput::keyboard_captured`] is true while a text field or dialog holds the keyboard:
//!   the app must not run its own shortcut dispatch then. Shortcuts for the registry actions
//!   are handled here (the keymap presets), so the app does not run `cuboid_actions::Dispatcher`.
//! * Other calls: [`Ui::toast`], [`Ui::open_recovery`], [`Ui::close_requested`] (window close
//!   button), [`Ui::prefs`] and [`Ui::set_prefs`], [`Ui::mode`], [`Ui::set_mode`], [`Ui::tool`]
//!   and [`Ui::set_tool`].
//!
//! # Layout
//!
//! egui_dock and egui-phosphor both require egui 0.36 while the workspace is on 0.37, so the
//! window uses three resizable zones (left, right, bottom) whose panels can be moved between
//! them ([`Layout`]), and icons drawn by `cuboid-ui-kit`.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod colors;
mod commands;
mod common;
mod dialogs;
mod history;
mod keymap;
mod labels;
mod layout;
mod menubar;
mod model;
mod outliner;
mod prefs;
mod prefs_ui;
mod properties;
mod request;
mod start;
mod statusbar;
mod textures;
mod toolbar;
mod ui;
mod uv;
mod viewport;

pub use cuboid_ui_kit::{FontSet, Level};
pub use dialogs::{RecoveryEntry, RecoveryInfo};
pub use keymap::{Chord, Command, KeyOverride, Keymap, KeymapPreset, UiCommand, all_ui_commands};
pub use labels::{
    Entry as PaletteEntry, action_category, action_label, entries as palette_entries,
    search as palette_search,
};
pub use layout::{Layout, PanelId, ViewLayout, Zone};
pub use model::{
    BandView, DisplaySlotView, DisplayView, DocView, Face, FaceView, HistoryView, ImageData,
    JobView, NodeId, NodeKind, NodeView, PaletteView, PropsView, RecentEntry, TextureView, UiCx,
    ViewSlot, ViewportFrame,
};
pub use prefs::{ColorMode, NavPreset, Prefs, Snap};
pub use request::{
    Button, ColorSlot, Edit, ElementFlag, FaceChange, KeyMods, Mode, RecoveryChoice, SelectOp,
    Template, Tool, ToolOptions, TransformField, UiOutput, UiRequest, ViewPreset, ViewportEvent,
    ViewportPlacement,
};
pub use ui::Ui;
