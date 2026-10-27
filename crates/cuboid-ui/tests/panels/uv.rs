//! The UV editor in headless frames: the view (zoom, pan, fit, the pixel grid), the faces drawn in
//! it and chosen by click, Shift and Ctrl, the move of several faces with the snap, the arrow
//! keys, and the numbers under the view. The input is synthetic input to the harness.

use egui::{Pos2, Rect, Shape};
use egui_kittest::kittest::NodeT;

use super::*;

/// A rig whose faces are six rectangles of 4 by 4 texels, three by two on the 16 by 16 lantern
/// texture: Up, Down (switched off) and North above; South, West and East below.
fn uv_harness() -> Harness<'static, Rig> {
    let mut fx = Fixture::new();
    let grid = [
        [0.0, 0.0, 4.0, 4.0],
        [4.0, 0.0, 8.0, 4.0],
        [8.0, 0.0, 12.0, 4.0],
        [0.0, 4.0, 4.0, 8.0],
        [4.0, 4.0, 8.0, 8.0],
        [8.0, 4.0, 12.0, 8.0],
    ];
    for (face, uv) in fx.props.faces.as_mut().expect("faces").iter_mut().zip(grid) {
        face.uv = uv;
    }
    harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0))
}

fn view(harness: &Harness<'static, Rig>) -> Rect {
    harness.get_by_label("UV view").rect()
}

/// The zoom of the view in points to a texel, from the readout of its toolbar.
fn zoom(harness: &Harness<'static, Rig>) -> f32 {
    let percent = |label: String| label.strip_suffix('%')?.parse::<f32>().ok();
    harness
        .query_all_by(|node| node.role() == Role::Button)
        .find_map(|node| percent(node.accesskit_node().label()?))
        .expect("the zoom readout")
        / 100.0
}

/// The line under the picture that names the texel under the pointer.
fn texel(harness: &Harness<'static, Rig>) -> Option<String> {
    let node = harness.query_by_label_contains("Texel ")?;
    // The text of a label is its value.
    node.accesskit_node().value()
}

/// The column of a texel line such as "Texel 3, 11".
fn column(text: &str) -> i32 {
    let numbers = text.trim_start_matches("Texel ");
    let first = numbers.split(',').next().expect("a column");
    first.trim().parse().expect("a number")
}

/// Where a point of the texture is on the screen while the picture is fitted and has not moved:
/// the 16 by 16 texture is centred in the view.
fn at(harness: &Harness<'static, Rig>, x: f32, y: f32) -> Pos2 {
    view(harness).center() + (vec2(x, y) - vec2(8.0, 8.0)) * zoom(harness)
}

/// A spot outside the UV editor.
fn elsewhere() -> Pos2 {
    vec2(800.0, 450.0).to_pos2()
}

fn hover(harness: &mut Harness<'static, Rig>, at: Pos2) {
    harness.event(Event::PointerMoved(at));
    harness.run_steps(2);
}

fn press(harness: &mut Harness<'static, Rig>, at: Pos2, button: PointerButton, mods: Modifiers) {
    harness.event(Event::ModifiersChanged(mods));
    hover(harness, at);
    harness.event(Event::PointerButton {
        pos: at,
        button,
        pressed: true,
        modifiers: mods,
    });
    harness.run_steps(2);
}

fn release(harness: &mut Harness<'static, Rig>, at: Pos2, button: PointerButton) {
    harness.event(Event::PointerButton {
        pos: at,
        button,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(3);
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(2);
}

fn click(harness: &mut Harness<'static, Rig>, at: Pos2, mods: Modifiers) {
    press(harness, at, PointerButton::Primary, mods);
    release(harness, at, PointerButton::Primary);
}

/// Presses at `from`, moves to `to` in steps with `mods` held from the first step, and lets go.
fn drag_with(
    harness: &mut Harness<'static, Rig>,
    button: PointerButton,
    from: Pos2,
    to: Pos2,
    mods: Modifiers,
) {
    press(harness, from, button, Modifiers::NONE);
    harness.event(Event::ModifiersChanged(mods));
    for step in 1..=4 {
        hover(harness, from + (to - from) * (step as f32 / 4.0));
    }
    release(harness, to, button);
}

fn drag(harness: &mut Harness<'static, Rig>, from: Pos2, to: Pos2, mods: Modifiers) {
    drag_with(harness, PointerButton::Primary, from, to, mods);
}

/// Turns the wheel by notches with the pointer at `at`.
fn wheel(harness: &mut Harness<'static, Rig>, at: Pos2, notches: f32) {
    hover(harness, at);
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, notches),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(3);
}

/// Checks that the face picker is named so: the primary face, and how many others are chosen.
fn chosen(harness: &Harness<'static, Rig>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label);
}

/// The moves of faces among the requests: the faces, the step and whether it ends the drag.
type Moves = Vec<(Vec<Face>, glam::Vec2, bool)>;

fn moves(requests: &[UiRequest]) -> Moves {
    edits(requests)
        .into_iter()
        .filter_map(|edit| match edit {
            Edit::MoveFaces {
                faces,
                delta,
                commit,
            } => Some((faces.clone(), *delta, *commit)),
            _ => None,
        })
        .collect()
}

/// How far the steps of the moves add up to, in texels.
fn travelled(moves: &Moves) -> glam::Vec2 {
    moves.iter().map(|m| m.1).sum()
}

/// Every shape drawn last frame with the clip rectangle it was drawn in.
fn shapes(harness: &Harness<'static, Rig>) -> Vec<(Rect, Shape)> {
    fn walk(clip: Rect, shape: &Shape, out: &mut Vec<(Rect, Shape)>) {
        match shape {
            Shape::Vec(inner) => inner.iter().for_each(|shape| walk(clip, shape, out)),
            other => out.push((clip, other.clone())),
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(clipped.clip_rect, &clipped.shape, &mut out);
    }
    out
}

/// The shapes drawn in the clip rectangle of the view, and nowhere else.
fn in_view(harness: &Harness<'static, Rig>) -> Vec<Shape> {
    let view = view(harness);
    let same = |a: Rect, b: Rect| (a.min - b.min).length() < 1.0 && (a.max - b.max).length() < 1.0;
    shapes(harness)
        .into_iter()
        .filter(|(clip, _)| same(*clip, view))
        .map(|(_, shape)| shape)
        .collect()
}

/// The grid lines in the view: the only line segments drawn in its clip rectangle.
fn grid_lines(harness: &Harness<'static, Rig>) -> Vec<[Pos2; 2]> {
    in_view(harness)
        .into_iter()
        .filter_map(|shape| match shape {
            Shape::LineSegment { points, .. } => Some(points),
            _ => None,
        })
        .collect()
}

/// The text drawn in the view with its colour.
fn view_texts(harness: &Harness<'static, Rig>) -> Vec<(String, egui::Color32)> {
    in_view(harness)
        .into_iter()
        .filter_map(|shape| match shape {
            Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.galley.job.sections[0].format.color,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn the_wheel_zooms_about_the_pointer_and_keeps_the_texel_under_it() {
    let mut harness = uv_harness();
    let fitted = zoom(&harness);
    assert!(fitted > 6.0, "the fitted view starts large: {fitted}");
    let spot = at(&harness, 3.5, 11.5);
    hover(&mut harness, spot);
    assert_eq!(texel(&harness).as_deref(), Some("Texel 3, 11"));

    for notches in [1.0, 2.0, -1.0, 4.0] {
        let before = zoom(&harness);
        wheel(&mut harness, spot, notches);
        let after = zoom(&harness);
        assert_eq!(after > before, notches > 0.0, "{before} to {after}");
        assert_eq!(texel(&harness).as_deref(), Some("Texel 3, 11"));
    }
    // The range ends at 3 200 % and 25 %.
    wheel(&mut harness, spot, 40.0);
    assert!((zoom(&harness) - 32.0).abs() < 1e-3);
    wheel(&mut harness, spot, -80.0);
    assert!((zoom(&harness) - 0.25).abs() < 1e-3);

    // The wheel over something else leaves the view alone.
    let before = zoom(&harness);
    wheel(&mut harness, elsewhere(), 3.0);
    assert!((zoom(&harness) - before).abs() < 1e-3);
    assert_eq!(edits(&take(&mut harness)), Vec::<&Edit>::new());
}

#[test]
fn plus_and_minus_zoom_the_view_while_the_pointer_is_over_the_editor() {
    let mut harness = uv_harness();
    let fitted = zoom(&harness);
    let spot = at(&harness, 3.5, 11.5);
    hover(&mut harness, spot);
    harness.key_press(Key::Plus);
    harness.run_steps(2);
    let bigger = zoom(&harness);
    assert!(bigger > fitted, "{fitted} to {bigger}");
    assert_eq!(texel(&harness).as_deref(), Some("Texel 3, 11"));
    harness.key_press(Key::Minus);
    harness.run_steps(2);
    assert!(zoom(&harness) < bigger);

    // Over a toolbar or the numbers the keys zoom about the middle of the view.
    let toolbar = harness
        .get_by_label("Fit the texture to the view")
        .rect()
        .center();
    hover(&mut harness, toolbar);
    let before = zoom(&harness);
    harness.key_press(Key::Plus);
    harness.run_steps(2);
    assert!(zoom(&harness) > before);
    let numbers = harness.get_by_label("UV width").rect().center();
    hover(&mut harness, numbers);
    let before = zoom(&harness);
    harness.key_press(Key::Minus);
    harness.run_steps(2);
    assert!(zoom(&harness) < before);

    // With the pointer elsewhere the keys leave this view alone.
    let before = zoom(&harness);
    hover(&mut harness, elsewhere());
    harness.key_press(Key::Plus);
    harness.run_steps(2);
    assert!((zoom(&harness) - before).abs() < 1e-3);
}

#[test]
fn the_picture_pans_with_the_middle_button_the_right_button_and_space_with_the_left() {
    let mut harness = uv_harness();
    let middle = at(&harness, 8.0, 8.0);
    wheel(&mut harness, middle, 2.0);
    let scale = zoom(&harness);
    let spot = view(&harness).center();
    hover(&mut harness, spot);
    let start = texel(&harness).expect("a texel under the pointer");

    // Two texels to the right takes the texel two to the left under the pointer.
    let by = vec2(2.0 * scale, 0.0);
    drag_with(
        &mut harness,
        PointerButton::Middle,
        spot,
        spot + by,
        Modifiers::NONE,
    );
    hover(&mut harness, spot);
    let moved = texel(&harness).expect("a texel");
    assert_eq!(column(&moved), column(&start) - 2, "{start} then {moved}");
    assert_eq!(
        moved.rsplit(',').next(),
        start.rsplit(',').next(),
        "the row stays"
    );

    drag_with(
        &mut harness,
        PointerButton::Secondary,
        spot,
        spot + by,
        Modifiers::NONE,
    );
    hover(&mut harness, spot);
    assert_eq!(
        column(&texel(&harness).expect("a texel")),
        column(&start) - 4
    );

    harness.key_down(Key::Space);
    harness.run_steps(2);
    drag(&mut harness, spot, spot + by, Modifiers::NONE);
    harness.key_up(Key::Space);
    harness.run_steps(2);
    hover(&mut harness, spot);
    assert_eq!(
        column(&texel(&harness).expect("a texel")),
        column(&start) - 6
    );
    // Moving the picture asks nothing of the document.
    assert_eq!(edits(&take(&mut harness)), Vec::<&Edit>::new());

    // The picture can leave the view, but not all of it.
    let far = vec2(5000.0, 0.0);
    drag_with(
        &mut harness,
        PointerButton::Middle,
        spot,
        spot + far,
        Modifiers::NONE,
    );
    let edge = view(&harness).right_center() - vec2(1.0, 0.0);
    hover(&mut harness, edge);
    assert!(texel(&harness).is_some(), "some texture stays in the view");
}

#[test]
fn fit_and_the_readout_bring_the_whole_texture_back_and_a_new_texture_starts_fitted() {
    let mut harness = uv_harness();
    let fitted = zoom(&harness);
    let corner = at(&harness, 3.5, 3.5);
    wheel(&mut harness, corner, 3.0);
    assert!(zoom(&harness) > fitted);
    harness.get_by_label("Fit the texture to the view").click();
    harness.run_steps(4);
    assert!((zoom(&harness) - fitted).abs() < 1e-3);

    wheel(&mut harness, corner, 3.0);
    assert!(zoom(&harness) > fitted);
    let readout = harness
        .query_all_by(|node| {
            node.role() == Role::Button && node.label().is_some_and(|l| l.ends_with('%'))
        })
        .next()
        .expect("the readout");
    readout.click();
    harness.run_steps(4);
    assert!((zoom(&harness) - fitted).abs() < 1e-3);

    // Another face of the same texture keeps the view; a face on another texture starts fitted.
    wheel(&mut harness, corner, 3.0);
    let zoomed = zoom(&harness);
    harness.get_by_role_and_label(Role::Button, "Up").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "West")
        .click();
    harness.run_steps(4);
    assert!((zoom(&harness) - zoomed).abs() < 1e-3, "the view stays");
    harness.state_mut().fx.props.faces.as_mut().expect("faces")[Face::South.index()].texture =
        std::sync::Arc::from("particle");
    harness.get_by_role_and_label(Role::Button, "West").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "South")
        .click();
    harness.run_steps(4);
    assert!(
        (zoom(&harness) - fitted).abs() < 1e-3,
        "a new texture is fitted"
    );
}

#[test]
fn the_pixel_grid_shows_from_six_points_a_texel_and_only_the_lines_in_view() {
    let mut harness = uv_harness();
    // On by default: the 15 lines inside the texture each way.
    assert!(zoom(&harness) >= 6.0);
    assert_eq!(grid_lines(&harness).len(), 30);

    // Zoomed far in, only the lines in sight are drawn.
    let middle = at(&harness, 8.0, 8.0);
    wheel(&mut harness, middle, 20.0);
    let area = view(&harness);
    let lines = grid_lines(&harness);
    assert!(!lines.is_empty() && lines.len() < 30, "{}", lines.len());
    for [a, b] in &lines {
        for p in [a, b] {
            assert!(area.expand(0.5).contains(*p), "{p:?} is outside {area:?}");
        }
    }

    // Zoomed out, the lines go once a texel is under six points.
    harness.get_by_label("Fit the texture to the view").click();
    harness.run_steps(4);
    loop {
        let scale = zoom(&harness);
        assert_eq!(!grid_lines(&harness).is_empty(), scale >= 6.0, "at {scale}");
        if scale < 6.0 {
            break;
        }
        wheel(&mut harness, area.center(), -1.0);
    }
}

#[test]
fn the_grid_toggle_hides_the_lines_and_is_kept_in_the_preferences() {
    let mut harness = uv_harness();
    assert!(!grid_lines(&harness).is_empty());
    take(&mut harness);
    harness.state_mut().requests.clear();
    harness.get_by_label("Show the pixel grid").click();
    harness.run_steps(4);
    assert!(grid_lines(&harness).is_empty());
    let saved = |harness: &Harness<'static, Rig>| {
        harness
            .state()
            .requests
            .iter()
            .filter_map(|r| match r {
                UiRequest::PrefsChanged(prefs) => Some(prefs.uv_grid),
                _ => None,
            })
            .next_back()
    };
    assert_eq!(saved(&harness), Some(false));
    harness.get_by_label("Show the pixel grid").click();
    harness.run_steps(4);
    assert!(!grid_lines(&harness).is_empty());
    assert_eq!(saved(&harness), Some(true));
}

#[test]
fn every_face_on_the_texture_has_its_letter_and_a_face_that_is_off_shows_only_when_chosen() {
    let mut harness = uv_harness();
    let letters = |harness: &Harness<'static, Rig>| {
        let mut letters: Vec<String> = view_texts(harness)
            .into_iter()
            .map(|(text, _)| text)
            .filter(|text| text.len() == 1)
            .collect();
        letters.sort();
        letters
    };
    // Down is switched off in the fixture.
    assert_eq!(letters(&harness), ["E", "N", "S", "U", "W"]);
    harness.get_by_role_and_label(Role::Button, "Up").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "Down")
        .click();
    harness.run_steps(4);
    assert_eq!(letters(&harness), ["D", "E", "N", "S", "U", "W"]);
}

#[test]
fn a_click_chooses_a_face_shift_and_ctrl_add_and_take_out_and_empty_texture_keeps_the_choice() {
    let mut harness = uv_harness();
    chosen(&harness, "Up");
    let north = at(&harness, 10.0, 2.0);
    let south = at(&harness, 2.0, 6.0);
    let west = at(&harness, 6.0, 6.0);

    click(&mut harness, north, Modifiers::NONE);
    chosen(&harness, "North");
    click(&mut harness, south, Modifiers::SHIFT);
    chosen(&harness, "South +1");
    click(&mut harness, west, Modifiers::COMMAND);
    chosen(&harness, "West +2");
    // Ctrl on a chosen face takes it out; the first one left is the primary face.
    click(&mut harness, west, Modifiers::COMMAND);
    chosen(&harness, "North +1");
    // Empty texture keeps the choice.
    let empty = at(&harness, 14.0, 14.0);
    click(&mut harness, empty, Modifiers::NONE);
    chosen(&harness, "North +1");
    // The last face cannot be taken out.
    click(&mut harness, south, Modifiers::SHIFT);
    chosen(&harness, "North");
    click(&mut harness, north, Modifiers::SHIFT);
    chosen(&harness, "North");
    // A plain click on one of several chosen faces chooses only that one.
    click(&mut harness, south, Modifiers::SHIFT);
    click(&mut harness, south, Modifiers::NONE);
    chosen(&harness, "South");
    // None of this changes the document.
    assert_eq!(edits(&take(&mut harness)), Vec::<&Edit>::new());
}

/// A window with North and South chosen, then dragged from North by `by` texels with `mods`
/// held. Gives the window and the moves the drag asked for.
fn two_faces_dragged(by: egui::Vec2, mods: Modifiers) -> (Harness<'static, Rig>, Moves) {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let south = at(&harness, 2.0, 6.0);
    click(&mut harness, north, Modifiers::NONE);
    click(&mut harness, south, Modifiers::SHIFT);
    take(&mut harness);
    let to = north + by * zoom(&harness);
    drag(&mut harness, north, to, mods);
    let sent = moves(&take(&mut harness));
    (harness, sent)
}

#[test]
fn a_drag_of_two_chosen_faces_asks_for_steps_that_add_up_to_the_snapped_delta() {
    let both = vec![Face::North, Face::South];
    let faces = |harness: &Harness<'static, Rig>| {
        harness
            .state()
            .fx
            .props
            .faces
            .as_ref()
            .expect("faces")
            .clone()
    };

    // 2.4 across and 1.1 down snap to whole texels.
    let (harness, sent) = two_faces_dragged(vec2(2.4, 1.1), Modifiers::NONE);
    let (last, earlier) = sent.split_last().expect("moves");
    // The document had the steps all along: the last one has nothing left to ask.
    assert_eq!((&last.0, last.1, last.2), (&both, glam::Vec2::ZERO, true));
    assert!(
        earlier.iter().all(|m| !m.2 && m.1 != glam::Vec2::ZERO),
        "only the last ends it: {sent:?}"
    );
    assert!(sent.iter().all(|m| m.0 == both));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.0, 1.0));
    let after = faces(&harness);
    assert_eq!(after[Face::North.index()].uv, [10.0, 1.0, 14.0, 5.0]);
    assert_eq!(after[Face::South.index()].uv, [2.0, 5.0, 6.0, 9.0]);

    // Shift snaps to quarter texels, Alt not at all.
    let (_, sent) = two_faces_dragged(vec2(2.4, 1.1), Modifiers::SHIFT);
    assert!(sent.last().is_some_and(|m| m.2 && m.0 == both));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.5, 1.0));
    let (_, sent) = two_faces_dragged(vec2(2.4, 1.1), Modifiers::ALT);
    assert!(sent.last().is_some_and(|m| m.2));
    let total = travelled(&sent);
    assert!(
        (total.x - 2.4).abs() < 0.02 && (total.y - 1.1).abs() < 0.02,
        "{total:?}"
    );

    // The faces stay on the texture: North is at the top and South at the left edge.
    let (_, sent) = two_faces_dragged(vec2(-5.0, -5.0), Modifiers::NONE);
    assert!(sent.is_empty(), "no room to go there: {sent:?}");
}

#[test]
fn a_step_is_worked_out_against_where_the_faces_are_in_the_document() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let scale = zoom(&harness);
    click(&mut harness, north, Modifiers::NONE);
    take(&mut harness);
    press(&mut harness, north, PointerButton::Primary, Modifiers::NONE);
    hover(&mut harness, north + vec2(2.0, 0.0) * scale);
    let sent = moves(&take(&mut harness));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.0, 0.0));

    // The core cut that short: the face got only one of the two texels.
    harness.state_mut().fx.props.faces.as_mut().expect("faces")[Face::North.index()].uv =
        [9.0, 0.0, 13.0, 4.0];
    hover(&mut harness, north + vec2(3.0, 0.0) * scale);
    let sent = moves(&take(&mut harness));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.0, 0.0), "{sent:?}");
    release(
        &mut harness,
        north + vec2(3.0, 0.0) * scale,
        PointerButton::Primary,
    );
    let faces = harness
        .state()
        .fx
        .props
        .faces
        .as_ref()
        .expect("faces")
        .clone();
    assert_eq!(faces[Face::North.index()].uv, [11.0, 0.0, 15.0, 4.0]);
}

#[test]
fn a_drag_on_a_face_that_is_not_chosen_chooses_it_and_moves_it_alone() {
    let mut harness = uv_harness();
    let west = at(&harness, 6.0, 6.0);
    let to = west + vec2(1.0, 1.0) * zoom(&harness);
    drag(&mut harness, west, to, Modifiers::NONE);
    chosen(&harness, "West");
    let sent = moves(&take(&mut harness));
    assert!(sent.last().is_some_and(|m| m.2));
    assert!(sent.iter().all(|m| m.0 == [Face::West]));
    assert_eq!(travelled(&sent), glam::Vec2::new(1.0, 1.0));
}

#[test]
fn shift_held_before_the_press_drags_the_choice_in_quarter_texels_and_keeps_it() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let south = at(&harness, 2.0, 6.0);
    click(&mut harness, north, Modifiers::NONE);
    click(&mut harness, south, Modifiers::SHIFT);
    take(&mut harness);
    let by = vec2(2.4, 1.1) * zoom(&harness);

    // Shift is down before the press on South, a chosen face, and stays down for the drag.
    press(
        &mut harness,
        south,
        PointerButton::Primary,
        Modifiers::SHIFT,
    );
    for step in 1..=4 {
        hover(&mut harness, south + by * (step as f32 / 4.0));
    }
    release(&mut harness, south + by, PointerButton::Primary);
    chosen(&harness, "South +1");
    let sent = moves(&take(&mut harness));
    assert!(sent.iter().all(|m| m.0 == [Face::North, Face::South]));
    assert!(sent.last().is_some_and(|m| m.2));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.5, 1.0));
}

/// A rig whose Up face is 8 by 8 texels and chosen, with a 2 by 2 North face inside it.
fn nested_harness() -> Harness<'static, Rig> {
    let mut fx = Fixture::new();
    let faces = fx.props.faces.as_mut().expect("faces");
    for (face, uv) in [
        (Face::Up, [0.0, 0.0, 8.0, 8.0]),
        (Face::North, [2.0, 2.0, 4.0, 4.0]),
        (Face::South, [8.0, 8.0, 12.0, 12.0]),
        (Face::West, [12.0, 8.0, 16.0, 12.0]),
        (Face::East, [8.0, 12.0, 12.0, 16.0]),
        (Face::Down, [12.0, 12.0, 16.0, 16.0]),
    ] {
        faces[face.index()].uv = uv;
    }
    harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0))
}

#[test]
fn a_click_takes_the_smallest_face_under_the_pointer_and_a_drag_keeps_the_chosen_one() {
    let mut harness = nested_harness();
    let inside = at(&harness, 3.0, 3.0);
    let outside = at(&harness, 6.0, 6.0);
    chosen(&harness, "Up");

    // North lies inside the chosen Up: a click chooses it all the same.
    click(&mut harness, inside, Modifiers::NONE);
    chosen(&harness, "North");
    click(&mut harness, outside, Modifiers::NONE);
    chosen(&harness, "Up");
    // Shift adds it, and takes it out again.
    click(&mut harness, inside, Modifiers::SHIFT);
    chosen(&harness, "North +1");
    click(&mut harness, inside, Modifiers::SHIFT);
    chosen(&harness, "Up");

    // A drag from the same spot moves the chosen face, not the small one.
    take(&mut harness);
    let to = inside + vec2(1.0, 0.0) * zoom(&harness);
    drag(&mut harness, inside, to, Modifiers::NONE);
    chosen(&harness, "Up");
    let sent = moves(&take(&mut harness));
    assert!(sent.iter().all(|m| m.0 == [Face::Up]), "{sent:?}");
    assert_eq!(travelled(&sent), glam::Vec2::new(1.0, 0.0));
}

#[test]
fn a_corner_resizes_its_face_of_the_primary_element_in_whole_texels() {
    let mut harness = uv_harness();
    // Up is chosen: its bottom right corner is at (4, 4).
    let corner = at(&harness, 4.0, 4.0);
    let to = corner + vec2(1.4, 0.6) * zoom(&harness);
    drag(&mut harness, corner, to, Modifiers::NONE);
    let requests = take(&mut harness);
    let last = edits(&requests).into_iter().next_back().expect("an edit");
    assert_eq!(
        last,
        &Edit::FaceOfPrimary {
            face: Face::Up,
            change: FaceChange::Uv {
                uv: [0.0, 0.0, 5.0, 5.0],
                commit: true
            }
        }
    );
    assert!(moves(&requests).is_empty(), "no face moved");
}

#[test]
fn the_arrow_keys_move_the_chosen_faces_while_the_pointer_is_over_the_editor() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let south = at(&harness, 2.0, 6.0);
    click(&mut harness, north, Modifiers::NONE);
    click(&mut harness, south, Modifiers::SHIFT);
    take(&mut harness);
    let both = vec![Face::North, Face::South];

    harness.key_press(Key::ArrowRight);
    assert_eq!(
        moves(&take(&mut harness)),
        [(both.clone(), glam::Vec2::new(1.0, 0.0), true)]
    );
    harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(
        moves(&take(&mut harness)),
        [(both.clone(), glam::Vec2::new(0.0, 4.0), true)]
    );

    // Over a toolbar of the editor they work as well.
    let toolbar = harness
        .get_by_label("Fit the texture to the view")
        .rect()
        .center();
    hover(&mut harness, toolbar);
    harness.key_press(Key::ArrowLeft);
    assert_eq!(
        moves(&take(&mut harness)),
        [(both, glam::Vec2::new(-1.0, 0.0), true)]
    );

    // Away from the editor the keys do nothing here.
    hover(&mut harness, elsewhere());
    harness.key_press(Key::ArrowRight);
    assert!(moves(&take(&mut harness)).is_empty());
}

#[test]
fn an_arrow_key_during_a_drag_leaves_the_move_to_the_pointer() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let by = vec2(3.0, 0.0) * zoom(&harness);
    click(&mut harness, north, Modifiers::NONE);
    take(&mut harness);

    press(&mut harness, north, PointerButton::Primary, Modifiers::NONE);
    hover(&mut harness, north + by);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(2);
    release(&mut harness, north + by, PointerButton::Primary);
    let sent = moves(&take(&mut harness));
    assert_eq!(travelled(&sent), glam::Vec2::new(3.0, 0.0), "{sent:?}");
    let ends = sent.iter().filter(|m| m.2).count();
    assert_eq!(ends, 1, "one undo step: {sent:?}");
}

#[test]
fn escape_during_a_drag_ends_it_where_it_is() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let scale = zoom(&harness);
    click(&mut harness, north, Modifiers::NONE);
    take(&mut harness);

    press(&mut harness, north, PointerButton::Primary, Modifiers::NONE);
    hover(&mut harness, north + vec2(2.0, 0.0) * scale);
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    let sent = moves(&take(&mut harness));
    assert!(sent.last().is_some_and(|m| m.2), "it commits: {sent:?}");
    assert_eq!(travelled(&sent), glam::Vec2::new(2.0, 0.0));

    // The button is still down, but the drag is over: the pointer moves nothing more.
    hover(&mut harness, north + vec2(5.0, 0.0) * scale);
    release(
        &mut harness,
        north + vec2(5.0, 0.0) * scale,
        PointerButton::Primary,
    );
    assert!(moves(&take(&mut harness)).is_empty());
}

#[test]
fn a_drag_whose_release_the_editor_missed_still_ends_its_undo_step_and_lets_go_of_the_keys() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let by = vec2(2.0, 0.0) * zoom(&harness);
    click(&mut harness, north, Modifiers::NONE);
    take(&mut harness);
    press(&mut harness, north, PointerButton::Primary, Modifiers::NONE);
    hover(&mut harness, north + by);

    // The selection is cleared with the button down: the editor shows nothing when it comes up.
    harness.state_mut().fx.has_props = false;
    harness.run_steps(2);
    release(&mut harness, north + by, PointerButton::Primary);
    harness.state_mut().fx.has_props = true;
    harness.run_steps(3);
    let sent = moves(&take(&mut harness));
    assert_eq!(travelled(&sent), glam::Vec2::new(2.0, 0.0), "{sent:?}");
    assert!(sent.last().is_some_and(|m| m.2), "it commits: {sent:?}");

    // The editor holds nothing: the keys go to whatever the pointer is over.
    hover(&mut harness, elsewhere());
    harness.key_press(Key::ArrowRight);
    assert!(moves(&take(&mut harness)).is_empty());
}

#[test]
fn a_toolbar_action_on_several_chosen_faces_is_one_request() {
    let mut harness = uv_harness();
    let north = at(&harness, 10.0, 2.0);
    let south = at(&harness, 2.0, 6.0);
    take(&mut harness);
    // One face is chosen: the action names it.
    harness.get_by_label("Rotate the UV a quarter turn").click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::Faces {
            faces: vec![Face::Up],
            change: FaceChange::RotateUv
        }]
    );

    click(&mut harness, north, Modifiers::NONE);
    click(&mut harness, south, Modifiers::SHIFT);
    take(&mut harness);
    for (tip, change) in [
        ("Apply auto UV", FaceChange::AutoUv),
        ("Rotate the UV a quarter turn", FaceChange::RotateUv),
        ("Mirror the UV left to right", FaceChange::FlipU),
        ("Flip the UV top to bottom", FaceChange::FlipV),
    ] {
        harness.get_by_label(tip).click();
        assert_eq!(
            edits(&take(&mut harness)),
            [&Edit::Faces {
                faces: vec![Face::North, Face::South],
                change
            }],
            "{tip}"
        );
    }
}

#[test]
fn the_numbers_of_the_face_send_their_edits_and_a_turn() {
    let mut harness = uv_harness();
    // Up is the primary face: X 0, Y 0, 4 wide and 4 high.
    for (name, text, expected) in [
        ("UV width", "6", [0.0, 0.0, 6.0, 4.0]),
        ("UV height", "2", [0.0, 0.0, 4.0, 2.0]),
        ("UV X", "3", [3.0, 0.0, 7.0, 4.0]),
        ("UV Y", "5", [0.0, 5.0, 4.0, 9.0]),
    ] {
        harness.get_by_label(name).click();
        harness.run_steps(3);
        harness
            .get_by_role_and_label(Role::TextInput, name)
            .type_text(text);
        harness.run_steps(2);
        harness.key_press(Key::Enter);
        let requests = take(&mut harness);
        assert!(
            edits(&requests).iter().any(|e| matches!(
                e,
                Edit::FaceOfPrimary { face: Face::Up, change: FaceChange::Uv { uv, commit: true } } if *uv == expected
            )),
            "{name}: {requests:?}"
        );
    }
    harness
        .get_by_role_and_label(Role::RadioButton, "180\u{b0}")
        .click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::FaceOfPrimary {
            face: Face::Up,
            change: FaceChange::Rotation(180)
        }]
    );
}

/// The colour of the first text of a shape that holds `needle`.
fn text_colour(shape: &Shape, needle: &str) -> Option<egui::Color32> {
    match shape {
        Shape::Text(text) if text.galley.text().contains(needle) => {
            Some(text.galley.job.sections[0].format.color)
        }
        Shape::Vec(inner) => inner.iter().find_map(|shape| text_colour(shape, needle)),
        _ => None,
    }
}

#[test]
fn the_density_is_in_texels_to_a_block_and_turns_the_warning_colour_when_the_axes_differ() {
    // An element of 8 units each way: its up face is half a block across and deep.
    let density = |uv: [f32; 4]| {
        let mut fx = Fixture::new();
        fx.props.size = glam::Vec3::splat(8.0);
        fx.props.faces.as_mut().expect("faces")[Face::Up.index()].uv = uv;
        let harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
        let line = harness
            .get_by_label_contains("texels per block")
            .accesskit_node()
            .value()
            .expect("a label");
        let colour = harness
            .output()
            .shapes
            .iter()
            .find_map(|clipped| text_colour(&clipped.shape, "texels per block"))
            .expect("the line is drawn");
        (line, colour, cuboid_ui_kit::Tokens::of(&harness.ctx).warn)
    };
    let (line, colour, warn) = density([0.0, 0.0, 8.0, 8.0]);
    assert_eq!(line, "Density 16 x 16 texels per block");
    assert_ne!(colour, warn);

    // Twice as many texels across as down: the texture is stretched.
    let (line, colour, warn) = density([0.0, 0.0, 8.0, 4.0]);
    assert_eq!(line, "Density 16 x 8 texels per block");
    assert_eq!(colour, warn);
}

#[test]
fn all_uvs_outlines_the_faces_of_the_other_elements_on_the_same_texture() {
    // Whether the view outlines, without filling it, the texels `uv` of the fitted texture.
    let outlined = |harness: &Harness<'static, Rig>, uv: [f32; 4]| {
        let (min, max) = (at(harness, uv[0], uv[1]), at(harness, uv[2], uv[3]));
        let near = |a: Pos2, b: Pos2| (a - b).length() < 1.5;
        in_view(harness).into_iter().any(|shape| {
            matches!(shape, Shape::Rect(r) if r.fill == egui::Color32::TRANSPARENT
                && r.stroke.width > 0.0 && near(r.rect.min, min) && near(r.rect.max, max))
        })
    };
    let lantern = [[0.0, 8.0, 4.0, 12.0], [8.0, 8.0, 12.0, 12.0]];
    let particle = [0.0, 12.0, 4.0, 16.0];
    let mut fx = Fixture::new();
    fx.uv_outlines = [
        ("lantern", lantern[0]),
        ("lantern", lantern[1]),
        ("particle", particle),
    ]
    .map(|(texture, uv)| cuboid_ui::UvOutline {
        texture: std::sync::Arc::from(texture),
        uv,
    })
    .into();
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    assert!(lantern.iter().all(|uv| !outlined(&harness, *uv)));
    // The app lists the faces of the other elements only while All UVs is on.
    assert!(!harness.state().ui.uv_outlines_wanted());
    harness.get_by_label("Show all UVs on this texture").click();
    harness.run_steps(4);
    assert!(harness.state().ui.uv_outlines_wanted());
    assert!(lantern.iter().all(|uv| outlined(&harness, *uv)));
    assert!(
        !outlined(&harness, particle),
        "the particle face is on another texture"
    );
    harness.get_by_label("Show all UVs on this texture").click();
    harness.run_steps(4);
    assert!(!harness.state().ui.uv_outlines_wanted());
    assert!(lantern.iter().all(|uv| !outlined(&harness, *uv)));
}

#[test]
fn a_face_with_a_missing_texture_shows_its_message_right_under_the_first_toolbar() {
    let mut fx = Fixture::new();
    fx.props.faces.as_mut().expect("faces")[Face::Up.index()].texture =
        std::sync::Arc::from("gone");
    let harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    let toolbar = harness.get_by_label("Apply auto UV").rect();
    let message = harness.get_by_label("The face uses a texture that does not exist.");
    // No band for the view controls: the gap is a spacing, not a row of the toolbar's height.
    let gap = message.rect().top() - toolbar.bottom();
    assert!(gap < cuboid_ui_kit::metrics::height::BAR, "{gap}");
    assert!(
        harness
            .query_by_label("Fit the texture to the view")
            .is_none()
    );
}

#[test]
fn the_pan_of_the_uv_view_with_space_does_not_pan_a_3d_view_under_the_pointer() {
    use cuboid_ui::ViewportEvent;
    let pans = |requests: &[UiRequest]| {
        requests
            .iter()
            .filter(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Pan { .. })))
            .count()
    };
    let mut harness = harness_with(
        rig(prefs(ComingFrom::Cubik), Fixture::new()),
        vec2(1600.0, 900.0),
    );
    let [x, y, w, h] = harness.state().view.expect("a view is drawn");
    let over_3d = Pos2::new(x + w / 2.0, y + h / 2.0);
    harness.key_down(Key::Space);
    harness.run_steps(2);
    take(&mut harness);

    // With Space held the camera pans as the pointer moves over a view.
    hover(&mut harness, over_3d);
    hover(&mut harness, over_3d + vec2(20.0, 0.0));
    assert!(pans(&take(&mut harness)) > 0);

    // The UV view holds the pointer: it pans itself and leaves the 3D view alone.
    let from = view(&harness).center();
    press(&mut harness, from, PointerButton::Primary, Modifiers::NONE);
    hover(&mut harness, over_3d);
    hover(&mut harness, over_3d + vec2(20.0, 0.0));
    release(
        &mut harness,
        over_3d + vec2(20.0, 0.0),
        PointerButton::Primary,
    );
    assert_eq!(pans(&take(&mut harness)), 0);
}
