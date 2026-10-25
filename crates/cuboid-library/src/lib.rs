#![forbid(unsafe_code)]
//! Layer L5. The asset library of the start screen: models, textures, JSON, references and lore,
//! kept in a global library under the data folder and in a project library beside the document.
//!
//! * [`Library`] is one scope in memory: the `library.json` index (schema version, entries with
//!   id, kind, file name, source path, SHA-256, size, tags, licence, added time and metadata,
//!   plus the brief with its targets and rules), written atomically. Stored copies are in
//!   `files/`, pictures in `thumbs/`. [`Library::read`] reads an index without changing anything.
//! * [`Library::import`] takes files and folders (see [`import`] for the limits and the
//!   refusals), copies new content in once, classifies it by content ([`kind`]) and reads
//!   metadata ([`meta`]). It reports progress and honours cancel through [`Control`], so the app
//!   runs it as a job.
//! * [`Library::view`] gives counts per kind, the newest entries, the brief and the
//!   suggestions; [`Library::search`] filters by text and kind; [`Library::set_tags`] and
//!   [`Library::remove`] edit.
//! * [`Library::brief_json`] and [`Library::brief_markdown`] give the brief, the observed values
//!   and the entries for an agent; [`Library::write_brief_md`] keeps `brief.md` next to the index.
//! * [`thumb`] keeps the 160 px pictures: textures are reduced here, models are rendered by the
//!   app.
//!
//! Everything stays on the machine; nothing here opens a socket.

#![deny(clippy::float_cmp)]

pub mod brief;
pub mod import;
pub mod kind;
pub mod meta;
mod report;
mod store;
pub mod thumb;
mod view;

pub use brief::{
    Brief, BriefView, DisplayPreset, NameCase, Naming, Platform, Suggestion, SuggestionKind,
    TEXTURE_SIZES,
};
pub use import::{Control, ImportOptions, ImportReport, Limits, Quiet};
pub use kind::{JsonClass, Kind};
pub use meta::{AlphaUse, LoreMeta, Meta, ModelMeta, TextureMeta};
pub use report::{BRIEF_FILE, JSON_VERSION};
pub use store::{
    Entry, INDEX_FILE, Library, LibraryError, PROJECT_DIR, Query, SCHEMA, Scope, UNKNOWN_LICENCE,
    clean_tags,
};
pub use view::View;
