//! The Validator panel: the findings of the model checker as a list, with fixes.
//!
//! The app runs the checker on a worker over a snapshot of the document and hands the result in
//! through [`ValidatorView`]; this file only draws it and turns clicks into requests. Selecting
//! the objects of an issue is an ordinary [`Edit::Select`]; a fix is a [`ValidatorRequest`] the
//! app answers with core commands.

use std::sync::Arc;

use cuboid_ui_kit::{Icon, RADIUS, Tokens};
use egui::{Align, Color32, FontId, Layout, RichText, Sense, Ui, vec2};

use crate::common::Sink;
use crate::model::NodeId;
use crate::request::{Edit, SelectOp, UiRequest};

/// How bad an issue is.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IssueLevel {
    /// The target cannot take the model as it is.
    Error,
    /// The target changes or mis-renders the model.
    Warning,
    /// A fact, not a defect.
    Info,
}

impl IssueLevel {
    /// Worst first.
    pub const ALL: [IssueLevel; 3] = [IssueLevel::Error, IssueLevel::Warning, IssueLevel::Info];

    /// Position in [`IssueLevel::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Group title.
    pub fn title(self) -> &'static str {
        match self {
            IssueLevel::Error => "Errors",
            IssueLevel::Warning => "Warnings",
            IssueLevel::Info => "Info",
        }
    }

    fn icon(self) -> Icon {
        match self {
            IssueLevel::Error => Icon::Error,
            IssueLevel::Warning => Icon::Warning,
            IssueLevel::Info => Icon::Info,
        }
    }

    fn color(self, tokens: &Tokens) -> Color32 {
        match self {
            IssueLevel::Error => tokens.danger,
            IssueLevel::Warning => tokens.warn,
            IssueLevel::Info => tokens.accent,
        }
    }
}

/// One finding of the checker.
#[derive(Clone, Debug, PartialEq)]
pub struct IssueView {
    /// Stable code such as `GEO_ROTATION`.
    pub code: Arc<str>,
    /// How bad it is.
    pub level: IssueLevel,
    /// The objects concerned; a click selects them.
    pub objects: Vec<NodeId>,
    /// What is wrong.
    pub message: Arc<str>,
    /// How to remove it.
    pub hint: Arc<str>,
    /// The app can fix it with core commands.
    pub fixable: bool,
}

/// The result of the last check, as the panel shows it.
#[derive(Clone, Debug, Default)]
pub struct ValidatorView {
    /// What the model is checked for, such as "Java block 1.21.11".
    pub target: Arc<str>,
    /// The findings, worst first.
    pub issues: Arc<[IssueView]>,
    /// Identifies this result; fix requests echo it so the app can tell a stale click.
    pub rev: u64,
    /// A check is running.
    pub running: bool,
    /// The model changed after this result was made: the list may be out of date.
    pub stale: bool,
}

impl ValidatorView {
    /// How many issues have this level.
    pub fn count(&self, level: IssueLevel) -> usize {
        self.issues.iter().filter(|i| i.level == level).count()
    }

    /// The tab title: the panel name with the error and warning counts.
    pub fn tab_title(&self) -> String {
        let (errors, warnings) = (
            self.count(IssueLevel::Error),
            self.count(IssueLevel::Warning),
        );
        match (errors, warnings) {
            (0, 0) => "Validator".to_owned(),
            (e, 0) => format!("Validator {e}E"),
            (0, w) => format!("Validator {w}W"),
            (e, w) => format!("Validator {e}E {w}W"),
        }
    }
}

/// What the panel asks the app to do about issues.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidatorRequest {
    /// Apply the fix of one issue as one undo step.
    Fix {
        /// The `rev` of the list the index belongs to.
        rev: u64,
        /// Position in [`ValidatorView::issues`].
        index: usize,
    },
    /// Apply the fixes of every fixable issue with this code as one undo step.
    FixAll {
        /// The `rev` of the list.
        rev: u64,
        /// The code.
        code: Arc<str>,
    },
    /// Check again now.
    Recheck,
}

/// The state of the panel: which severities are listed.
pub(crate) struct ValidatorState {
    shown: [bool; 3],
}

impl Default for ValidatorState {
    fn default() -> Self {
        Self { shown: [true; 3] }
    }
}

impl ValidatorState {
    pub fn show(&mut self, ui: &mut Ui, view: Option<&ValidatorView>, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        let Some(view) = view else {
            ui.label(RichText::new("The model is not checked yet.").color(tokens.text_weak));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for level in IssueLevel::ALL {
                let text = format!("{} {}", level.title(), view.count(level));
                ui.toggle_value(&mut self.shown[level.index()], text);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Recheck").clicked() {
                    sink.push(UiRequest::Validator(ValidatorRequest::Recheck));
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("For {}", view.target)).color(tokens.text_weak));
            if view.running {
                ui.add(egui::Spinner::new().size(12.0));
            } else if view.stale {
                ui.label(RichText::new("out of date").color(tokens.text_weak));
            }
        });
        ui.add_space(4.0);

        if view.issues.is_empty() {
            if !view.running {
                ui.label(RichText::new("No issues found.").color(tokens.ok));
            }
            return;
        }
        let fixable_per_code = |code: &str| {
            view.issues
                .iter()
                .filter(|i| i.fixable && &*i.code == code)
                .count()
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for level in IssueLevel::ALL {
                    if !self.shown[level.index()] {
                        continue;
                    }
                    for (index, issue) in view.issues.iter().enumerate() {
                        if issue.level != level {
                            continue;
                        }
                        let same_kind = fixable_per_code(&issue.code);
                        row(ui, &tokens, view, index, issue, same_kind, sink);
                    }
                }
            });
    }
}

fn row(
    ui: &mut Ui,
    tokens: &Tokens,
    view: &ValidatorView,
    index: usize,
    issue: &IssueView,
    same_kind: usize,
    sink: &mut Sink,
) {
    let color = issue.level.color(tokens);
    egui::Frame::new()
        .fill(tokens.bg_raised)
        .stroke(egui::Stroke::new(1.0, tokens.border))
        .corner_radius(RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut clicked = false;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                issue.level.icon().paint(ui.painter(), rect, color);
                let code = ui.add(
                    egui::Label::new(
                        RichText::new(&*issue.code)
                            .font(FontId::monospace(11.5))
                            .color(tokens.text_weak),
                    )
                    .sense(Sense::click()),
                );
                clicked |= code.clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if issue.fixable {
                        let enabled = !view.stale && !view.running;
                        let fix = ui.add_enabled(enabled, egui::Button::new("Fix"));
                        if fix.clicked() {
                            sink.push(UiRequest::Validator(ValidatorRequest::Fix {
                                rev: view.rev,
                                index,
                            }));
                        }
                        if same_kind > 1 {
                            let all = ui.add_enabled(
                                enabled,
                                egui::Button::new(format!("Fix all {same_kind}")),
                            );
                            if all.clicked() {
                                sink.push(UiRequest::Validator(ValidatorRequest::FixAll {
                                    rev: view.rev,
                                    code: issue.code.clone(),
                                }));
                            }
                        }
                    }
                });
            });
            let message = ui.add(
                egui::Label::new(RichText::new(&*issue.message).color(tokens.text)).sense(
                    if issue.objects.is_empty() {
                        Sense::hover()
                    } else {
                        Sense::click()
                    },
                ),
            );
            clicked |= message.clicked();
            if !issue.hint.is_empty() {
                ui.label(
                    RichText::new(&*issue.hint)
                        .size(12.0)
                        .color(tokens.text_weak),
                );
            }
            if clicked && !issue.objects.is_empty() {
                sink.edit(Edit::Select {
                    ids: issue.objects.clone(),
                    op: SelectOp::Replace,
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use egui::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn issue(code: &str, level: IssueLevel, fixable: bool, objects: &[u64]) -> IssueView {
        IssueView {
            code: Arc::from(code),
            level,
            objects: objects.iter().copied().map(NodeId).collect(),
            message: Arc::from(format!("{code} message")),
            hint: Arc::from("a hint"),
            fixable,
        }
    }

    fn view(issues: Vec<IssueView>) -> ValidatorView {
        ValidatorView {
            target: Arc::from("Java block 1.21.11"),
            issues: Arc::from(issues),
            rev: 7,
            running: false,
            stale: false,
        }
    }

    type Rig = (ValidatorState, Option<ValidatorView>, Sink);

    fn harness(view: Option<ValidatorView>) -> Harness<'static, Rig> {
        let mut harness = Harness::builder()
            .with_size(vec2(420.0, 480.0))
            .allow_missing_glyphs()
            .build_ui_state(
                |ui: &mut Ui, (state, view, sink): &mut Rig| state.show(ui, view.as_ref(), sink),
                (ValidatorState::default(), view, Sink::default()),
            );
        harness.run_steps(3);
        harness
    }

    fn requests(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
        harness.run_steps(3);
        std::mem::take(&mut harness.state_mut().2.requests)
    }

    #[test]
    fn the_tab_title_counts_errors_and_warnings() {
        assert_eq!(ValidatorView::default().tab_title(), "Validator");
        let v = view(vec![
            issue("A", IssueLevel::Error, false, &[]),
            issue("B", IssueLevel::Error, false, &[]),
            issue("C", IssueLevel::Warning, false, &[]),
            issue("D", IssueLevel::Info, false, &[]),
        ]);
        assert_eq!(v.tab_title(), "Validator 2E 1W");
        assert_eq!(v.count(IssueLevel::Info), 1);
        let only_warnings = view(vec![issue("C", IssueLevel::Warning, false, &[])]);
        assert_eq!(only_warnings.tab_title(), "Validator 1W");
    }

    fn two_rotation_errors() -> ValidatorView {
        view(vec![
            issue("GEO_ROTATION", IssueLevel::Error, true, &[4]),
            issue("GEO_ROTATION", IssueLevel::Error, true, &[6]),
            issue("TEX_UNUSED", IssueLevel::Info, false, &[]),
        ])
    }

    #[test]
    fn drawing_a_list_sends_nothing_by_itself() {
        let mut h = harness(Some(two_rotation_errors()));
        assert!(requests(&mut h).is_empty());
        let mut none = harness(None);
        assert!(requests(&mut none).is_empty());
        none.get_by_label_contains("not checked yet");
    }

    #[test]
    fn fix_and_fix_all_name_the_list_and_the_issue() {
        let mut h = harness(Some(two_rotation_errors()));
        h.get_all_by_role_and_label(Role::Button, "Fix")
            .nth(1)
            .expect("two Fix buttons")
            .click();
        assert_eq!(
            requests(&mut h),
            [UiRequest::Validator(ValidatorRequest::Fix {
                rev: 7,
                index: 1
            })]
        );
        h.get_all_by_role_and_label(Role::Button, "Fix all 2")
            .next()
            .expect("a Fix all button")
            .click();
        assert_eq!(
            requests(&mut h),
            [UiRequest::Validator(ValidatorRequest::FixAll {
                rev: 7,
                code: Arc::from("GEO_ROTATION"),
            })]
        );
        h.get_by_role_and_label(Role::Button, "Recheck").click();
        assert_eq!(
            requests(&mut h),
            [UiRequest::Validator(ValidatorRequest::Recheck)]
        );
    }

    #[test]
    fn a_stale_list_cannot_be_fixed() {
        let mut stale = two_rotation_errors();
        stale.stale = true;
        let mut h = harness(Some(stale));
        h.get_all_by_role_and_label(Role::Button, "Fix")
            .next()
            .expect("a Fix button")
            .click();
        assert!(requests(&mut h).is_empty());
    }

    #[test]
    fn clicking_an_issue_selects_its_objects() {
        let mut h = harness(Some(two_rotation_errors()));
        h.get_all_by_label("GEO_ROTATION message")
            .nth(1)
            .expect("two messages")
            .click();
        assert_eq!(
            requests(&mut h),
            [UiRequest::Edit(Edit::Select {
                ids: vec![NodeId(6)],
                op: SelectOp::Replace,
            })]
        );
    }

    #[test]
    fn the_severity_filter_hides_a_level() {
        let mut h = harness(Some(two_rotation_errors()));
        h.get_by_label("TEX_UNUSED message");
        h.get_by_label("Info 1").click();
        h.run_steps(3);
        assert!(h.query_by_label("TEX_UNUSED message").is_none());
        h.get_all_by_label("GEO_ROTATION message")
            .next()
            .expect("errors stay");
    }
}
