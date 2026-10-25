//! The Validator panel: the findings of the model checker as a list, with fixes.
//!
//! The app runs the checker on a worker over a snapshot of the document and hands the result in
//! through [`ValidatorView`]; this file only draws it and turns clicks into requests. Selecting
//! the objects of an issue is an ordinary [`Edit::Select`]; a fix is a [`ValidatorRequest`] the
//! app answers with core commands.

use std::collections::HashMap;
use std::sync::Arc;

use cuboid_ui_kit::metrics::{height, radius, space};
use cuboid_ui_kit::{Button, EmptyState, Icon, Tokens, Tone, Type, badge, chip, progress, text};
use egui::{Align, Label, Layout, ScrollArea, Sense, Ui, UiBuilder, vec2};

use crate::common::Sink;
use crate::model::{NodeId, NodeView};
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

    /// The word on the badge of one issue.
    fn name(self) -> &'static str {
        match self {
            IssueLevel::Error => "Error",
            IssueLevel::Warning => "Warning",
            IssueLevel::Info => "Info",
        }
    }

    fn tone(self) -> Tone {
        match self {
            IssueLevel::Error => Tone::Danger,
            IssueLevel::Warning => Tone::Warning,
            IssueLevel::Info => Tone::Neutral,
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

    /// The count on the tab icon: the errors, or the warnings while there are no errors.
    pub fn badge(&self) -> Option<(usize, Tone)> {
        [IssueLevel::Error, IssueLevel::Warning]
            .into_iter()
            .map(|level| (self.count(level), level.tone()))
            .find(|(count, _)| *count > 0)
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

/// The list as the filter shows it, rebuilt when the result or the filter changes.
struct Listed {
    rev: u64,
    issues: usize,
    shown: [bool; 3],
    /// Positions in the issues, by level, worst first.
    order: Vec<usize>,
    /// How many fixable issues each code has.
    fixable: HashMap<Arc<str>, usize>,
}

/// The state of the panel: which severities are listed and what is derived from the result.
pub(crate) struct ValidatorState {
    shown: [bool; 3],
    listed: Option<Listed>,
    /// The names of the nodes, by id, for the object of an issue.
    names: HashMap<NodeId, Arc<str>>,
    names_rev: Option<u64>,
}

impl Default for ValidatorState {
    fn default() -> Self {
        Self {
            shown: [true; 3],
            listed: None,
            names: HashMap::new(),
            names_rev: None,
        }
    }
}

impl ValidatorState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        view: Option<&ValidatorView>,
        nodes: &[NodeView],
        nodes_rev: u64,
        sink: &mut Sink,
    ) {
        let Some(view) = view else {
            EmptyState::new(Icon::Validator, "The model is not checked yet")
                .body("The check runs while this tab is open.")
                .show(ui);
            return;
        };
        self.header(ui, view, sink);
        if view.issues.is_empty() {
            if view.running {
                return;
            }
            EmptyState::new(Icon::CheckCircle, "No issues found")
                .body("The model fits its target.")
                .show(ui);
            return;
        }
        if self.names_rev != Some(nodes_rev) {
            self.names_rev = Some(nodes_rev);
            self.names = nodes.iter().map(|n| (n.id, n.name.clone())).collect();
        }
        let stale = self.listed.as_ref().is_none_or(|l| {
            l.rev != view.rev || l.issues != view.issues.len() || l.shown != self.shown
        });
        if stale {
            self.listed = Some(self.list(view));
        }
        let Some(listed) = self.listed.as_ref() else {
            return;
        };
        if listed.order.is_empty() {
            ui.label(text(Type::Label, "No issue has a shown severity."));
            return;
        }
        let row_height = row_height();
        ui.spacing_mut().item_spacing.y = space::XS;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, listed.order.len(), |ui, range| {
                for &index in &listed.order[range] {
                    let Some(issue) = view.issues.get(index) else {
                        continue;
                    };
                    let same_kind = listed.fixable.get(&issue.code).copied().unwrap_or(0);
                    row(
                        ui,
                        row_height,
                        view,
                        issue,
                        index,
                        same_kind,
                        &self.names,
                        sink,
                    );
                }
            });
    }

    /// The severity filter, the recheck button and what the result is for.
    fn header(&mut self, ui: &mut Ui, view: &ValidatorView, sink: &mut Sink) {
        ui.horizontal_wrapped(|ui| {
            for level in IssueLevel::ALL {
                let label = format!("{} {}", level.title(), view.count(level));
                if chip(ui, &label, self.shown[level.index()]).clicked() {
                    self.shown[level.index()] = !self.shown[level.index()];
                }
            }
        });
        ui.add_space(space::XS);
        ui.horizontal(|ui| {
            if ui.add(Button::new("Recheck").icon(Icon::Reload)).clicked() {
                sink.push(UiRequest::Validator(ValidatorRequest::Recheck));
            }
            if view.running {
                progress(ui, None, height::ACTION * 2.0);
            } else if view.stale {
                ui.label(text(Type::Caption, "out of date"));
            }
            ui.add(Label::new(text(Type::Caption, format!("For {}", view.target))).truncate());
        });
        ui.add_space(space::SM);
    }

    fn list(&self, view: &ValidatorView) -> Listed {
        let mut order = Vec::with_capacity(view.issues.len());
        for level in IssueLevel::ALL {
            if !self.shown[level.index()] {
                continue;
            }
            order.extend(
                view.issues
                    .iter()
                    .enumerate()
                    .filter(|(_, issue)| issue.level == level)
                    .map(|(index, _)| index),
            );
        }
        let mut fixable: HashMap<Arc<str>, usize> = HashMap::new();
        for issue in view.issues.iter().filter(|i| i.fixable) {
            *fixable.entry(issue.code.clone()).or_default() += 1;
        }
        Listed {
            rev: view.rev,
            issues: view.issues.len(),
            shown: self.shown,
            order,
            fixable,
        }
    }
}

/// The height of one issue row: the badge line, the message and the code with the hint, with
/// padding.
fn row_height() -> f32 {
    2.0 * space::SM + height::CONTROL + Type::Body.line_height() + Type::Caption.line_height()
}

/// The object of an issue: the name of the first, and how many more there are.
fn object_name(names: &HashMap<NodeId, Arc<str>>, objects: &[NodeId]) -> Option<String> {
    let first = objects.first()?;
    let name = names.get(first).map_or("Object", |name| &**name);
    Some(match objects.len() {
        1 => name.to_owned(),
        more => format!("{name} and {} more", more - 1),
    })
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    row_height: f32,
    view: &ValidatorView,
    issue: &IssueView,
    index: usize,
    same_kind: usize,
    names: &HashMap<NodeId, Arc<str>>,
    sink: &mut Sink,
) {
    let tokens = Tokens::of(ui.ctx());
    let selectable = !issue.objects.is_empty();
    let sense = if selectable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_height), sense);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let fill = if selectable && response.hovered() {
        tokens.bg_hover
    } else {
        tokens.bg_raised
    };
    ui.painter().rect_filled(rect, radius::CARD, fill);
    let inner = rect.shrink(space::SM);
    let top = egui::Rect::from_min_size(inner.min, vec2(inner.width(), height::CONTROL));

    let mut buttons_area = egui::Rect::NOTHING;
    if issue.fixable {
        let enabled = !view.stale && !view.running;
        let mut buttons = ui.new_child(
            UiBuilder::new()
                .max_rect(top)
                .layout(Layout::right_to_left(Align::Center)),
        );
        if buttons.add_enabled(enabled, Button::new("Fix")).clicked() {
            sink.push(UiRequest::Validator(ValidatorRequest::Fix {
                rev: view.rev,
                index,
            }));
        }
        let all = format!("Fix all {same_kind}");
        if same_kind > 1 && buttons.add_enabled(enabled, Button::new(&all)).clicked() {
            sink.push(UiRequest::Validator(ValidatorRequest::FixAll {
                rev: view.rev,
                code: issue.code.clone(),
            }));
        }
        buttons_area = buttons.min_rect();
        let taken = top.right() - buttons_area.left() + space::SM;
        draw_title(ui, top.with_max_x(top.right() - taken), issue, names);
    } else {
        draw_title(ui, top, issue, names);
    }

    let message = egui::Rect::from_min_size(
        egui::pos2(inner.left(), top.bottom()),
        vec2(inner.width(), Type::Body.line_height()),
    );
    let hint = egui::Rect::from_min_size(
        egui::pos2(inner.left(), message.bottom()),
        vec2(inner.width(), Type::Caption.line_height()),
    );
    for (area, line) in [
        (message, text(Type::Body, &*issue.message)),
        (
            hint,
            text(Type::Caption, format!("{}  {}", issue.code, issue.hint)),
        ),
    ] {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(area)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.add(Label::new(line).truncate().selectable(false));
    }
    // A click on a button, enabled or not, is not a click on the row.
    let on_buttons = response
        .interact_pointer_pos()
        .is_some_and(|pos| buttons_area.contains(pos));
    if selectable && response.clicked() && !on_buttons {
        sink.edit(Edit::Select {
            ids: issue.objects.clone(),
            op: SelectOp::Replace,
        });
    }
}

/// The badge and the object of an issue, at the left of its first line.
fn draw_title(ui: &mut Ui, area: egui::Rect, issue: &IssueView, names: &HashMap<NodeId, Arc<str>>) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = space::SM;
    badge(&mut child, issue.level.name(), issue.level.tone());
    if let Some(object) = object_name(names, &issue.objects) {
        child.add(
            Label::new(text(Type::Strong, object))
                .truncate()
                .selectable(false),
        );
    }
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
                |ui: &mut Ui, (state, view, sink): &mut Rig| {
                    state.show(ui, view.as_ref(), &[], 1, sink);
                },
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
    fn the_badge_counts_errors_or_else_warnings() {
        assert_eq!(ValidatorView::default().badge(), None);
        let v = view(vec![
            issue("A", IssueLevel::Error, false, &[]),
            issue("B", IssueLevel::Error, false, &[]),
            issue("C", IssueLevel::Warning, false, &[]),
            issue("D", IssueLevel::Info, false, &[]),
        ]);
        assert_eq!(v.badge(), Some((2, Tone::Danger)));
        assert_eq!(v.count(IssueLevel::Info), 1);
        let only_warnings = view(vec![
            issue("C", IssueLevel::Warning, false, &[]),
            issue("D", IssueLevel::Info, false, &[]),
        ]);
        assert_eq!(only_warnings.badge(), Some((1, Tone::Warning)));
        let only_info = view(vec![issue("D", IssueLevel::Info, false, &[])]);
        assert_eq!(only_info.badge(), None);
    }

    #[test]
    fn an_object_is_named_by_its_first_node() {
        let names: HashMap<NodeId, Arc<str>> = HashMap::from([
            (NodeId(1), Arc::from("Glass")),
            (NodeId(2), Arc::from("Cap")),
        ]);
        assert_eq!(object_name(&names, &[]), None);
        assert_eq!(object_name(&names, &[NodeId(1)]).as_deref(), Some("Glass"));
        assert_eq!(
            object_name(&names, &[NodeId(2), NodeId(1), NodeId(9)]).as_deref(),
            Some("Cap and 2 more")
        );
        assert_eq!(object_name(&names, &[NodeId(9)]).as_deref(), Some("Object"));
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

    #[test]
    fn a_clean_result_says_so() {
        let h = harness(Some(view(Vec::new())));
        h.get_by_label("No issues found");
    }

    #[test]
    fn a_long_list_draws_only_the_visible_rows() {
        let issues: Vec<IssueView> = (0..10_000)
            .map(|i| issue(&format!("RULE_{i}"), IssueLevel::Warning, false, &[1]))
            .collect();
        let h = harness(Some(view(issues)));
        h.get_by_label("RULE_0 message");
        assert!(h.query_by_label("RULE_9999 message").is_none());
        let drawn = h.get_all_by_label_contains("message").count();
        assert!(drawn < 40, "{drawn}");
    }
}
