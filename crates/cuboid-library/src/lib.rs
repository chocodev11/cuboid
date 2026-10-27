#![forbid(unsafe_code)]
//! Layer L5. The asset library of the start screen: models, textures, JSON, references and lore,
//! kept in a global library under the data folder and in a project library beside the document.
//!
//! * [`Library`] is one scope in memory: the `library.json` index (schema version, entries with
//!   id, kind, role, file name, source path, SHA-256, size, tags, licence, added time and
//!   metadata, plus the brief with its targets, rules and style folders), written atomically.
//!   Stored copies are in `files/`, pictures in `thumbs/`. [`Library::read`] reads an index
//!   without changing anything.
//! * [`Library::import`] takes files and folders (see [`import`] for the limits and the
//!   refusals), copies new content in once, classifies it by content ([`kind`]) and reads
//!   metadata ([`meta`]). It reports progress and honours cancel through [`Control`], so the app
//!   runs it as a job.
//! * [`Library::import_style`], [`Library::rescan_style`] and [`Library::remove_style_source`]
//!   keep folders of reference models and textures (the role [`Role::Style`]); the summary of
//!   what they have in common is [`Library::style_summary`].
//! * [`Library::view`] gives counts per kind, the newest entries, the brief and the
//!   suggestions; [`Library::search`] filters by text and kind; [`Library::set_tags`],
//!   [`Library::set_role`] and [`Library::remove`] edit.
//! * [`Library::brief_json`] and [`Library::brief_markdown`] give the brief, the observed values
//!   and the entries for an agent; [`Library::write_brief_md`] keeps `brief.md` next to the index.
//! * [`doc::load`], [`check::check`] and [`outline()`] open a model file, validate it against
//!   the brief and read it as data, for `cuboid-cli` and the agent toolkit.
//! * [`thumb`] keeps the 160 px pictures: textures are reduced here, models are rendered by the
//!   app.
//!
//! Everything stays on the machine; nothing here opens a socket.

#![deny(clippy::float_cmp)]

pub mod brief;
pub mod check;
pub mod doc;
pub mod import;
pub mod kind;
pub mod meta;
mod outline;
mod report;
mod store;
mod style;
pub mod thumb;
mod view;

pub use brief::{
    Brief, BriefView, DisplayPreset, NameCase, Naming, Platform, StyleSource, Suggestion,
    SuggestionKind, TEXTURE_SIZES,
};
pub use import::{Control, ImportOptions, ImportReport, Limits, Quiet};
pub use kind::{JsonClass, Kind, Role};
pub use meta::{AlphaUse, LoreMeta, Meta, ModelMeta, StyleMeta, TextureMeta};
pub use outline::{
    OUTLINE_PAGE, Outline, OutlineDisplay, OutlineElement, OutlineFace, OutlineFaces, OutlineGroup,
    OutlineTexture, outline,
};
pub use report::{BRIEF_FILE, JSON_VERSION};
pub use store::{
    Entry, INDEX_FILE, Library, LibraryError, PROJECT_DIR, Query, SCHEMA, Scope, UNKNOWN_LICENCE,
    clean_tags,
};
pub use style::{
    DensitySummary, ElementSize, GroupSummary, NamingSummary, RotationSummary, Spread,
    StyleSummary, style_summary, within,
};
pub use view::View;
