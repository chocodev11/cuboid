//! The start screen and the Library panel in headless frames, with synthetic input only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_ui::{
    BriefCardView, ColorMode, ComingFrom, DocView, FontSet, HistoryView, ImageData, KeymapPreset,
    Layout, LibEntryView, LibKind, LibraryRequest, LibraryView, NavPreset, PaletteView, Prefs,
    RecentEntry, SuggestionAction, SuggestionGoal, SuggestionView, Template, ThumbState, Ui, UiCx,
    UiRequest, ViewSlot, ViewportFrame,
};
use egui::{Event, Modifiers, PointerButton, Role, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

struct Fixture {
    has_doc: bool,
    library: Option<LibraryView>,
    recent: Vec<RecentEntry>,
    palette: PaletteView,
    history: HistoryView,
}

struct App {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
    placements: usize,
    view_rect: Option<[f32; 4]>,
}

fn entry(id: &str, kind: LibKind, name: &str, thumb: ThumbState) -> LibEntryView {
    LibEntryView {
        id: id.to_owned(),
        kind,
        name: name.to_owned(),
        detail: "12 KB".to_owned(),
        tags: vec!["weapon".to_owned()],
        thumb,
    }
}

fn library() -> LibraryView {
    let picture = ImageData {
        size: [2, 2],
        rgba: Arc::from(vec![200_u8; 16]),
        rev: 1,
    };
    LibraryView {
        rev: 1,
        busy: false,
        counts: [2, 3, 1, 0, 4],
        total: 10,
        matches: 3,
        entries: Arc::from(vec![
            entry("m1", LibKind::Model, "gun.cubik", ThumbState::Unknown),
            entry(
                "t1",
                LibKind::Texture,
                "stone.png",
                ThumbState::Ready(picture),
            ),
            entry("r1", LibKind::Reference, "photo.jpg", ThumbState::Missing),
        ]),
        tags: Arc::from(vec![("weapon".to_owned(), 3)]),
        brief: BriefCardView {
            project_name: "Ink".to_owned(),
            mc_version: "1.21.6".to_owned(),
            style_tags: vec!["pixel".to_owned()],
            palette: vec![[255, 0, 0], [0, 0, 255]],
            density: Some(16.0),
            models: 2,
            textures: 3,
        },
        suggestions: Arc::from(vec![SuggestionView {
            title: "Roller has no model".to_owned(),
            detail: "The lore lists it.".to_owned(),
            action: None,
        }]),
        pinned: Arc::from(vec![PathBuf::from("C:/models/b.cubik")]),
        ..LibraryView::default()
    }
}

fn recent(name: &str) -> RecentEntry {
    RecentEntry {
        path: PathBuf::from(format!("C:/models/{name}")),
        name: name.to_owned(),
        detail: "C:/models".to_owned(),
        thumbnail: None,
        modified: None,
    }
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        fx,
        requests,
        placements,
        view_rect,
    } = app;
    let mut cx = UiCx::empty(&fx.recent);
    cx.library = fx.library.as_ref();
    if fx.has_doc {
        cx.doc = Some(DocView {
            title: "tower.cubik",
            path: Some(Path::new("C:/models/tower.cubik")),
            dirty: false,
            nodes: &[],
            nodes_rev: 1,
            selection: &[],
            primary: None,
            props: None,
            textures: &[],
            active_texture: None,
            palette: &fx.palette,
            history: &fx.history,
            display: None,
            status: "",
        });
        cx.views[3] = Some(ViewportFrame {
            texture: None,
            size_px: [400, 300],
        });
    }
    let out = cuboid.show(ui, &cx);
    requests.extend(out.requests);
    *placements = out.viewports.len();
    *view_rect = out.viewports.first().map(|v| v.rect);
    let _ = ViewSlot::MAIN;
}

fn prefs() -> Prefs {
    Prefs {
        color_mode: ColorMode::Dark,
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    }
}

fn harness(has_doc: bool, library: Option<LibraryView>) -> Harness<'static, App> {
    let fx = Fixture {
        has_doc,
        library,
        recent: vec![recent("a.cubik"), recent("b.cubik")],
        palette: PaletteView::default(),
        history: HistoryView::default(),
    };
    let app = App {
        ui: Ui::new(prefs(), FontSet::default()),
        fx,
        requests: Vec::new(),
        placements: 0,
        view_rect: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

fn take(harness: &mut Harness<'static, App>) -> Vec<UiRequest> {
    std::mem::take(&mut harness.state_mut().requests)
}

fn library_requests(requests: &[UiRequest]) -> Vec<&LibraryRequest> {
    requests
        .iter()
        .filter_map(|r| match r {
            UiRequest::Library(request) => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn the_start_screen_shows_new_open_recent_library_brief_and_the_version() {
    let mut harness = harness(false, Some(library()));
    for label in [
        "Block",
        "Item",
        "Voxel",
        "Empty",
        "Cubik classic",
        "2 models",
        "3 textures",
        "1 JSON",
        "4 lore",
        "Roller has no model",
        "Project brief",
        "Mossy Hollow",
        "Open file",
        "Ready",
    ] {
        // The brief of the fixture is named Ink; the sample names above are for the sample data.
        if label == "Mossy Hollow" {
            continue;
        }
        harness.get_by_label(label);
    }
    harness.get_by_label("Ink");
    harness.get_by_label(&format!("cuboid {}", env!("CARGO_PKG_VERSION")));
    harness
        .get_by_role_and_label(Role::ListItem, "Empty")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Import into library").click();
    harness.run_steps(3);
    harness.get_by_label("Import folder...").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(
        requests.contains(&UiRequest::NewDocument(Template::Empty)),
        "{requests:?}"
    );
    assert!(
        library_requests(&requests).contains(&&LibraryRequest::ImportFolder),
        "{requests:?}"
    );
}

#[test]
fn open_file_and_the_templates_send_their_requests_and_voxel_asks_for_a_size() {
    let mut harness = harness(false, Some(library()));
    harness.get_by_label("Open file").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::ListItem, "Cubik classic")
        .click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [
            UiRequest::OpenDialog,
            UiRequest::NewDocument(Template::CubikClassic)
        ]
    );
    // Voxel asks for the size first and starts nothing until it is answered.
    harness
        .get_by_role_and_label(Role::ListItem, "Voxel")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Voxels per block");
    assert!(take(&mut harness).is_empty());
    harness.get_by_label("64").click();
    harness.run_steps(2);
    harness.get_by_label("Create").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::NewDocument(Template::Voxel { resolution: 64 })]
    );
}

#[test]
fn pinned_recent_files_come_first_and_the_pin_does_not_open_the_file() {
    let mut harness = harness(false, Some(library()));
    let a = harness.get_by_role_and_label(Role::Button, "a").rect();
    let b = harness.get_by_role_and_label(Role::Button, "b").rect();
    assert!(
        b.left() < a.left(),
        "the pinned b comes before a: {a:?} {b:?}"
    );
    harness.get_by_role_and_label(Role::Button, "Pin a").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert_eq!(
        library_requests(&requests),
        [&LibraryRequest::TogglePin(PathBuf::from(
            "C:/models/a.cubik"
        ))]
    );
    assert!(
        !requests.iter().any(|r| matches!(r, UiRequest::Open(_))),
        "{requests:?}"
    );
}

#[test]
fn a_recent_card_opens_its_file_and_names_the_format() {
    let mut harness = harness(false, Some(library()));
    assert_eq!(harness.get_all_by_label("cubik").count(), 2);
    harness.get_by_role_and_label(Role::Button, "a").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Open(PathBuf::from("C:/models/a.cubik"))]
    );
}

#[test]
fn the_filter_narrows_the_recent_files() {
    let mut harness = harness(false, Some(library()));
    harness.get_by_label("Filter recent files").focus();
    harness.run_steps(2);
    harness.get_by_label("Filter recent files").type_text("a.c");
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::Button, "a");
    assert!(harness.query_by_role_and_label(Role::Button, "b").is_none());
}

#[test]
fn the_start_screen_works_without_a_library() {
    let harness = harness(false, None);
    harness.get_by_label("Block");
    harness.get_by_label("The library is not available.");
}

#[test]
fn the_start_screen_has_no_settings_strip() {
    let harness = harness(false, Some(library()));
    for label in ["Light", "Dark", "125%", "Blender"] {
        assert!(
            harness
                .query_by_role_and_label(Role::Button, label)
                .is_none(),
            "{label}"
        );
    }
}

#[test]
fn an_empty_start_screen_offers_new_block_open_and_import() {
    let fx = Fixture {
        has_doc: false,
        library: Some(LibraryView {
            busy: false,
            ..LibraryView::default()
        }),
        recent: Vec::new(),
        palette: PaletteView::default(),
        history: HistoryView::default(),
    };
    let app = App {
        ui: Ui::new(prefs(), FontSet::default()),
        fx,
        requests: Vec::new(),
        placements: 0,
        view_rect: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness.get_by_label("No models yet");
    harness.get_by_label("New block").click();
    harness.run_steps(3);
    harness.get_by_label("Import a folder").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(requests.contains(&UiRequest::NewDocument(Template::Block)));
    assert!(library_requests(&requests).contains(&&LibraryRequest::ImportFolder));
}

#[test]
fn a_suggestion_with_an_action_has_one_button() {
    let mut view = library();
    view.suggestions = Arc::from(vec![SuggestionView {
        title: "Roller has no model".to_owned(),
        detail: "The lore lists it.".to_owned(),
        action: Some(SuggestionAction {
            label: "Create".to_owned(),
            goal: SuggestionGoal::New(Template::Item),
        }),
    }]);
    let mut harness = harness(false, Some(view));
    harness.get_by_label("Create").click();
    harness.run_steps(3);
    assert_eq!(take(&mut harness), [UiRequest::NewDocument(Template::Item)]);
}

#[test]
fn the_brief_is_read_only_and_edit_opens_the_dialog_that_sends_it() {
    let mut harness = harness(false, Some(library()));
    harness.get_by_label("Java 1.21.6 \u{b7} pixel");
    assert!(
        harness
            .get_all_by_role(Role::TextInput)
            .all(|node| node.value().as_deref() != Some("Ink")),
        "the card has no text field"
    );
    harness.get_by_label("Edit").click();
    harness.run_steps(3);
    input_with(&harness, "Ink").focus();
    harness.run_steps(2);
    input_with(&harness, "Ink").type_text(" Wars");
    harness.run_steps(2);
    harness.get_by_label("Save").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    let brief = library_requests(&requests)
        .into_iter()
        .find_map(|r| match r {
            LibraryRequest::SetBrief {
                name,
                mc_version,
                style_tags,
            } => Some((name.clone(), mc_version.clone(), style_tags.clone())),
            _ => None,
        });
    assert_eq!(
        brief,
        Some((
            "Ink Wars".to_owned(),
            "1.21.6".to_owned(),
            vec!["pixel".to_owned()]
        )),
        "{requests:?}"
    );
    assert!(harness.query_by_label("Project brief").is_some());
    harness.get_by_label("Edit").click();
    harness.run_steps(3);
    harness.get_by_label("Cancel").click();
    harness.run_steps(3);
    assert!(
        library_requests(&take(&mut harness))
            .iter()
            .all(|r| !matches!(r, LibraryRequest::SetBrief { .. })),
        "Cancel sends nothing"
    );
}

// The text field that holds this text.
fn input_with<'a>(harness: &'a Harness<'static, App>, value: &str) -> egui_kittest::Node<'a> {
    harness
        .get_all_by_role(Role::TextInput)
        .find(|node| node.value().as_deref() == Some(value))
        .expect("a text field with this text")
}

fn open_panel(harness: &mut Harness<'static, App>) {
    harness.get_by_role_and_label(Role::Tab, "Library").click();
    harness.run_steps(3);
}

#[test]
fn the_panel_lists_entries_filters_and_asks_for_missing_pictures_once() {
    let mut harness = harness(true, Some(library()));
    open_panel(&mut harness);
    for name in ["gun.cubik", "stone.png", "photo.jpg"] {
        harness.get_by_role_and_label(Role::Button, name);
    }
    harness.run_steps(3);
    let requests = take(&mut harness);
    let libs = library_requests(&requests);
    let thumbs: Vec<_> = libs
        .iter()
        .filter_map(|r| match r {
            LibraryRequest::Thumbs(ids) => Some(ids.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        thumbs,
        [vec!["m1".to_owned()]],
        "only the unknown picture is asked for, once"
    );
    assert!(libs.contains(&&LibraryRequest::Query {
        text: String::new(),
        kind: None,
        tag: None
    }));

    harness
        .get_by_role_and_label(Role::Button, "Textures 3")
        .click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(
        library_requests(&requests).contains(&&LibraryRequest::Query {
            text: String::new(),
            kind: Some(LibKind::Texture),
            tag: None
        }),
        "{requests:?}"
    );
}

#[test]
fn selecting_a_row_and_committing_tags_sends_them() {
    let mut harness = harness(true, Some(library()));
    open_panel(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "stone.png")
        .click();
    harness.run_steps(3);
    input_with(&harness, "weapon").focus();
    harness.run_steps(2);
    input_with(&harness, "weapon").type_text(", blue");
    harness.run_steps(2);
    harness.get_by_label("Search the library").focus();
    harness.run_steps(3);
    let requests = take(&mut harness);
    let sent = library_requests(&requests)
        .into_iter()
        .find_map(|r| match r {
            LibraryRequest::SetTags { id, tags } => Some((id.clone(), tags.clone())),
            _ => None,
        });
    assert_eq!(
        sent,
        Some((
            "t1".to_owned(),
            vec!["weapon".to_owned(), "blue".to_owned()]
        ))
    );
}

fn drag_row_to_view(harness: &mut Harness<'static, App>, name: &str) -> Vec<UiRequest> {
    let from = harness
        .get_by_role_and_label(Role::Button, name)
        .rect()
        .center();
    let rect = harness.state().view_rect.expect("a view is drawn");
    let to = pos2(rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
    take(harness);
    harness.event(Event::PointerMoved(from));
    harness.run_steps(2);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        harness.event(Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(2);
    }
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(3);
    take(harness)
}

#[test]
fn a_texture_dropped_on_the_model_is_added_and_a_reference_is_pinned() {
    let mut harness = harness(true, Some(library()));
    open_panel(&mut harness);
    let requests = drag_row_to_view(&mut harness, "stone.png");
    assert!(
        library_requests(&requests).contains(&&LibraryRequest::UseTexture("t1".to_owned())),
        "{requests:?}"
    );
    let requests = drag_row_to_view(&mut harness, "photo.jpg");
    assert!(
        library_requests(&requests).contains(&&LibraryRequest::PinReference("r1".to_owned())),
        "{requests:?}"
    );
    // A model has nothing to do on the view.
    let requests = drag_row_to_view(&mut harness, "gun.cubik");
    let acted = library_requests(&requests).into_iter().any(|r| {
        matches!(
            r,
            LibraryRequest::UseTexture(_) | LibraryRequest::PinReference(_)
        )
    });
    assert!(!acted, "{requests:?}");
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("not needed".to_owned())
    }
}

#[test]
fn files_dropped_on_the_panel_are_imported_not_opened() {
    let mut harness = harness(true, Some(library()));
    open_panel(&mut harness);
    let rect = harness
        .get_by_role_and_label(Role::Button, "stone.png")
        .rect();
    harness.event(Event::PointerMoved(rect.center()));
    harness.run_steps(2);
    take(&mut harness);
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(Dropped(PathBuf::from("C:/art/new.png"))));
    harness.run_steps(2);
    let requests = take(&mut harness);
    assert!(
        library_requests(&requests).contains(&&LibraryRequest::Import(vec![PathBuf::from(
            "C:/art/new.png"
        )])),
        "{requests:?}"
    );
    assert!(
        !requests.iter().any(|r| matches!(r, UiRequest::Open(_))),
        "{requests:?}"
    );
}

fn fresh_install() -> Harness<'static, App> {
    let fx = Fixture {
        has_doc: false,
        library: Some(library()),
        recent: Vec::new(),
        palette: PaletteView::default(),
        history: HistoryView::default(),
    };
    let app = App {
        ui: Ui::new(
            Prefs {
                color_mode: ColorMode::Dark,
                ..Prefs::default()
            },
            FontSet::default(),
        ),
        fx,
        requests: Vec::new(),
        placements: 0,
        view_rect: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

fn continue_disabled(harness: &Harness<'static, App>) -> bool {
    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .accesskit_node()
        .is_disabled()
}

#[test]
fn the_first_run_asks_which_app_the_user_knows_and_an_accent_and_continues() {
    let mut harness = fresh_install();
    let neutral_accent = cuboid_ui_kit::Tokens::of(&harness.ctx).accent;
    let real = Prefs::default().theme().tokens(true);
    assert_ne!(
        neutral_accent, real.accent,
        "no accent colour before the choice"
    );
    assert_eq!(neutral_accent, real.text_strong);

    harness.get_by_label("Which app are you used to?");
    for from in ComingFrom::ALL {
        harness.get_by_role_and_label(Role::RadioButton, from.name());
    }
    for name in ["Yellow", "Peach", "Mauve", "Green", "Teal", "Blue"] {
        harness.get_by_role_and_label(Role::Button, name);
    }
    // Blockbench is preselected and the accent is optional: Continue works at once.
    assert!(!continue_disabled(&harness));

    harness
        .get_by_role_and_label(Role::RadioButton, ComingFrom::Blender.name())
        .click();
    harness.run_steps(3);
    let stored = harness.state().ui.prefs().clone();
    assert!(stored.nav_chosen && !stored.accent_chosen);
    assert_eq!(stored.coming_from, ComingFrom::Blender);
    assert_eq!(stored.nav, NavPreset::Blender);
    assert_eq!(stored.keymap, KeymapPreset::Blender);
    assert!(!continue_disabled(&harness));

    harness.get_by_role_and_label(Role::Button, "Blue").click();
    harness.run_steps(3);
    let stored = harness.state().ui.prefs().clone();
    assert!(stored.nav_chosen && stored.accent_chosen);
    assert_eq!(stored.accent, [137, 180, 250]);
    let chosen = cuboid_ui_kit::Tokens::of(&harness.ctx).accent;
    assert_ne!(chosen, neutral_accent, "the chosen accent takes over");
    assert!(!continue_disabled(&harness));
    assert!(take(&mut harness).iter().any(|r| matches!(
        r,
        UiRequest::PrefsChanged(p) if p.accent_chosen && p.nav_chosen
    )));

    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .click();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Continue")
            .is_none(),
        "the dialog is gone"
    );
    harness.get_by_role_and_label(Role::ListItem, "Block");
}

#[test]
fn each_app_choice_sets_navigation_keymap_and_arrangement_at_once() {
    for from in ComingFrom::ALL {
        let mut harness = fresh_install();
        harness
            .get_by_role_and_label(Role::RadioButton, from.name())
            .click();
        harness.run_steps(3);
        let prefs = harness.state().ui.prefs();
        assert_eq!(prefs.nav, from.nav(), "{from:?}");
        assert_eq!(prefs.keymap, from.keymap(), "{from:?}");
        assert_eq!(
            prefs.layout,
            Layout::arranged(from.arrangement()),
            "{from:?}"
        );
        // The theme and the accent are not part of the choice.
        assert_eq!(prefs.color_mode, ColorMode::Dark);
        assert!(!prefs.accent_chosen);
    }
}

#[test]
fn continuing_with_the_preselected_app_applies_blockbench_and_keeps_the_neutral_highlight() {
    let mut harness = fresh_install();
    let neutral_accent = cuboid_ui_kit::Tokens::of(&harness.ctx).accent;
    assert!(!harness.state().ui.prefs().nav_chosen);
    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .click();
    harness.run_steps(3);
    let prefs = harness.state().ui.prefs().clone();
    assert!(prefs.nav_chosen && !prefs.accent_chosen);
    assert_eq!(
        (prefs.nav, prefs.keymap),
        (NavPreset::Blockbench, KeymapPreset::Blockbench)
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Continue")
            .is_none(),
        "the dialog is gone"
    );
    assert_eq!(
        cuboid_ui_kit::Tokens::of(&harness.ctx).accent,
        neutral_accent,
        "no accent was picked"
    );
    // The question is answered: a later start does not ask again.
    let again = Ui::new(prefs, FontSet::default());
    assert_eq!(again.prefs().coming_from, ComingFrom::Blockbench);
}

#[test]
fn nothing_behind_the_first_run_dialog_moves() {
    let mut harness = fresh_install();
    let before = harness.get_by_label("Open file").rect();
    harness.get_by_role_and_label(Role::Button, "Mauve").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .click();
    harness.run_steps(3);
    assert_eq!(harness.get_by_label("Open file").rect(), before);
}

#[test]
fn choosing_the_first_accent_equal_to_the_default_still_leaves_neutral_mode() {
    // Yellow is the default accent: the theme itself does not change, only the flag does.
    let mut harness = fresh_install();
    let neutral_accent = cuboid_ui_kit::Tokens::of(&harness.ctx).accent;
    harness
        .get_by_role_and_label(Role::Button, "Yellow")
        .click();
    harness.run_steps(3);
    let chosen = cuboid_ui_kit::Tokens::of(&harness.ctx).accent;
    assert_ne!(chosen, neutral_accent);
    assert_eq!(chosen, Prefs::default().theme().tokens(true).accent);
}

#[test]
fn an_old_file_is_not_asked_and_keeps_its_accent() {
    let prefs = Prefs::from_json(r#"{"accent": [137, 180, 250]}"#);
    assert!(prefs.nav_chosen && prefs.accent_chosen);
    let ui = Ui::new(prefs, FontSet::default());
    assert_eq!(ui.prefs().accent, [137, 180, 250]);
    let fx = Fixture {
        has_doc: false,
        library: None,
        recent: Vec::new(),
        palette: PaletteView::default(),
        history: HistoryView::default(),
    };
    let app = App {
        ui,
        fx,
        requests: Vec::new(),
        placements: 0,
        view_rect: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Continue")
            .is_none()
    );
    let real = harness.state().ui.prefs().theme().tokens(true);
    assert_eq!(cuboid_ui_kit::Tokens::of(&harness.ctx).accent, real.accent);
}

/// The start screen at a window width in points.
fn harness_at(width: f32) -> Harness<'static, App> {
    let fx = Fixture {
        has_doc: false,
        library: Some(library()),
        recent: vec![recent("a.cubik"), recent("b.cubik")],
        palette: PaletteView::default(),
        history: HistoryView::default(),
    };
    let app = App {
        ui: Ui::new(prefs(), FontSet::default()),
        fx,
        requests: Vec::new(),
        placements: 0,
        view_rect: None,
    };
    let mut harness = Harness::builder()
        .with_size(vec2(width, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

#[test]
fn the_brief_has_a_column_from_1280_and_folds_into_the_main_area_below() {
    let wide = harness_at(1280.0);
    let brief = wide.get_by_label("Project brief").rect();
    assert!(brief.left() >= 1280.0 - 260.0, "{brief:?}");
    for width in [1279.0, 1024.0] {
        let narrow = harness_at(width);
        let brief = narrow.get_by_label("Project brief").rect();
        assert!(brief.right() < width - 8.0, "{width}: {brief:?}");
        assert!(brief.left() > 240.0, "{width}: {brief:?}");
        // The sidebar keeps its rows.
        narrow.get_by_role_and_label(Role::ListItem, "Block");
    }
}

#[test]
fn below_1024_the_sidebar_shows_icons_only() {
    let mut harness = harness_at(1000.0);
    assert!(
        harness
            .query_by_role_and_label(Role::ListItem, "Block")
            .is_none(),
        "the rows are icon buttons"
    );
    let open = harness.get_by_label("Open file").rect();
    assert!(open.width() <= 52.0, "{open:?}");
    harness.get_by_role_and_label(Role::Button, "Empty").click();
    harness.run_steps(3);
    assert!(take(&mut harness).contains(&UiRequest::NewDocument(Template::Empty)));
}
