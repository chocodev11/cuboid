//! Model checks for the Validator panel and the export dialog, and the fixes they offer.
//!
//! A check is `cuboid_validate::validate` over a pointer copy of the document on a worker thread
//! of its own (the I/O thread is a FIFO that saves use, so a long check must not queue there).
//! The live check follows the document revision with a short debounce and runs only while the
//! panel is visible. A fix is run by `cuboid_mcp::apply_fixes`, the mapping of the `FixOp`s of an
//! issue to core commands that the agent server shares; all the fixes of a request are one batch,
//! so they are one undo step.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use cuboid_assets::mc_versions::{McVersion, PackFormat, mc_versions};
use cuboid_library::Brief;
use cuboid_mcp::{apply_fixes, fixable};
use cuboid_model::{Document, ModelKind, ObjRef, Rev};
use cuboid_ui::{
    ExportFormat, ExportReport, IssueLevel, IssueView, ValidatorRequest, ValidatorView,
    VersionChoice,
};
use cuboid_validate::{Issue, Profile, Severity, Target as CheckTarget, validate};

use crate::frontend::Frontend;
use crate::jobs::{JobCtx, JobError, JobHandle, run_isolated};
use crate::preset::{Preset, ProjectSettings};
use crate::state::{Level, State};
use crate::uimodel::node_id;

/// How long the document must stay unchanged before the live check starts.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Errors listed in the export summary; the rest are counted.
const REPORT_ISSUES: usize = 30;

/// Runs `body` on a thread of its own, with the panic and cancel handling of every job.
pub fn spawn_job<T: Send + 'static>(
    name: &str,
    body: impl FnOnce(&JobCtx) -> Result<T, JobError> + Send + 'static,
) -> JobHandle<T> {
    let (tx, rx) = mpsc::channel();
    let ctx = JobCtx::default();
    let handle = JobHandle::new(rx, ctx.clone());
    let task_tx = tx.clone();
    let spawned = thread::Builder::new().name(name.to_owned()).spawn(move || {
        // The handle may be gone already; nobody wants the result then.
        let _ = task_tx.send(run_isolated(&ctx, body));
    });
    if let Err(error) = spawned {
        let _ = tx.send(Err(JobError::Failed(format!("no worker thread: {error}"))));
    }
    handle
}

/// What a check runs with: the target, and the element budget and project rules of the brief of
/// the document's project library, if it has one. `model_name` is the file name of the document
/// without its extension.
pub fn check_profile(target: CheckTarget, brief: Option<&Brief>, model_name: &str) -> Profile {
    let mut profile = Profile::new(target);
    if let Some(brief) = brief {
        brief.fill_profile(&mut profile, model_name);
    }
    profile
}

/// The file name of a document without its extension; empty for a document that was never saved.
pub fn model_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_stem)
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
}

/// A check of a snapshot: the issues with the fixes they carry.
pub fn start_check(doc: &Document, profile: Profile) -> JobHandle<Vec<Issue>> {
    let snapshot = doc.clone();
    spawn_job("cuboid-validate", move |_| {
        Ok(validate(&snapshot, &profile))
    })
}

fn pack_format_text(format: PackFormat) -> String {
    if format.minor == 0 {
        format.major.to_string()
    } else {
        format!("{}.{}", format.major, format.minor)
    }
}

/// The game versions of the version table, newest first: both ends of every pack format range
/// and of every tier. A version without a pack format is listed without one.
pub fn version_choices() -> Vec<VersionChoice> {
    let table = mc_versions();
    let mut found: Vec<McVersion> = Vec::new();
    for entry in &table.pack_formats {
        found.extend([entry.from, entry.to]);
    }
    for tier in &table.tiers {
        found.push(tier.from);
        found.extend(tier.to);
    }
    found.sort();
    found.dedup();
    found
        .into_iter()
        .rev()
        .map(|version| VersionChoice {
            id: version.to_string(),
            pack_format: table
                .pack_format_for(version)
                .map(|entry| pack_format_text(entry.format)),
        })
        .collect()
}

/// The version an export dialog text names, the newest known one when it names none.
pub fn parse_version(text: &str) -> McVersion {
    McVersion::parse(text).unwrap_or_else(cuboid_fmt_minecraft::latest_version)
}

/// What the live check follows: the validation target of the document's preset.
pub fn live_target(project: &ProjectSettings, doc: &Document) -> (CheckTarget, String) {
    let target = project.validation_target(doc);
    let label = match target {
        CheckTarget::JavaBlock { version } => format!("Java block {version}"),
        CheckTarget::JavaItem { version } => format!("Java item {version}"),
        _ => "a Cubik file".to_owned(),
    };
    (target, label)
}

/// The game version of the preset, `None` for a Cubik document.
pub fn project_version(project: &ProjectSettings) -> Option<McVersion> {
    match project.preset {
        Preset::Minecraft { version } => Some(version),
        Preset::Cubik => None,
    }
}

/// What an export of this format is checked against; `None` for formats without a check.
pub fn export_target(
    doc: &Document,
    format: ExportFormat,
    version: McVersion,
) -> Option<(CheckTarget, String)> {
    let item = doc.props.minecraft.kind == ModelKind::Item;
    Some(match format {
        ExportFormat::Cubik => (CheckTarget::Cubik, "a Cubik file".to_owned()),
        ExportFormat::MinecraftJson if item => (
            CheckTarget::JavaItem { version },
            format!("Java item {version}"),
        ),
        ExportFormat::MinecraftJson => (
            CheckTarget::JavaBlock { version },
            format!("Java block {version}"),
        ),
        ExportFormat::ResourcePack | ExportFormat::ResourcePackFolder => (
            CheckTarget::ResourcePack { version },
            format!("a resource pack for {version}"),
        ),
        ExportFormat::Bbmodel => (
            CheckTarget::Bbmodel { java_block: false },
            "a Blockbench project".to_owned(),
        ),
        _ => return None,
    })
}

fn view_of(issue: &Issue) -> IssueView {
    IssueView {
        code: Arc::from(issue.code),
        level: match issue.severity {
            Severity::Error => IssueLevel::Error,
            Severity::Warning => IssueLevel::Warning,
            Severity::Info => IssueLevel::Info,
        },
        objects: issue
            .objects
            .iter()
            .filter_map(|&id| node_id(ObjRef::Element(id)))
            .collect(),
        message: Arc::from(issue.message.as_str()),
        hint: Arc::from(issue.hint.as_str()),
        fixable: fixable(issue),
    }
}

/// The summary the export dialog shows for the result of a check.
pub fn export_report(target: &str, issues: &[Issue]) -> ExportReport {
    let errors: Vec<&Issue> = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .collect();
    ExportReport {
        target: target.to_owned(),
        errors: errors.len(),
        warnings: issues
            .iter()
            .filter(|issue| issue.severity == Severity::Warning)
            .count(),
        fixable: errors.iter().filter(|issue| fixable(issue)).count(),
        issues: errors
            .iter()
            .take(REPORT_ISSUES)
            .map(|i| view_of(i))
            .collect(),
    }
}

/// The live check of the Validator panel.
pub struct LiveValidator {
    view: ValidatorView,
    issues: Vec<Issue>,
    /// The document revision the issues are for.
    done: Option<Rev>,
    /// The newest revision seen and when it was first seen.
    seen: Option<(Rev, Instant)>,
    run: Option<(JobHandle<Vec<Issue>>, Rev, String, Profile)>,
    label: String,
    /// The profile of the last check, so a change of the brief checks again.
    profile: Option<Profile>,
    force: bool,
    next_rev: u64,
}

impl Default for LiveValidator {
    fn default() -> Self {
        Self {
            view: ValidatorView::default(),
            issues: Vec::new(),
            done: None,
            seen: None,
            run: None,
            label: String::new(),
            profile: None,
            force: false,
            next_rev: 1,
        }
    }
}

impl LiveValidator {
    /// What the panel shows.
    pub fn view(&self) -> &ValidatorView {
        &self.view
    }

    /// Checks again at once, also when the document did not change.
    pub fn recheck(&mut self) {
        self.force = true;
    }

    /// True while a check runs or one is due.
    pub fn is_busy(&self) -> bool {
        self.run.is_some()
    }

    /// Collects a finished check and starts the next one when the document changed. `wanted` is
    /// false while the panel is hidden: nothing starts then.
    pub fn step(
        &mut self,
        doc: &Document,
        (profile, label): (Profile, String),
        now: Instant,
        wanted: bool,
    ) {
        self.collect();
        let current = doc.revision;
        if self.seen.is_none_or(|(rev, _)| rev != current) {
            self.seen = Some((current, now));
        }
        let outdated = self.done != Some(current)
            || self.label != label
            || self.profile.as_ref() != Some(&profile);
        let quiet = self
            .seen
            .is_some_and(|(_, since)| now.duration_since(since) >= DEBOUNCE);
        if wanted && self.run.is_none() && outdated && (quiet || self.force) {
            self.force = false;
            self.run = Some((start_check(doc, profile.clone()), current, label, profile));
        }
        self.view.running = self.run.is_some();
        self.view.stale = self.done != Some(current);
    }

    fn collect(&mut self) {
        let Some((handle, ..)) = self.run.as_mut() else {
            return;
        };
        let Some(result) = handle.poll() else {
            return;
        };
        let Some((_, rev, label, profile)) = self.run.take() else {
            return;
        };
        match result {
            Ok(issues) => {
                self.view.issues = issues.iter().map(view_of).collect();
                self.view.target = Arc::from(label.as_str());
                self.view.rev = self.next_rev;
                self.next_rev += 1;
                self.issues = issues;
            }
            // Without a result for this revision the check would start over every frame.
            Err(error) => log::warn!("the model check failed: {error}"),
        }
        self.done = Some(rev);
        self.label = label;
        self.profile = Some(profile);
    }

    /// The issues a request names, if the list is still the one the panel showed.
    pub fn fixes(&self, rev: u64, pick: impl Fn(usize, &Issue) -> bool) -> Vec<&Issue> {
        if rev != self.view.rev {
            return Vec::new();
        }
        self.issues
            .iter()
            .enumerate()
            .filter(|(index, issue)| fixable(issue) && pick(*index, issue))
            .map(|(_, issue)| issue)
            .collect()
    }
}

impl Frontend {
    /// What the Validator panel asks for.
    pub fn validator_request(&mut self, state: &mut State, request: ValidatorRequest) {
        let validation = &mut self.validation;
        let (rev, issues) = match &request {
            ValidatorRequest::Recheck => {
                validation.recheck();
                return;
            }
            ValidatorRequest::Fix { rev, index } => {
                (*rev, validation.fixes(*rev, |at, _| at == *index))
            }
            ValidatorRequest::FixAll { rev, code } => (
                *rev,
                validation.fixes(*rev, |_, issue| issue.code == &**code),
            ),
        };
        if validation.view().stale || issues.is_empty() {
            let text = if rev == validation.view().rev {
                "The model changed since the check. Wait for the new result."
            } else {
                "That list is out of date."
            };
            state.toast(Level::Info, text);
            return;
        }
        let summary = apply_fixes(&mut state.editor.core, &issues);
        match (summary.applied, summary.failed.first()) {
            (0, Some(why)) => state.toast(Level::Warn, format!("Cannot fix: {why}")),
            (_, Some(why)) => state.toast(Level::Warn, format!("Some fixes did not apply: {why}")),
            (0, None) => state.toast(Level::Info, "Nothing needed changing."),
            (count, None) => state.toast(
                Level::Info,
                format!(
                    "Fixed {count} edit{}. Undo reverts it in one step.",
                    if count == 1 { "" } else { "s" }
                ),
            ),
        }
        validation.recheck();
    }
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind, Target};

    use super::*;
    use crate::editor::Editor;

    fn minecraft() -> ProjectSettings {
        // A version with the classic rotation rule: two axes are illegal there.
        ProjectSettings::new(Preset::Minecraft {
            version: McVersion::new(1, 21, 4),
        })
    }

    /// A block model with a cube that sticks out of the Java range and a rotation the classic
    /// rule does not accept.
    fn editor_with_problems() -> Editor {
        let mut editor = Editor::new(1252);
        editor.project = minecraft();
        editor.core.env_mut().restrict_rotations = false;
        std::sync::Arc::make_mut(&mut editor.core.doc_for_extract().props)
            .general
            .restrict_rotations = false;
        for _ in 0..2 {
            assert!(
                editor
                    .core
                    .execute(Command::AddElement(ElementKind::Cube))
                    .is_done()
            );
        }
        let out = editor.core.execute(Command::SetPosition {
            targets: vec![Target::Element(0)],
            value: [Some(40.0), None, None],
        });
        assert!(out.is_done(), "{out:?}");
        let out = editor.core.execute(Command::SetRotation {
            targets: vec![Target::Element(1)],
            value: [None, Some(30.0), None],
        });
        assert!(out.is_done(), "{out:?}");
        editor
    }

    fn target(editor: &Editor) -> (Profile, String) {
        let (target, label) = live_target(&editor.project, editor.core.doc());
        (Profile::new(target), label)
    }

    fn check(editor: &Editor) -> Vec<Issue> {
        validate(
            editor.core.doc(),
            &Profile::new(live_target(&editor.project, editor.core.doc()).0),
        )
    }

    #[test]
    fn the_brief_of_the_project_library_fills_the_profile() {
        let brief = Brief {
            max_elements: Some(1),
            namespace: "splat".to_owned(),
            ..Brief::default()
        };
        let target = CheckTarget::JavaBlock {
            version: McVersion::new(1, 21, 4),
        };
        let profile = check_profile(target, Some(&brief), "gun");
        assert_eq!(profile.max_elements, Some(1));
        assert_eq!(profile.project.namespace, "splat");
        assert_eq!(profile.project.model_name, "gun");
        // No project library, no project rules.
        assert_eq!(check_profile(target, None, "gun"), Profile::new(target));
        assert_eq!(
            model_name(Some(Path::new("C:/work/Ink Gun.cubik"))),
            "Ink Gun"
        );
        assert_eq!(model_name(None), "");
    }

    #[test]
    fn a_changed_brief_checks_again_and_the_naming_fix_renames_the_element() {
        let mut editor = editor_with_problems();
        let rename = editor.core.execute(Command::Rename {
            targets: vec![Target::Element(0)],
            name: "Big Cube".to_owned(),
        });
        assert!(rename.is_done(), "{rename:?}");
        let (plain, label) = target(&editor);
        let mut live = LiveValidator::default();
        let start = Instant::now();
        let settle = |live: &mut LiveValidator, profile: &Profile, now: Instant| {
            live.step(
                editor.core.doc(),
                (profile.clone(), label.clone()),
                now,
                true,
            );
            let wait = Instant::now() + Duration::from_secs(30);
            while live.is_busy() {
                assert!(Instant::now() < wait, "the check did not finish");
                thread::sleep(Duration::from_millis(5));
                live.step(
                    editor.core.doc(),
                    (profile.clone(), label.clone()),
                    now,
                    true,
                );
            }
        };
        live.step(
            editor.core.doc(),
            (plain.clone(), label.clone()),
            start,
            true,
        );
        settle(&mut live, &plain, start + DEBOUNCE);
        assert!(live.view().issues.iter().all(|i| &*i.code != "PRJ_NAME"));
        // The same document under a brief that asks for snake_case is checked again.
        let mut snake = plain.clone();
        snake.project.case = cuboid_validate::NameCase::Snake;
        settle(&mut live, &snake, start + DEBOUNCE);
        let view = live.view().clone();
        assert!(
            view.issues
                .iter()
                .any(|i| &*i.code == "PRJ_NAME" && i.fixable)
        );

        // The fix renames the element, as one undo step.
        let issues = validate(editor.core.doc(), &snake);
        let named: Vec<&Issue> = issues.iter().filter(|i| i.code == "PRJ_NAME").collect();
        assert!(named.iter().all(|i| fixable(i)), "{named:?}");
        let before = editor.core.history().undo_len();
        let summary = apply_fixes(&mut editor.core, &named);
        assert!(summary.failed.is_empty(), "{:?}", summary.failed);
        assert_eq!(editor.core.history().undo_len(), before + 1);
        let labels: Vec<String> = editor
            .core
            .doc()
            .elements
            .items()
            .iter()
            .map(|el| el.base.label.to_string())
            .collect();
        assert!(labels.contains(&"big_cube".to_owned()), "{labels:?}");
        assert!(
            validate(editor.core.doc(), &snake)
                .iter()
                .all(|i| i.code != "PRJ_NAME")
        );
    }

    #[test]
    fn the_version_list_marks_what_the_table_lacks() {
        let list = version_choices();
        assert!(list.len() > 10);
        assert!(
            list.windows(2)
                .all(|pair| { McVersion::parse(&pair[0].id) > McVersion::parse(&pair[1].id) })
        );
        let newest = list.first().map(|v| v.id.clone());
        assert_eq!(
            newest.as_deref(),
            Some(cuboid_fmt_minecraft::latest_version().to_string().as_str())
        );
        assert!(
            list.iter()
                .any(|v| v.id == "1.21.4" && v.pack_format.as_deref() == Some("46"))
        );
    }

    #[test]
    fn issues_become_views_with_nodes_and_fixability() {
        let editor = editor_with_problems();
        let issues = check(&editor);
        let bounds = issues
            .iter()
            .find(|issue| issue.code == cuboid_validate::GEO_BOUNDS)
            .expect("the cube outside the range is reported");
        let view = view_of(bounds);
        assert_eq!(view.level, IssueLevel::Error);
        assert!(view.fixable);
        let id = editor.core.doc().elements.ids()[0];
        assert_eq!(view.objects, vec![node_id(ObjRef::Element(id)).unwrap()]);
        let report = export_report("Java block", &issues);
        assert!(report.errors >= 1);
        assert!(report.fixable >= 1);
        assert_eq!(report.issues.len(), report.errors);
    }

    #[test]
    fn a_fix_is_one_undo_step_and_removes_the_issue() {
        let mut editor = editor_with_problems();
        let before_steps = editor.core.history().undo_len();
        let issues = check(&editor);
        let chosen: Vec<&Issue> = issues.iter().filter(|i| fixable(i)).collect();
        assert!(chosen.len() >= 2, "bounds and rotation are both fixable");
        let summary = apply_fixes(&mut editor.core, &chosen);
        assert!(summary.failed.is_empty(), "{:?}", summary.failed);
        assert!(summary.applied >= 2);
        assert_eq!(
            editor.core.history().undo_len(),
            before_steps + 1,
            "all fixes share one undo step"
        );
        let after = check(&editor);
        assert!(
            after.iter().all(|i| i.code != cuboid_validate::GEO_BOUNDS
                && i.code != cuboid_validate::GEO_ROTATION),
            "{after:?}"
        );
        assert!(editor.core.execute(Command::Undo).is_done());
        let again = check(&editor);
        assert!(again.iter().any(|i| i.code == cuboid_validate::GEO_BOUNDS));
        assert!(
            again
                .iter()
                .any(|i| i.code == cuboid_validate::GEO_ROTATION)
        );
    }

    #[test]
    fn a_fix_that_removes_a_texture_shares_the_undo_step_of_the_other_fixes() {
        let mut editor = editor_with_problems();
        let made = editor.core.execute(Command::CreateTexture {
            label: Some("spare".to_owned()),
            width: 16,
            height: 16,
            color: [10, 20, 30, 255],
        });
        assert!(made.is_done(), "{made:?}");
        let textures = editor.core.doc().textures.entries.len();
        let before = editor.core.history().undo_len();
        let issues = check(&editor);
        let chosen: Vec<&Issue> = issues
            .iter()
            .filter(|i| i.code == cuboid_validate::GEO_BOUNDS || i.code == "TEX_UNUSED")
            .collect();
        assert!(chosen.iter().any(|i| i.code == "TEX_UNUSED"), "{issues:?}");
        assert!(chosen.iter().any(|i| i.code == cuboid_validate::GEO_BOUNDS));
        let summary = apply_fixes(&mut editor.core, &chosen);
        assert!(summary.failed.is_empty(), "{:?}", summary.failed);
        assert_eq!(editor.core.doc().textures.entries.len(), textures - 1);
        assert_eq!(
            editor.core.history().undo_len(),
            before + 1,
            "the texture and the cube are one step"
        );
        assert!(editor.core.execute(Command::Undo).is_done());
        assert_eq!(editor.core.doc().textures.entries.len(), textures);
        assert!(
            check(&editor)
                .iter()
                .any(|i| i.code == cuboid_validate::GEO_BOUNDS)
        );
    }

    #[test]
    fn the_live_check_waits_for_a_quiet_document_and_ignores_stale_clicks() {
        let editor = editor_with_problems();
        let mut live = LiveValidator::default();
        let start = Instant::now();
        live.step(editor.core.doc(), target(&editor), start, true);
        assert!(!live.is_busy(), "the debounce has not passed");
        assert!(live.view().stale);
        live.step(editor.core.doc(), target(&editor), start + DEBOUNCE, false);
        assert!(!live.is_busy(), "a hidden panel runs nothing");
        live.step(editor.core.doc(), target(&editor), start + DEBOUNCE, true);
        assert!(live.is_busy());
        let end = Instant::now() + Duration::from_secs(30);
        while live.is_busy() {
            assert!(Instant::now() < end, "the check did not finish");
            thread::sleep(Duration::from_millis(5));
            live.step(editor.core.doc(), target(&editor), start + DEBOUNCE, true);
        }
        let view = live.view().clone();
        assert!(!view.stale);
        assert!(view.count(IssueLevel::Error) >= 2);
        assert!(view.badge().is_some());
        assert_eq!(
            live.fixes(view.rev, |_, _| true).len(),
            view.issues.iter().filter(|i| i.fixable).count()
        );
        assert!(live.fixes(view.rev + 1, |_, _| true).is_empty());
    }
}
