//! The state of one run: which rules are on, the issue list, the per-rule cap, and the elements
//! that the target would write.

use cuboid_model::{Document, Element, ObjId};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::issue::{Issue, Severity};
use crate::profile::{Params, Profile};
use crate::rules::RULES;

pub(crate) struct Ctx {
    pub params: Params,
    enabled: FxHashSet<&'static str>,
    cap: usize,
    min: Severity,
    counts: FxHashMap<&'static str, usize>,
    issues: Vec<Issue>,
}

impl Ctx {
    pub fn new(profile: &Profile) -> Self {
        let params = profile.params();
        let enabled = RULES
            .iter()
            .filter(|rule| {
                (rule.applies)(&params)
                    && rule.max.rank() >= profile.min_severity.rank()
                    && !profile.disabled.iter().any(|code| code == rule.code)
                    && (profile.only.is_empty() || profile.only.contains(&rule.code))
            })
            .map(|rule| rule.code)
            .collect();
        Self {
            params,
            enabled,
            cap: profile.max_issues_per_rule,
            min: profile.min_severity,
            counts: FxHashMap::default(),
            issues: Vec::new(),
        }
    }

    /// Whether the rule applies to the target and is not switched off.
    pub fn on(&self, code: &str) -> bool {
        self.enabled.contains(code)
    }

    /// Records an issue of `code` unless the rule is off or has reached its cap. The issue is
    /// only built when it is kept.
    pub fn emit(&mut self, code: &'static str, build: impl FnOnce() -> Issue) {
        if !self.on(code) {
            return;
        }
        let count = self.counts.entry(code).or_insert(0);
        *count += 1;
        if *count > self.cap {
            return;
        }
        let issue = build();
        if issue.severity.rank() >= self.min.rank() {
            self.issues.push(issue);
        }
    }

    /// The issues, worst first, with one summary per capped code.
    pub fn finish(mut self) -> Vec<Issue> {
        let mut capped: Vec<(&'static str, usize)> = self
            .counts
            .iter()
            .filter(|&(_, &n)| n > self.cap)
            .map(|(&code, &n)| (code, n - self.cap))
            .collect();
        capped.sort_unstable();
        if self.min == Severity::Info {
            for (code, more) in capped {
                self.issues.push(Issue::new(
                    code,
                    Severity::Info,
                    Vec::new(),
                    format!("{more} more {code} issues are not listed"),
                    "Raise Profile::max_issues_per_rule to see them",
                ));
            }
        }
        self.issues
            .sort_by_key(|issue| std::cmp::Reverse(issue.severity.rank()));
        self.issues
    }
}

/// An element the target writes.
pub(crate) struct Exported<'a> {
    pub id: ObjId,
    /// The number the list panel shows.
    pub no: u32,
    pub el: &'a Element,
}

impl Exported<'_> {
    /// `label #no`, as the export dialogs name an element (interop 7.2).
    pub fn name(&self) -> String {
        format!("\"{}\" #{}", self.el.base.label, self.no)
    }
}

/// The elements and the texture index, built once per run.
pub(crate) struct Scan<'a> {
    pub elements: Vec<Exported<'a>>,
    /// Texture label to position in the list; the first entry wins for a repeated label.
    pub tex_index: FxHashMap<&'a str, usize>,
    /// Which textures an enabled face of an exported element names.
    pub used: Vec<bool>,
}

impl<'a> Scan<'a> {
    pub fn new(doc: &'a Document) -> Self {
        let general = &doc.props.general;
        let elements = if general.exclude_elements {
            Vec::new()
        } else {
            let lists = doc
                .elements
                .ids()
                .iter()
                .zip(doc.elements.items())
                .zip(doc.elements.display_ids());
            lists
                .filter(|((_, el), _)| !(general.exclude_hidden && el.base.hidden))
                .map(|((&id, el), &no)| Exported { id, no, el })
                .collect()
        };
        let mut tex_index = FxHashMap::default();
        for (at, entry) in doc.textures.entries.iter().enumerate() {
            tex_index.entry(&*entry.label).or_insert(at);
        }
        Self {
            elements,
            tex_index,
            used: vec![false; doc.textures.entries.len()],
        }
    }
}
