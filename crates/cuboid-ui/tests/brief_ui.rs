//! The style references of the brief in headless frames: the card line, the dialog section with
//! its folders, and the requests it sends. Synthetic input only.

use cuboid_ui::{
    BriefCardView, BriefInputs, ColorMode, FontSet, JobView, LibScope, LibraryRequest, LibraryView,
    Prefs, StyleSourceView, Ui, UiCx, UiRequest,
};
use egui::{Key, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

struct App {
    ui: Ui,
    view: LibraryView,
    requests: Vec<UiRequest>,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        view,
        requests,
    } = app;
    let mut cx = UiCx::empty(&[]);
    cx.library = Some(view);
    requests.extend(cuboid.show(ui, &cx).requests);
}

fn source(path: &str, models: usize, textures: usize, licence: &str) -> StyleSourceView {
    StyleSourceView {
        path: path.to_owned(),
        models,
        textures,
        licence: licence.to_owned(),
    }
}

/// The library of a project with two style folders: 16 references in one, 2 in the other.
fn project_view() -> LibraryView {
    LibraryView {
        rev: 1,
        busy: false,
        scope: LibScope::Project,
        project_available: true,
        brief: BriefCardView {
            inputs: BriefInputs {
                project_name: "Ink".to_owned(),
                ..BriefInputs::default()
            },
            project: true,
            style_sources: vec![
                source("refs/lamps", 12, 4, "CC0"),
                source("refs/trims", 2, 0, "unknown"),
            ],
            style_models: 14,
            style_textures: 4,
            ..BriefCardView::default()
        },
        ..LibraryView::default()
    }
}

fn harness(view: LibraryView) -> Harness<'static, App> {
    let prefs = Prefs {
        color_mode: ColorMode::Dark,
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    };
    let app = App {
        ui: Ui::new(prefs, FontSet::default()),
        view,
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

/// Opens the dialog and scrolls its last section into view.
fn open_style(harness: &mut Harness<'static, App>) {
    harness.get_by_label("Edit").click();
    harness.run_steps(3);
    harness.get_by_label("Style references").scroll_to_me();
    harness.run_steps(20);
}

fn take(harness: &mut Harness<'static, App>) -> Vec<LibraryRequest> {
    std::mem::take(&mut harness.state_mut().requests)
        .into_iter()
        .filter_map(|request| match request {
            UiRequest::Library(request) => Some(request),
            _ => None,
        })
        .collect()
}

/// The licence field of the next style folder, which a hint names.
fn licence_field<'a>(harness: &'a Harness<'static, App>) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::TextInput, "unknown")
}

#[test]
fn the_card_says_how_many_references_the_project_has_and_from_how_many_folders() {
    harness(project_view()).get_by_label("Style: 14 models, 4 textures from 2 folders");

    // An empty folder is not counted.
    let mut view = project_view();
    view.brief.style_sources = vec![
        source("refs/lamps", 12, 0, "CC0"),
        source("refs/none", 0, 0, "unknown"),
    ];
    view.brief.style_models = 12;
    view.brief.style_textures = 0;
    harness(view).get_by_label("Style: 12 models from 1 folder");

    // Models marked one by one come from no folder, so the folders are not named.
    let mut view = project_view();
    view.brief.style_sources = vec![source("refs/one", 1, 0, "unknown")];
    view.brief.style_models = 3;
    view.brief.style_textures = 0;
    harness(view).get_by_label("Style: 3 models");

    // One reference: the singular. Without references the card says nothing.
    let mut view = project_view();
    view.brief.style_sources = vec![source("refs/one", 1, 0, "unknown")];
    view.brief.style_models = 1;
    view.brief.style_textures = 0;
    harness(view).get_by_label("Style: 1 model from 1 folder");
    let mut view = project_view();
    view.brief.style_sources.clear();
    view.brief.style_models = 0;
    view.brief.style_textures = 0;
    assert!(harness(view).query_by_label_contains("Style:").is_none());
}

#[test]
fn the_counts_of_the_card_use_the_singular_for_one() {
    let mut view = project_view();
    view.brief.models = 1;
    view.brief.textures = 1;
    harness(view).get_by_label("no texel density yet \u{b7} 1 model, 1 texture");
}

#[test]
fn the_dialog_lists_each_folder_with_its_counts_and_licence() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness.get_by_label("refs/lamps");
    harness.get_by_label("12 models, 4 textures \u{b7} Licence: CC0");
    harness.get_by_label("refs/trims");
    harness.get_by_label("2 models \u{b7} Licence: unknown");
    assert_eq!(harness.get_all_by_label("Rescan").count(), 2);
    assert_eq!(harness.get_all_by_label("Remove").count(), 2);
    // The licence of the next folder starts empty, and the hint says what that means.
    assert!(licence_field(&harness).value().is_none_or(|v| v.is_empty()));
    harness.get_by_label("Add folder");
    assert!(take(&mut harness).is_empty(), "looking sends nothing");
}

#[test]
fn rescan_and_add_folder_send_their_requests_and_leave_the_dialog_open() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness
        .get_all_by_role_and_label(Role::Button, "Rescan")
        .nth(1)
        .expect("the second folder")
        .click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [LibraryRequest::RescanStyle("refs/trims".to_owned())]
    );

    // An empty licence is sent as it is: the library reads it as unknown.
    harness
        .get_by_role_and_label(Role::Button, "Add folder")
        .click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [LibraryRequest::AddStyleFolder {
            licence: String::new()
        }]
    );

    // The licence is read at the click.
    licence_field(&harness).focus();
    harness.run_steps(2);
    licence_field(&harness).type_text("CC-BY 4.0");
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::Button, "Add folder")
        .click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [LibraryRequest::AddStyleFolder {
            licence: "CC-BY 4.0".to_owned()
        }]
    );
    harness.get_by_role_and_label(Role::Button, "Save");
}

#[test]
fn remove_asks_first_and_only_the_answer_remove_sends_it() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness
        .get_all_by_role_and_label(Role::Button, "Remove")
        .next()
        .expect("the first folder")
        .click();
    harness.run_steps(3);
    harness.get_by_label(
        "Remove the 16 style references imported from this folder? The folder itself is not touched.",
    );
    assert!(take(&mut harness).is_empty(), "nothing before the answer");

    // Cancel, or Esc, closes the question and sends nothing.
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("The folder itself is not touched.")
            .is_none()
    );
    assert!(take(&mut harness).is_empty());
    harness.get_by_role_and_label(Role::Button, "Save");

    harness
        .get_all_by_role_and_label(Role::Button, "Remove")
        .next()
        .expect("the first folder")
        .click();
    harness.run_steps(3);
    // Enter does not confirm a removal.
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    assert!(take(&mut harness).is_empty());
    // The button of the question is the one that came last.
    harness
        .get_all_by_role_and_label(Role::Button, "Remove")
        .last()
        .expect("the answer")
        .click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [LibraryRequest::RemoveStyleSource("refs/lamps".to_owned())]
    );
    assert!(
        harness
            .query_by_label_contains("The folder itself is not touched.")
            .is_none(),
        "the question is closed"
    );
    harness.get_by_role_and_label(Role::Button, "Save");
}

#[test]
fn the_question_counts_the_references_of_its_folder() {
    let mut view = project_view();
    view.brief.style_sources = vec![
        source("refs/one", 1, 0, "unknown"),
        source("refs/none", 0, 0, "unknown"),
    ];
    let mut harness = harness(view);
    open_style(&mut harness);
    let remove = |harness: &mut Harness<'static, App>, at: usize| {
        harness
            .get_all_by_role_and_label(Role::Button, "Remove")
            .nth(at)
            .expect("a folder")
            .click();
        harness.run_steps(3);
    };
    remove(&mut harness, 0);
    harness.get_by_label(
        "Remove the style reference imported from this folder? The folder itself is not touched.",
    );
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    harness.get_by_label("No models or textures \u{b7} Licence: unknown");
    remove(&mut harness, 1);
    harness.get_by_label(
        "Remove this folder from the style references? The folder itself is not touched.",
    );
}

#[test]
fn saving_sends_the_brief_alone_and_the_folders_stay_with_the_library() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Save").click();
    harness.run_steps(3);
    let sent = take(&mut harness);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let LibraryRequest::SetBrief(inputs) = &sent[0] else {
        panic!("{sent:?}");
    };
    assert_eq!(inputs.project_name, "Ink");
}

#[test]
fn the_folders_follow_the_library_while_the_dialog_is_open() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness.state_mut().view.brief.style_sources.remove(0);
    harness.run_steps(3);
    assert!(harness.query_by_label("refs/lamps").is_none());
    harness.get_by_label("refs/trims");
    harness.state_mut().view.brief.style_sources[0].models = 5;
    harness.run_steps(3);
    harness.get_by_label("5 models \u{b7} Licence: unknown");

    // While a job holds the library, the buttons wait.
    harness.state_mut().view.busy = true;
    harness.run_steps(3);
    for label in ["Rescan", "Remove", "Add folder"] {
        let node = harness.get_by_role_and_label(Role::Button, label);
        assert!(node.accesskit_node().is_disabled(), "{label}");
    }
}

#[test]
fn save_and_enter_wait_while_a_job_holds_the_library_and_the_edit_is_kept() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness.state_mut().view.busy = true;
    harness.run_steps(3);
    let save = harness.get_by_role_and_label(Role::Button, "Save");
    assert!(save.accesskit_node().is_disabled());
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    assert!(take(&mut harness).is_empty(), "Enter sends nothing either");

    // The dialog stays open and takes the edit.
    harness.get_by_label("What the project is for").focus();
    harness.run_steps(2);
    harness
        .get_by_label("What the project is for")
        .type_text("Splat weapons");
    harness.run_steps(2);
    harness.state_mut().view.busy = false;
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::Button, "Save").click();
    harness.run_steps(3);
    let sent = take(&mut harness);
    let [LibraryRequest::SetBrief(inputs)] = sent.as_slice() else {
        panic!("{sent:?}");
    };
    assert_eq!(inputs.goal, "Splat weapons");
}

#[test]
fn the_job_of_the_library_shows_in_the_section_and_cancel_import_stops_it() {
    // The dialog opens on an idle library, then Add folder starts the import.
    let mut harness = harness(project_view());
    open_style(&mut harness);
    assert!(harness.query_by_label("Cancel import").is_none());
    let view = &mut harness.state_mut().view;
    view.busy = true;
    view.job = Some(JobView {
        id: 2,
        label: "Importing style references".to_owned(),
        fraction: Some(0.4),
        cancellable: true,
    });
    harness.run_steps(3);
    harness.get_by_label("Importing style references");
    harness
        .get_by_role_and_label(Role::Button, "Cancel import")
        .click();
    harness.run_steps(3);
    let requests = std::mem::take(&mut harness.state_mut().requests);
    assert!(
        matches!(requests.as_slice(), [UiRequest::CancelJob(2)]),
        "{requests:?}"
    );

    // A job that cannot be stopped has no button; without a job there is no card.
    harness
        .state_mut()
        .view
        .job
        .as_mut()
        .expect("a job")
        .cancellable = false;
    harness.run_steps(3);
    harness.get_by_label("Importing style references");
    assert!(harness.query_by_label("Cancel import").is_none());
    harness.state_mut().view.job = None;
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Importing style references")
            .is_none()
    );
}

#[test]
fn a_removal_that_waits_for_the_answer_closes_when_its_folder_is_gone() {
    let mut harness = harness(project_view());
    open_style(&mut harness);
    harness
        .get_all_by_role_and_label(Role::Button, "Remove")
        .next()
        .expect("the first folder")
        .click();
    harness.run_steps(3);
    harness.state_mut().view.brief.style_sources.remove(0);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("The folder itself is not touched.")
            .is_none()
    );
    assert!(take(&mut harness).is_empty());
}

#[test]
fn the_global_library_has_no_style_folders() {
    let mut view = project_view();
    view.scope = LibScope::Global;
    view.brief.project = false;
    view.brief.style_sources.clear();
    view.brief.style_models = 0;
    view.brief.style_textures = 0;
    let mut harness = harness(view);
    open_style(&mut harness);
    // A saved project is there: the caption says where to find its library.
    harness.get_by_label(
        "Style references belong to the project library. Switch the library to Project to add them.",
    );
    assert!(harness.query_by_label("Add folder").is_none());
    assert!(harness.query_by_label("Rescan").is_none());
}

#[test]
fn without_a_saved_project_the_caption_asks_for_a_saved_model() {
    let mut view = project_view();
    view.scope = LibScope::Global;
    view.project_available = false;
    view.brief.project = false;
    view.brief.style_sources.clear();
    let mut harness = harness(view);
    open_style(&mut harness);
    harness.get_by_label(
        "Style references belong to the project library, which sits next to a saved model.",
    );
}
