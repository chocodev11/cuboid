#![forbid(unsafe_code)]
//! Layer L5. The modern window: top bar with menus and mode tabs, tool strip, outliner,
//! properties, viewport area, status bar, command palette, keymap presets, start screen,
//! preferences, undo history and the recovery dialog (roadmap_lean 6, `docs/design/ui_v2_spec.md`).
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
//! * The asset library is plain data too: [`UiCx::library`] holds a [`LibraryView`] built by the
//!   app (counts, the entries that match the search, the brief, hints, pictures as
//!   [`ThumbState`]); the start screen and the Library panel send [`LibraryRequest`]s inside
//!   [`UiRequest::Library`]. The crate never reads library files. On a fresh install a dialog
//!   over the start screen asks which app the user knows ([`ComingFrom`]: Blockbench, Cubik Studio
//!   or Blender), which sets the camera buttons, the keymap and the panel arrangement together,
//!   and offers an accent colour; until the accent is chosen the interface uses a neutral
//!   highlight ([`Prefs::accent_chosen`]).
//! * Other calls: [`Ui::toast`], [`Ui::open_recovery`], [`Ui::close_requested`] (window close
//!   button), [`Ui::prefs`] and [`Ui::set_prefs`], [`Ui::mode`], [`Ui::set_mode`], [`Ui::tool`]
//!   and [`Ui::set_tool`]. [`Ui::set_project`] describes the preset of the open document
//!   ([`ProjectView`]) for the View > Shading menu, the project settings window and the status
//!   bar; their choices come back as [`UiRequest::Project`].
//!
//! # Layout
//!
//! egui_dock and egui-phosphor both require egui 0.36 while the workspace is on 0.37, so the
//! window has two resizable areas (left and right of the viewports), each cut into two groups of
//! tabbed panels, and every panel can be moved to the other area ([`Layout`]). The icons come
//! from `cuboid-ui-kit`.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod brief;
mod colors;
mod commands;
mod common;
mod dialogs;
mod export_dialog;
mod first_run;
mod history;
mod keymap;
mod labels;
mod layout;
mod library;
mod model;
mod neutral;
mod outliner;
mod prefs;
mod prefs_ui;
mod project_settings;
mod properties;
mod props_area;
mod props_display;
mod props_model;
mod props_texture;
mod render_dialog;
mod request;
mod start;
mod statusbar;
mod textures;
mod toolbar;
mod topbar;
mod ui;
mod uv;
mod validator;
mod viewport;

pub use cuboid_ui_kit::{FontSet, Level};
pub use dialogs::{RecoveryEntry, RecoveryInfo};
pub use export_dialog::{
    ExportFormat, ExportInfo, ExportOutcome, ExportReport, ExportRequest, ExportSettings,
    ExportStage, VersionChoice,
};
pub use keymap::{Chord, Command, KeyOverride, Keymap, KeymapPreset, UiCommand, all_ui_commands};
pub use labels::{
    Entry as PaletteEntry, action_category, action_label, entries as palette_entries,
    search as palette_search,
};
pub use layout::{Arrangement, Group, Layout, PanelId, ViewLayout, Zone, ZoneState};
pub use library::{
    BriefCardView, BriefInputs, LibEntryView, LibKind, LibScope, LibraryRequest, LibraryView,
    SuggestionAction, SuggestionGoal, SuggestionView, ThumbState,
};
pub use model::{
    BandView, DisplaySlotView, DisplayView, DocView, Face, FaceView, HistoryView, ImageData,
    JobView, ModelView, NodeId, NodeKind, NodeView, PaletteView, PropsView, RecentEntry,
    TextureView, UiCx, ViewSlot, ViewportFrame,
};
pub use prefs::{ColorMode, ComingFrom, NavPreset, Prefs, Snap};
pub use project_settings::{PresetKind, ProjectChange, ProjectView, Shading};
pub use render_dialog::{
    RENDER_EDGE, RenderBackground, RenderCamera, RenderRequest, RenderSettings, RenderShading,
};
pub use request::{
    Button, ColorSlot, DisplayPreset, Edit, ElementFlag, FaceChange, KeyMods, Mode, RecoveryChoice,
    SelectOp, Template, Tool, ToolOptions, TransformField, UiOutput, UiRequest, ViewPreset,
    ViewportEvent, ViewportPlacement,
};
pub use ui::Ui;
pub use validator::{IssueLevel, IssueView, ValidatorRequest, ValidatorView};
pub use viewport::CUBIK_ORBIT_SCALE;
