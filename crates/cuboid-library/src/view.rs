//! The summary of a library for the start screen and the library panel.

use crate::brief::{BriefView, Suggestion, brief_view, suggestions};
use crate::kind::Kind;
use crate::store::{Entry, Library, Scope};

/// Counts per kind, the newest entries, the brief and the suggestions.
#[derive(Clone, Debug)]
pub struct View {
    /// Which library.
    pub scope: Scope,
    /// Entries per kind, in [`Kind::ALL`] order.
    pub counts: [usize; 5],
    /// Entries in total.
    pub total: usize,
    /// The newest entries, newest first.
    pub recent: Vec<Entry>,
    /// Tags in use, most used first.
    pub tags: Vec<(String, usize)>,
    /// The brief.
    pub brief: BriefView,
    /// The hints.
    pub suggestions: Vec<Suggestion>,
}

impl View {
    /// Entries of one kind.
    pub fn count(&self, kind: Kind) -> usize {
        self.counts.get(kind.index()).copied().unwrap_or_default()
    }
}

impl Library {
    /// The summary, with the `recent` newest entries. Reads only the index in memory.
    pub fn view(&self, recent: usize) -> View {
        let entries = self.entries();
        View {
            scope: self.scope(),
            counts: self.counts(),
            total: entries.len(),
            recent: entries.iter().rev().take(recent).cloned().collect(),
            tags: self.tags(),
            brief: brief_view(self.brief(), entries),
            suggestions: suggestions(self.brief(), entries),
        }
    }
}
