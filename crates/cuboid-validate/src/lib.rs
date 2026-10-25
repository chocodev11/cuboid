#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
//! Layer L3. The model validator: a pure function from a document snapshot and a [`Profile`] to
//! a list of [`Issue`]s, with stable codes and fixes as plain [`FixOp`] data.
//!
//! Rules for the Java block and item targets follow `formats_interop` 2.2.2, 7.2 to 7.7 and
//! 31.2 to 31.4; the per-version facts come from the table in `cuboid-assets`. The Cubik target
//! reports only what a `.cubik` file cannot store. Every rule has a defect injector, so
//! [`validate_self_test`] proves on a given document that the rule still fires.
//!
//! Must not edit a document, perform I/O or know a file format.

mod codes;
mod ctx;
mod density;
mod elements;
mod issue;
mod model_rules;
mod pack;
mod profile;
mod project;
mod resource_id;
mod rules;
mod selftest;
mod textures;
mod zfight;

#[cfg(test)]
mod tests;

pub use codes::*;
pub use cuboid_assets::mc_versions::{McVersion, PackFormat};
pub use issue::{FixOp, Issue, Severity};
pub use profile::{
    NameCase, PackInfo, PackMeta, PackModel, Profile, ProjectRules, RenderType, Target,
};
pub use selftest::{Outcome, RuleResult, SelfTestReport, validate_self_test};

use cuboid_model::Document;

use ctx::{Ctx, Scan};

/// A rule of the validator, for the UI list of checks.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RuleInfo {
    /// The code its issues carry.
    pub code: &'static str,
    /// One line on what it checks.
    pub summary: &'static str,
    /// The worst severity it reports.
    pub max_severity: Severity,
}

/// Every rule, in the order of the code list.
pub fn rules() -> Vec<RuleInfo> {
    rules::RULES
        .iter()
        .map(|rule| RuleInfo {
            code: rule.code,
            summary: rule.summary,
            max_severity: rule.max,
        })
        .collect()
}

/// Checks a document against a profile. Pure: the document is not changed and nothing is read
/// but the document and the embedded version table. Issues come worst first.
pub fn validate(doc: &Document, profile: &Profile) -> Vec<Issue> {
    let mut ctx = Ctx::new(profile);
    let mut scan = Scan::new(doc);
    model_rules::run(&mut ctx, doc, &scan, profile);
    let found = elements::run(&mut ctx, doc, &mut scan, profile);
    zfight::run(&mut ctx, &scan.elements, &found.quads);
    density::run(
        &mut ctx,
        &scan.elements,
        &found.densities,
        profile.texel_ratio,
    );
    textures::run(&mut ctx, doc, &scan, profile);
    pack::run(&mut ctx, doc, profile);
    project::run(&mut ctx, doc, &scan, profile);
    ctx.finish()
}

/// The rules that decide whether a model can be written in a target at all.
const FIT_RULES: [&str; 6] = [
    NUM_NONFINITE,
    GEO_BOUNDS,
    GEO_ROTATION,
    UV_RANGE,
    TEX_FACE_NONE,
    TEX_UNRESOLVED,
];

/// Whether the model fits the target as it is: no error or warning from the rules about numbers,
/// bounds, rotation, UV and texture references. An exporter can call this to choose between the
/// `java_block` and the free `.bbmodel` format: `fits(doc, &Target::Bbmodel { java_block: true })`.
pub fn fits(doc: &Document, target: &Target) -> bool {
    let mut profile = Profile::new(*target);
    profile.min_severity = Severity::Warning;
    profile.only = FIT_RULES.to_vec();
    validate(doc, &profile).is_empty()
}
