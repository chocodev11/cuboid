//! The self test: every rule ships a defect injector, and the test proves on a given document
//! that each rule still fires.

use cuboid_model::Document;

use crate::issue::Severity;
use crate::profile::Profile;
use crate::rules::RULES;
use crate::validate;

/// What happened to one rule in the self test.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The rule reported the injected defect.
    Fired,
    /// The rule does not apply to this target, or is switched off.
    NotApplicable,
    /// The rule stayed silent about its own defect: it is broken.
    Silent,
}

/// The result for one rule.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RuleResult {
    /// The rule's code.
    pub code: &'static str,
    /// What happened.
    pub outcome: Outcome,
}

/// The result for all rules.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelfTestReport {
    /// One entry per rule, in registry order.
    pub results: Vec<RuleResult>,
}

impl SelfTestReport {
    /// Whether no rule stayed silent.
    pub fn ok(&self) -> bool {
        self.results.iter().all(|r| r.outcome != Outcome::Silent)
    }

    /// The codes of the rules that stayed silent.
    pub fn silent(&self) -> Vec<&'static str> {
        self.results
            .iter()
            .filter(|r| r.outcome == Outcome::Silent)
            .map(|r| r.code)
            .collect()
    }

    /// The number of rules that fired.
    pub fn fired(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.outcome == Outcome::Fired)
            .count()
    }
}

/// Injects each rule's defect into a copy of `doc` and checks that `validate` reports it with
/// the rule's code. The document is not changed. The profile selects the target; its budgets
/// and pack data are replaced where the defect needs it.
pub fn validate_self_test(doc: &Document, profile: &Profile) -> SelfTestReport {
    let params = profile.params();
    let mut report = SelfTestReport::default();
    for rule in RULES {
        let off = profile.disabled.iter().any(|code| code == rule.code);
        if off || !(rule.applies)(&params) {
            report.results.push(RuleResult {
                code: rule.code,
                outcome: Outcome::NotApplicable,
            });
            continue;
        }
        let mut solo = profile.clone();
        solo.only = vec![rule.code];
        solo.max_issues_per_rule = usize::MAX;
        solo.min_severity = Severity::Info;
        let before = validate(doc, &solo);

        let mut injected_doc = doc.clone();
        let mut injected = solo.clone();
        (rule.inject)(&mut injected_doc, &mut injected);
        injected.only = vec![rule.code];
        let after = validate(&injected_doc, &injected);

        // A defect that was already there proves the rule too; a new one proves the injector.
        let new = after.iter().any(|issue| !before.contains(issue));
        let fired = new || !before.is_empty();
        report.results.push(RuleResult {
            code: rule.code,
            outcome: if fired {
                Outcome::Fired
            } else {
                Outcome::Silent
            },
        });
    }
    report
}
