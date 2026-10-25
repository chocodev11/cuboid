use std::sync::Arc;
use std::time::Instant;

use cuboid_assets::mc_versions::McVersion;
use cuboid_model::{
    DisplaySlotId, Document, Element, ElementType, FaceFlags, Label, ModelKind, ObjId,
    OverrideCase, TextureList,
};
use glam::{Vec2, Vec3};
use proptest::prelude::*;

use crate::rules::{RULES, cube_with, image, texture};
use crate::*;

fn v(major: u16, minor: u16, patch: u16) -> McVersion {
    McVersion::new(major, minor, patch)
}

fn block(version: McVersion) -> Profile {
    Profile::new(Target::JavaBlock { version })
}

fn block_1_21_4() -> Profile {
    block(v(1, 21, 4))
}

/// A document with the self-test texture and one clean cube.
fn clean() -> (Document, ObjId) {
    let mut doc = Document::new(TextureList::default());
    let id = cube_with(&mut doc, |_| {});
    (doc, id)
}

fn edit(doc: &mut Document, id: ObjId, f: impl FnOnce(&mut Element)) {
    if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
        f(el);
    }
}

fn codes(issues: &[Issue]) -> Vec<&'static str> {
    issues.iter().map(|issue| issue.code).collect()
}

fn find<'a>(issues: &'a [Issue], code: &str) -> Option<&'a Issue> {
    issues.iter().find(|issue| issue.code == code)
}

#[test]
fn a_clean_model_has_no_issue() {
    let (doc, _) = clean();
    for target in [
        Target::JavaBlock {
            version: v(1, 21, 4),
        },
        Target::JavaBlock {
            version: v(1, 12, 2),
        },
        Target::JavaBlock {
            version: v(26, 3, 0),
        },
        Target::Bbmodel { java_block: true },
        Target::Bbmodel { java_block: false },
        Target::Cubik,
    ] {
        let issues = validate(&doc, &Profile::new(target));
        assert!(issues.is_empty(), "{target:?}: {issues:?}");
    }
}

#[test]
fn bounds_follow_the_json_range_and_the_inflate() {
    let (mut doc, id) = clean();
    // Unrotated: from = (p.x, p.y, -p.z), to = from + s.
    edit(&mut doc, id, |el| {
        el.base.p = Vec3::new(-16.0, -16.0, 16.0);
        el.base.o = el.base.p;
        el.base.s = Vec3::splat(48.0);
    });
    assert!(validate(&doc, &block_1_21_4()).is_empty());

    let mut inflated = block_1_21_4();
    inflated.inflate = 0.5;
    let issues = validate(&doc, &inflated);
    let issue = find(&issues, GEO_BOUNDS).expect("inflate pushes it out");
    assert_eq!(issue.severity, Severity::Error);
    assert_eq!(issue.objects, vec![id]);
    assert_eq!(
        issue.fix,
        Some(vec![FixOp::ClampIntoBounds {
            id,
            min: -15.5,
            max: 31.5
        }])
    );

    edit(&mut doc, id, |el| el.base.p.x = 40.0);
    assert!(find(&validate(&doc, &block_1_21_4()), GEO_BOUNDS).is_some());
    // Free .bbmodel takes any box.
    assert!(validate(&doc, &Profile::new(Target::Bbmodel { java_block: false })).is_empty());
}

#[test]
fn a_rotated_element_is_judged_by_its_unrotated_box() {
    let (mut doc, id) = clean();
    // Place a 16-wide box rotated 45 degrees about its centre pivot; the stored corner moves but
    // the JSON box does not.
    let b = cuboid_model::mc::place(
        &cuboid_model::mc::McBox {
            from: Vec3::new(0.0, 0.0, 0.0),
            to: Vec3::new(16.0, 16.0, 16.0),
            pivot: Vec3::new(8.0, 8.0, 8.0),
            rotation: Vec3::new(0.0, 45.0, 0.0),
        },
        cuboid_model::mc::Rigid::IDENTITY,
    );
    edit(&mut doc, id, |el| {
        el.base.p = b.p;
        el.base.s = b.s;
        el.base.o = b.o;
        el.base.e = b.e;
    });
    let issues = validate(&doc, &block_1_21_4());
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn rotation_follows_the_version_tier() {
    let (mut doc, id) = clean();
    let at = |e: Vec3, doc: &mut Document| edit(doc, id, |el| el.base.e = e);

    at(Vec3::new(22.5, 0.0, 0.0), &mut doc);
    assert!(validate(&doc, &block_1_21_4()).is_empty());
    at(Vec3::new(337.5, 0.0, 0.0), &mut doc);
    assert!(validate(&doc, &block_1_21_4()).is_empty());

    at(Vec3::new(10.0, 0.0, 0.0), &mut doc);
    let issues = validate(&doc, &block_1_21_4());
    let issue = find(&issues, GEO_ROTATION).expect("10 degrees");
    assert_eq!(issue.severity, Severity::Error);
    assert_eq!(
        issue.fix,
        Some(vec![FixOp::SetRotation {
            id,
            rotation: Vec3::ZERO
        }])
    );

    at(Vec3::new(30.0, 40.0, 0.0), &mut doc);
    let issues = validate(&doc, &block_1_21_4());
    assert!(find(&issues, GEO_ROTATION).is_some_and(|i| i.message.contains("more than one axis")));

    // 1.21.6: any single-axis angle up to 45, but the rule is unverified: warning only.
    at(Vec3::new(0.0, 30.0, 0.0), &mut doc);
    assert!(validate(&doc, &block(v(1, 21, 6))).is_empty());
    at(Vec3::new(0.0, 60.0, 0.0), &mut doc);
    let issues = validate(&doc, &block(v(1, 21, 8)));
    let issue = find(&issues, GEO_ROTATION).expect("60 degrees");
    assert_eq!(issue.severity, Severity::Warning);
    assert!(issue.hint.contains("unverified"));
    assert_eq!(
        issue.fix,
        Some(vec![FixOp::SetRotation {
            id,
            rotation: Vec3::new(0.0, 45.0, 0.0)
        }])
    );

    // 1.21.11 and later: any rotation.
    at(Vec3::new(10.0, 70.0, 100.0), &mut doc);
    assert!(validate(&doc, &block(v(1, 21, 11))).is_empty());
    assert!(validate(&doc, &block(v(26, 3, 0))).is_empty());
    // An unknown version falls back to the strict rule, as a warning.
    let issues = validate(&doc, &block(v(1, 0, 0)));
    assert_eq!(codes(&issues), [VERSION_UNKNOWN, GEO_ROTATION]);
    assert_eq!(issues[1].severity, Severity::Warning);
}

#[test]
fn an_inverted_cube_is_info_only() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| el.base.s.x = -1.0);
    let issues = validate(&doc, &block_1_21_4());
    assert_eq!(codes(&issues), [GEO_INVERTED]);
    assert_eq!(issues[0].severity, Severity::Info);
}

#[test]
fn faces_without_a_texture_or_with_a_dangling_one() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| {
        el.faces[0].texture = Label::from("");
        el.faces[1].texture = Label::from("nope");
        el.faces[2].active = false;
        el.faces[2].texture = Label::from("also_nope");
    });
    let issues = validate(&doc, &block_1_21_4());
    let none = find(&issues, TEX_FACE_NONE).expect("empty label");
    assert!(none.message.contains("down"));
    assert_eq!(none.fix.as_ref().map(Vec::len), Some(1));
    let dangling = find(&issues, TEX_UNRESOLVED).expect("dangling label");
    assert!(dangling.message.contains("up -> #nope") && !dangling.message.contains("also_nope"));
    assert!(matches!(
        dangling.fix.as_deref(),
        Some([FixOp::AssignTexture { label, .. }]) if &**label == "selftest_ok"
    ));
}

#[test]
fn unused_textures_and_shortcuts() {
    let (mut doc, _) = clean();
    let img = image(16, 16, false);
    doc.textures
        .entries
        .push(texture("particle", "block/particle", img.clone()));
    doc.textures
        .entries
        .push(texture("spare", "block/spare", img.clone()));
    // A shortcut that a face uses makes its target used.
    doc.textures
        .entries
        .push(texture("alias", "#target", img.clone()));
    doc.textures
        .entries
        .push(texture("target", "block/target", img));
    let id = doc.elements.ids()[0];
    edit(&mut doc, id, |el| {
        el.faces[1].texture = Label::from("alias")
    });
    let issues = validate(&doc, &block_1_21_4());
    let unused: Vec<&str> = issues
        .iter()
        .filter(|i| i.code == TEX_UNUSED)
        .map(|i| i.message.as_str())
        .collect();
    assert_eq!(unused, ["texture \"spare\" is used by no visible face"]);
    let fix = &find(&issues, TEX_UNUSED).expect("spare").fix;
    assert_eq!(
        fix,
        &Some(vec![FixOp::RemoveTexture {
            label: Label::from("spare")
        }])
    );
}

#[test]
fn a_shortcut_to_nothing_is_unresolved() {
    let (mut doc, _) = clean();
    doc.textures
        .entries
        .push(texture("alias", "#ghost", image(16, 16, false)));
    let issues = validate(&doc, &block_1_21_4());
    assert!(find(&issues, TEX_UNRESOLVED).is_some_and(|i| i.message.contains("#ghost")));
}

#[test]
fn texture_paths_follow_the_namespace_and_folder_rules() {
    let (mut doc, _) = clean();
    let img = image(16, 16, false);
    doc.textures
        .entries
        .push(texture("upper", "Block/Stone", img.clone()));
    doc.textures
        .entries
        .push(texture("old", "blocks/stone", img.clone()));
    doc.textures
        .entries
        .push(texture("ns", "mymod:items/gem", img.clone()));
    doc.textures.entries.push(texture("none", "", img.clone()));
    doc.textures
        .entries
        .push(texture("sword", "item/sword", img));
    let issues = validate(&doc, &block_1_21_4());
    assert!(find(&issues, ID_CHARS).is_some_and(|i| i.message.contains("upper-case")));
    let layout: Vec<&Issue> = issues.iter().filter(|i| i.code == TEX_LAYOUT).collect();
    assert_eq!(layout.len(), 2);
    assert_eq!(
        layout[0].fix,
        Some(vec![FixOp::SetTexturePath {
            label: Label::from("old"),
            path: "block/stone".to_owned(),
        }])
    );
    assert!(matches!(
        layout[1].fix.as_deref(),
        Some([FixOp::SetTexturePath { path, .. }]) if path == "mymod:item/gem"
    ));
    assert!(find(&issues, TEX_NO_PATH).is_some());

    // Before 1.13 the plural folders are the right ones.
    let old = validate(&doc, &block(v(1, 12, 2)));
    let wrong: Vec<&str> = old
        .iter()
        .filter(|i| i.code == TEX_LAYOUT)
        .map(|i| i.message.as_str())
        .collect();
    assert_eq!(wrong.len(), 1, "{wrong:?}");
    assert!(wrong[0].contains("item/sword") && wrong[0].contains("reads items/"));
}

#[test]
fn a_texture_that_is_not_square_is_a_warning() {
    let (mut doc, _) = clean();
    doc.textures
        .entries
        .push(texture("wide", "block/wide", image(16, 8, false)));
    let issues = validate(&doc, &block_1_21_4());
    assert!(find(&issues, TEX_NOT_SQUARE).is_some_and(|i| i.severity == Severity::Warning));
}

#[test]
fn semi_transparent_pixels_on_cutout_faces() {
    let (mut doc, id) = clean();
    doc.textures
        .entries
        .push(texture("glass", "block/glass", image(16, 16, true)));
    edit(&mut doc, id, |el| {
        el.faces[0].texture = Label::from("glass");
    });
    let mut profile = block_1_21_4();
    let issues = validate(&doc, &profile);
    assert!(find(&issues, TEX_ALPHA_CUTOUT).is_some_and(|i| i.message.contains("down")));
    profile.render_type = RenderType::Translucent;
    assert!(find(&validate(&doc, &profile), TEX_ALPHA_CUTOUT).is_none());

    // Without a clear texel the face is not cutout in Auto mode.
    let mut soft = RgbaBufFill::soft();
    doc.textures
        .entries
        .push(texture("soft", "block/soft", soft.take()));
    edit(&mut doc, id, |el| {
        el.faces[0].texture = Label::from("soft");
    });
    profile.render_type = RenderType::Auto;
    assert!(find(&validate(&doc, &profile), TEX_ALPHA_CUTOUT).is_none());
    profile.render_type = RenderType::Cutout;
    assert!(find(&validate(&doc, &profile), TEX_ALPHA_CUTOUT).is_some());
}

/// A texture whose texels are all half transparent.
struct RgbaBufFill(Option<Arc<cuboid_model::image::Image>>);

impl RgbaBufFill {
    fn soft() -> Self {
        let mut buf = cuboid_model::image::RgbaBuf::new(16, 16);
        for px in buf.pixels.as_chunks_mut::<4>().0 {
            *px = [10, 20, 30, 128];
        }
        Self(Some(Arc::new(cuboid_model::image::Image::from_rgba(buf))))
    }

    fn take(&mut self) -> Arc<cuboid_model::image::Image> {
        self.0.take().expect("taken once")
    }
}

#[test]
fn colour_and_element_budgets_are_configurable() {
    let (mut doc, _) = clean();
    let mut profile = block_1_21_4();
    assert!(validate(&doc, &profile).is_empty(), "no budget by default");
    profile.max_colors = Some(0);
    profile.max_elements = Some(0);
    let issues = validate(&doc, &profile);
    assert!(find(&issues, TEX_COLORS).is_some());
    assert!(find(&issues, BUDGET_ELEMENTS).is_some());
    profile.max_colors = Some(1);
    profile.max_elements = Some(1);
    assert!(validate(&doc, &profile).is_empty());
    cube_with(&mut doc, |_| {});
    assert!(find(&validate(&doc, &profile), BUDGET_ELEMENTS).is_some());
}

#[test]
fn uv_outside_the_texture_has_a_clamping_fix() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| {
        el.faces[3]
            .uv
            .set_unclamped(Vec2::new(3.0, 0.5), Vec2::new(4.0, 1.0));
    });
    let issues = validate(&doc, &block_1_21_4());
    let issue = find(&issues, UV_RANGE).expect("uv");
    assert!(issue.message.contains("south"));
    assert!(matches!(
        issue.fix.as_deref(),
        Some([FixOp::SetUv { id: i, .. }]) if *i == id
    ));
    // The setter's own range [-1, 2] folds back, so a normal edit never trips the rule.
    edit(&mut doc, id, |el| {
        el.faces[3]
            .uv
            .set(Vec2::new(-0.5, 0.0), Vec2::new(0.5, 1.0));
    });
    assert!(find(&validate(&doc, &block_1_21_4()), UV_RANGE).is_none());
}

#[test]
fn cull_must_be_one_side() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| {
        el.faces[0].cull = FaceFlags::DOWN;
        el.faces[1].cull = FaceFlags::UP | FaceFlags::DOWN;
    });
    let issues = validate(&doc, &block_1_21_4());
    let issue = find(&issues, FACE_CULL).expect("two sides");
    assert!(issue.message.contains("up") && !issue.message.contains("down"));
}

#[test]
fn display_values_the_game_clamps_are_warnings() {
    let (mut doc, _) = clean();
    {
        let slot = &mut Arc::make_mut(&mut doc.display)[DisplaySlotId::Gui];
        slot.enabled = true;
        slot.translation = Vec3::new(90.0, -100.0, 80.0);
        slot.scale = Vec3::new(5.0, 4.0, -6.0);
    }
    assert!(
        find(&validate(&doc, &block_1_21_4()), DISPLAY_RANGE).is_none(),
        "display is not exported while include_display is off"
    );
    Arc::make_mut(&mut doc.props).minecraft.include_display = true;
    let issues = validate(&doc, &block_1_21_4());
    let issue = find(&issues, DISPLAY_RANGE).expect("clamped");
    assert_eq!(issue.severity, Severity::Warning);
    assert_eq!(
        issue.fix,
        Some(vec![FixOp::SetDisplay {
            slot: DisplaySlotId::Gui,
            translation: Vec3::new(80.0, -80.0, 80.0),
            scale: Vec3::new(4.0, 4.0, -4.0),
        }])
    );
    // Inside the limits: fine.
    {
        let slot = &mut Arc::make_mut(&mut doc.display)[DisplaySlotId::Gui];
        slot.translation = Vec3::new(80.0, -80.0, 0.0);
        slot.scale = Vec3::new(4.0, 1.0, 1.0);
    }
    assert!(find(&validate(&doc, &block_1_21_4()), DISPLAY_RANGE).is_none());
}

#[test]
fn model_kind_and_emptiness() {
    let (mut doc, _) = clean();
    let item = Profile::new(Target::JavaItem {
        version: v(1, 21, 4),
    });
    let issues = validate(&doc, &item);
    let kind = find(&issues, MODEL_KIND).expect("block model, item target");
    assert_eq!(kind.severity, Severity::Warning);
    assert_eq!(
        kind.fix,
        Some(vec![FixOp::SetModelKind {
            kind: ModelKind::Item
        }])
    );

    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Entity;
    let issues = validate(&doc, &block_1_21_4());
    assert_eq!(
        find(&issues, MODEL_KIND).map(|i| i.severity),
        Some(Severity::Error)
    );

    Arc::make_mut(&mut doc.props).general.exclude_elements = true;
    assert!(find(&validate(&doc, &block_1_21_4()), MODEL_EMPTY).is_some());
}

#[test]
fn hidden_elements_are_skipped_when_the_export_excludes_them() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| {
        el.base.hidden = true;
        el.base.p.x = 500.0;
    });
    assert!(find(&validate(&doc, &block_1_21_4()), GEO_BOUNDS).is_some());
    Arc::make_mut(&mut doc.props).general.exclude_hidden = true;
    let issues = validate(&doc, &block_1_21_4());
    assert!(find(&issues, GEO_BOUNDS).is_none());
    assert!(find(&issues, MODEL_EMPTY).is_some());
}

#[test]
fn overrides_need_an_item_definition_from_1_21_4() {
    let (mut doc, _) = clean();
    doc.overrides = Arc::from(vec![OverrideCase::default()]);
    assert!(find(&validate(&doc, &block_1_21_4()), MODEL_OVERRIDES).is_some());
    assert!(find(&validate(&doc, &block(v(1, 21, 3))), MODEL_OVERRIDES).is_none());
}

fn grid(n: usize, step: f32, size: f32) -> Document {
    let mut doc = Document::new(TextureList::default());
    let label = crate::rules::base_texture(&mut doc);
    let side = (n as f32).sqrt().ceil() as usize;
    for i in 0..n {
        let id = crate::rules::add_cube(&mut doc, &label);
        let (x, z) = ((i % side) as f32 * step, (i / side) as f32 * step);
        if let Some(el) = doc.elements.edit(id, &mut doc.journal) {
            el.base.p = Vec3::new(x - 16.0, 0.0, -z + 16.0);
            el.base.o = el.base.p;
            el.base.s = Vec3::splat(size);
        }
    }
    doc
}

#[test]
fn coplanar_overlapping_faces_are_found_and_neighbours_are_not() {
    // A floor of touching tiles: no fight.
    let mut doc = grid(64, 2.0, 2.0);
    assert!(find(&validate(&doc, &block_1_21_4()), GEO_ZFIGHT).is_none());
    // Two cubes in one place fight on every face; the pair is one issue.
    let a = doc.elements.ids()[0];
    let label = Label::from("selftest_ok");
    let b = crate::rules::add_cube(&mut doc, &label);
    edit(&mut doc, b, |el| {
        el.base.p = Vec3::new(-16.0, 0.0, 16.0);
        el.base.o = el.base.p;
        el.base.s = Vec3::splat(2.0);
    });
    let issues = validate(&doc, &block_1_21_4());
    let fights: Vec<&Issue> = issues.iter().filter(|i| i.code == GEO_ZFIGHT).collect();
    assert_eq!(fights.len(), 1);
    assert_eq!(fights[0].objects, vec![a, b]);
    assert!(fights[0].message.contains("6 coplanar"));

    // Rotated elements are skipped.
    edit(&mut doc, b, |el| el.base.e = Vec3::new(0.0, 22.5, 0.0));
    assert!(find(&validate(&doc, &block_1_21_4()), GEO_ZFIGHT).is_none());
}

#[test]
fn opposite_faces_on_one_plane_do_not_fight() {
    let mut doc = grid(2, 1.0, 1.0);
    // The east face of the first cube and the west face of a cube next to it share a plane but
    // look away from each other.
    let ids = doc.elements.ids().to_vec();
    edit(&mut doc, ids[1], |el| {
        el.base.p = Vec3::new(-15.0, 0.0, 16.0);
        el.base.o = el.base.p;
    });
    assert!(find(&validate(&doc, &block_1_21_4()), GEO_ZFIGHT).is_none());
}

#[test]
fn mixed_texel_density_is_reported_per_element() {
    let mut doc = grid(4, 3.0, 2.0);
    let ids = doc.elements.ids().to_vec();
    edit(&mut doc, ids[3], |el| {
        for face in &mut el.faces {
            face.uv.set(Vec2::ZERO, Vec2::splat(0.1));
        }
    });
    let issues = validate(&doc, &block_1_21_4());
    let dens: Vec<&Issue> = issues.iter().filter(|i| i.code == UV_DENSITY).collect();
    assert_eq!(dens.len(), 1);
    assert_eq!(dens[0].objects, vec![ids[3]]);
    let mut lax = block_1_21_4();
    lax.texel_ratio = 100.0;
    assert!(find(&validate(&doc, &lax), UV_DENSITY).is_none());
}

#[test]
fn non_finite_numbers_are_an_error_for_every_target() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| el.base.s.y = f32::INFINITY);
    for target in [
        Target::Cubik,
        Target::Bbmodel { java_block: false },
        Target::Bbmodel { java_block: true },
        Target::JavaBlock {
            version: v(1, 21, 4),
        },
    ] {
        let issues = validate(&doc, &Profile::new(target));
        assert_eq!(codes(&issues), [NUM_NONFINITE], "{target:?}");
    }
}

#[test]
fn cubik_reports_only_what_it_cannot_store() {
    let (mut doc, id) = clean();
    // Everything Java rejects is storable.
    edit(&mut doc, id, |el| {
        el.base.p = Vec3::new(300.0, 0.0, 0.0);
        el.base.e = Vec3::new(10.0, 20.0, 30.0);
        el.faces[0].texture = Label::from("missing");
    });
    let cubik = Profile::new(Target::Cubik);
    assert!(validate(&doc, &cubik).is_empty());

    doc.overrides = Arc::from(vec![OverrideCase::default()]);
    edit(&mut doc, id, |el| {
        el.faces[1]
            .uv
            .set_unclamped(Vec2::new(-0.25, 0.0), Vec2::new(0.5, 1.0));
    });
    Arc::make_mut(&mut doc.display)[DisplaySlotId::Head].scale.z = 0.0;
    let issues = validate(&doc, &cubik);
    assert_eq!(
        codes(&issues),
        [CUBIK_OVERRIDES, CUBIK_SCALE_ZERO, CUBIK_UV_FOLDED]
    );
}

#[test]
fn bbmodel_java_block_takes_the_rules_every_version_accepts() {
    let (mut doc, id) = clean();
    let target = Target::Bbmodel { java_block: true };
    assert!(fits(&doc, &target));
    edit(&mut doc, id, |el| el.base.e = Vec3::new(0.0, 22.5, 0.0));
    assert!(fits(&doc, &target));
    edit(&mut doc, id, |el| el.base.e = Vec3::new(0.0, 30.0, 0.0));
    assert!(!fits(&doc, &target));
    assert!(fits(&doc, &Target::Bbmodel { java_block: false }));
    assert!(fits(
        &doc,
        &Target::JavaBlock {
            version: v(1, 21, 11)
        }
    ));
    assert!(!fits(
        &doc,
        &Target::JavaBlock {
            version: v(1, 21, 4)
        }
    ));
    // Warnings about style do not stop a fit: an unused texture is fine.
    edit(&mut doc, id, |el| el.base.e = Vec3::ZERO);
    doc.textures
        .entries
        .push(texture("spare", "block/spare", image(16, 16, false)));
    assert!(fits(&doc, &target));
    edit(&mut doc, id, |el| el.base.p.x = 90.0);
    assert!(!fits(&doc, &target));
}

#[test]
fn issue_caps_are_per_code_with_one_summary() {
    let mut doc = grid(30, 1.5, 1.0);
    for id in doc.elements.ids().to_vec() {
        edit(&mut doc, id, |el| el.base.e = Vec3::new(5.0, 0.0, 0.0));
    }
    let mut profile = block_1_21_4();
    profile.max_issues_per_rule = 10;
    let issues = validate(&doc, &profile);
    let rotation: Vec<&Issue> = issues.iter().filter(|i| i.code == GEO_ROTATION).collect();
    assert_eq!(rotation.len(), 11);
    assert_eq!(rotation.last().map(|i| i.severity), Some(Severity::Info));
    assert!(
        rotation
            .last()
            .is_some_and(|i| i.message.starts_with("20 more"))
    );
    // Errors come first.
    assert_eq!(issues.first().map(|i| i.severity), Some(Severity::Error));
}

#[test]
fn switches_skip_rules() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| el.base.e = Vec3::new(5.0, 0.0, 0.0));
    let mut profile = block_1_21_4();
    profile.disabled = vec![GEO_ROTATION.to_owned()];
    assert!(validate(&doc, &profile).is_empty());
    profile.disabled.clear();
    profile.min_severity = Severity::Error;
    assert_eq!(codes(&validate(&doc, &profile)), [GEO_ROTATION]);
    profile.only = vec![GEO_BOUNDS];
    assert!(validate(&doc, &profile).is_empty());
}

fn pack_profile(version: McVersion, pack: PackInfo) -> Profile {
    let mut profile = Profile::new(Target::ResourcePack { version });
    profile.pack = Some(pack);
    profile
}

fn files(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn pack_formats_come_from_the_table() {
    let (doc, _) = clean();
    let model = Some(PackModel {
        namespace: "minecraft".to_owned(),
        id: "m".to_owned(),
    });
    let all = files(&[
        "assets/minecraft/models/block/m.json",
        "assets/minecraft/textures/selftest.png",
    ]);
    let meta = |major, minor| PackMeta {
        pack_format: Some(PackFormat { major, minor }),
        ..PackMeta::default()
    };

    // 1.12: pack_format 3.
    let ok = pack_profile(
        v(1, 12, 2),
        PackInfo {
            meta: Some(meta(3, 0)),
            files: all.clone(),
            model: model.clone(),
        },
    );
    assert!(validate(&doc, &ok).is_empty());
    let wrong = pack_profile(
        v(1, 12, 2),
        PackInfo {
            meta: Some(meta(4, 0)),
            files: all.clone(),
            model: model.clone(),
        },
    );
    assert_eq!(codes(&validate(&doc, &wrong)), [PACK_FORMAT]);

    // 26.3: format 97.1 needs min_format and max_format.
    let bare = pack_profile(
        v(26, 3, 0),
        PackInfo {
            meta: Some(meta(97, 1)),
            files: all.clone(),
            model: model.clone(),
        },
    );
    assert_eq!(codes(&validate(&doc, &bare)), [PACK_FORMAT_KEYS]);
    let ranged = pack_profile(
        v(26, 3, 0),
        PackInfo {
            meta: Some(PackMeta {
                pack_format: None,
                min_format: Some(PackFormat {
                    major: 97,
                    minor: 0,
                }),
                max_format: Some(PackFormat {
                    major: 97,
                    minor: 1,
                }),
                supported_formats: true,
            }),
            files: all.clone(),
            model: model.clone(),
        },
    );
    let issues = validate(&doc, &ranged);
    assert_eq!(codes(&issues), [PACK_FORMAT_KEYS]);
    assert!(issues[0].message.contains("supported_formats"));

    // No entry for 26.4 in the table: info, never an invented format.
    let unknown = pack_profile(
        v(26, 4, 0),
        PackInfo {
            meta: Some(PackMeta {
                pack_format: None,
                min_format: Some(PackFormat {
                    major: 97,
                    minor: 0,
                }),
                max_format: Some(PackFormat {
                    major: 97,
                    minor: 1,
                }),
                supported_formats: false,
            }),
            files: all.clone(),
            model: model.clone(),
        },
    );
    let issues = validate(&doc, &unknown);
    assert_eq!(codes(&issues), [PACK_FORMAT_UNKNOWN]);
    assert_eq!(issues[0].severity, Severity::Info);

    let missing = pack_profile(
        v(1, 12, 2),
        PackInfo {
            meta: None,
            files: all,
            model,
        },
    );
    assert_eq!(codes(&validate(&doc, &missing)), [PACK_MCMETA]);
}

#[test]
fn pack_files_and_item_definitions() {
    let (mut doc, _) = clean();
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Item;
    let meta = PackMeta {
        pack_format: Some(PackFormat { major: 1, minor: 0 }),
        ..PackMeta::default()
    };
    let model = Some(PackModel {
        namespace: "mod".to_owned(),
        id: "sword/iron".to_owned(),
    });
    let mut pack = PackInfo {
        meta: Some(meta),
        files: Vec::new(),
        model,
    };
    let profile = pack_profile(v(1, 21, 4), pack.clone());
    let issues = validate(&doc, &profile);
    let kinds = codes(&issues);
    assert!(kinds.contains(&PACK_ITEMDEF) && kinds.contains(&PACK_FILE_MISSING));
    let msgs: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
    assert!(
        msgs.iter()
            .any(|m| m.contains("assets/mod/items/sword/iron.json"))
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("assets/mod/models/item/sword/iron.json"))
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("assets/minecraft/textures/selftest.png"))
    );

    pack.files = files(&[
        "assets/mod/items/sword/iron.json",
        "assets/mod/models/item/sword/iron.json",
        "assets/minecraft/textures/selftest.png",
    ]);
    let issues = validate(&doc, &pack_profile(v(1, 21, 4), pack.clone()));
    assert!(!codes(&issues).contains(&PACK_ITEMDEF));
    assert!(!codes(&issues).contains(&PACK_FILE_MISSING));

    // Before 1.21.4 there is no item definition to miss.
    pack.files.remove(0);
    let issues = validate(&doc, &pack_profile(v(1, 20, 1), pack));
    assert!(!codes(&issues).contains(&PACK_ITEMDEF));
}

#[test]
fn every_code_has_one_rule() {
    let listed: Vec<&str> = RULES.iter().map(|r| r.code).collect();
    let mut sorted = listed.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), listed.len(), "a code appears twice");
    assert_eq!(listed.len(), 38);
    assert_eq!(rules().len(), 38);
    for code in &listed {
        assert!(
            code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "{code}"
        );
    }
}

fn self_test_targets() -> Vec<Target> {
    vec![
        Target::Cubik,
        Target::Bbmodel { java_block: true },
        Target::Bbmodel { java_block: false },
        Target::JavaBlock {
            version: v(1, 12, 2),
        },
        Target::JavaBlock {
            version: v(1, 20, 1),
        },
        Target::JavaItem {
            version: v(1, 21, 4),
        },
        Target::JavaItem {
            version: v(1, 21, 8),
        },
        Target::JavaBlock {
            version: v(1, 21, 11),
        },
        Target::JavaItem {
            version: v(26, 3, 0),
        },
        Target::ResourcePack {
            version: v(1, 12, 2),
        },
        Target::ResourcePack {
            version: v(1, 21, 4),
        },
        Target::ResourcePack {
            version: v(26, 3, 0),
        },
        Target::JavaBlock {
            version: v(1, 0, 0),
        },
    ]
}

#[test]
fn the_self_test_proves_every_applicable_rule_fires() {
    let empty = Document::new(TextureList::default());
    let (full, _) = clean();
    for doc in [&empty, &full] {
        for target in self_test_targets() {
            let report = validate_self_test(doc, &Profile::new(target));
            assert!(report.ok(), "{target:?}: silent {:?}", report.silent());
            assert_eq!(report.results.len(), RULES.len());
            assert!(report.fired() > 0, "{target:?}");
        }
    }
}

#[test]
fn the_self_test_covers_every_rule_on_some_target() {
    let (doc, _) = clean();
    let mut fired = std::collections::BTreeSet::new();
    for target in self_test_targets() {
        let report = validate_self_test(&doc, &Profile::new(target));
        fired.extend(
            report
                .results
                .iter()
                .filter(|r| r.outcome == Outcome::Fired)
                .map(|r| r.code),
        );
    }
    let missing: Vec<&str> = RULES
        .iter()
        .map(|r| r.code)
        .filter(|c| !fired.contains(c))
        .collect();
    assert!(missing.is_empty(), "never fired: {missing:?}");
}

#[test]
fn the_self_test_notices_a_rule_that_is_switched_off_by_a_bug() {
    // Disabled rules are reported as not applicable, not as a failure.
    let (doc, _) = clean();
    let mut profile = block_1_21_4();
    profile.disabled = vec![GEO_BOUNDS.to_owned()];
    let report = validate_self_test(&doc, &profile);
    let bounds = report.results.iter().find(|r| r.code == GEO_BOUNDS);
    assert_eq!(bounds.map(|r| r.outcome), Some(Outcome::NotApplicable));
    assert!(report.ok());
}

#[test]
fn validate_does_not_change_the_document() {
    let (doc, _) = clean();
    let before = doc.revision;
    let copy = doc.clone();
    let _ = validate(&doc, &block_1_21_4());
    let _ = validate_self_test(&doc, &block_1_21_4());
    assert_eq!(doc.revision, before);
    assert_eq!(doc.elements.len(), copy.elements.len());
    assert_eq!(doc.textures.entries.len(), copy.textures.entries.len());
}

proptest! {
    #[test]
    fn validate_never_panics(
        p in prop::array::uniform3(prop::num::f32::ANY),
        s in prop::array::uniform3(prop::num::f32::ANY),
        e in prop::array::uniform3(prop::num::f32::ANY),
        uv in prop::array::uniform4(prop::num::f32::ANY),
        hidden in any::<bool>(),
        label in "[ -~]{0,6}",
    ) {
        let (mut doc, id) = clean();
        let second = cube_with(&mut doc, |_| {});
        edit(&mut doc, id, |el| {
            el.base.p = Vec3::from_array(p);
            el.base.s = Vec3::from_array(s);
            el.base.e = Vec3::from_array(e);
            el.base.o = Vec3::from_array(p);
            el.base.hidden = hidden;
            el.faces[0].uv.set_unclamped(Vec2::new(uv[0], uv[1]), Vec2::new(uv[2], uv[3]));
            el.faces[1].texture = Label::from(label.as_str());
        });
        edit(&mut doc, second, |el| el.base.p = Vec3::from_array(p));
        for target in self_test_targets() {
            let _ = validate(&doc, &Profile::new(target));
            let _ = fits(&doc, &target);
        }
    }
}

/// 10 000 elements, a few overlapping, two textures: the whole validation takes under 50 ms in
/// release. Run with `cargo test -p cuboid-validate --release -- --ignored timing`.
#[test]
#[ignore = "timing test, release builds only"]
fn timing_ten_thousand_elements_under_50_ms() {
    let n = 10_000;
    let mut doc = grid(n, 2.0, 1.5);
    let label = Label::from("selftest_ok");
    // Some overlapping duplicates, as a real model has.
    for i in 0..100 {
        let id = crate::rules::add_cube(&mut doc, &label);
        let src = doc.elements.ids()[i * 7];
        let base = doc
            .elements
            .get(src)
            .map(|e| (e.base.p, e.base.o))
            .unwrap_or_default();
        edit(&mut doc, id, |el| {
            el.base.p = base.0;
            el.base.o = base.1;
            el.base.s = Vec3::splat(1.5);
        });
    }
    let element: Option<&Element> = doc.elements.items().first().map(|e| &**e);
    assert!(element.is_some_and(|e| e.kind == ElementType::Cube));
    let profile = block_1_21_4();
    let _warm = validate(&doc, &profile);
    let mut best = u128::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        let issues = validate(&doc, &profile);
        best = best.min(start.elapsed().as_micros());
        assert!(issues.iter().any(|i| i.code == GEO_ZFIGHT));
    }
    eprintln!(
        "validate of {} elements: {:.2} ms",
        doc.elements.len(),
        best as f64 / 1000.0
    );
    assert!(best < 50_000, "{best} us");
}

#[test]
fn a_flood_of_coincident_faces_stops_at_the_pair_cap_with_a_summary() {
    // 400 copies in one place: one cell run would pair them all, 6 faces each.
    let mut doc = grid(1, 2.0, 2.0);
    let label = Label::from("selftest_ok");
    for _ in 0..399 {
        let id = crate::rules::add_cube(&mut doc, &label);
        edit(&mut doc, id, |el| {
            el.base.p = Vec3::new(-16.0, 0.0, 16.0);
            el.base.o = el.base.p;
            el.base.s = Vec3::splat(2.0);
        });
    }
    let issues = validate(&doc, &block_1_21_4());
    let fights: Vec<&Issue> = issues.iter().filter(|i| i.code == GEO_ZFIGHT).collect();
    assert!(fights.iter().any(|i| i.message.contains("stopped early")));
    assert!(fights.len() <= 100_001, "{}", fights.len());
}

/// A block profile with project rules.
fn project(set: impl FnOnce(&mut ProjectRules)) -> Profile {
    let mut profile = block_1_21_4();
    set(&mut profile.project);
    profile
}

/// The texels per block of the first face of the first element.
fn texels_per_block(doc: &Document) -> f32 {
    let el = &doc.elements.items()[0];
    let image = &doc.textures.entries[0].texture.image;
    crate::elements::texels_per_unit(&el.faces[0], el.base.s, image).expect("a face with area")
        * 16.0
}

fn messages(issues: &[Issue], code: &str) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.code == code)
        .map(|i| i.message.clone())
        .collect()
}

#[test]
fn project_density_follows_the_target_and_the_ratio() {
    let (doc, id) = clean();
    let actual = texels_per_block(&doc);
    let with = |target: f32| project(|p| p.texel_density = Some(target));
    assert!(find(&validate(&doc, &block_1_21_4()), PRJ_DENSITY).is_none());
    for target in [actual, actual * 1.4, actual / 1.4] {
        assert!(
            find(&validate(&doc, &with(target)), PRJ_DENSITY).is_none(),
            "{target}"
        );
    }
    for target in [actual * 1.6, actual / 1.6, actual * 4.0] {
        let issues = validate(&doc, &with(target));
        let issue = find(&issues, PRJ_DENSITY).expect("outside the ratio");
        assert_eq!(issue.severity, Severity::Warning);
        assert_eq!(issue.objects, vec![id]);
        assert!(
            issue.message.contains("texels per block"),
            "{}",
            issue.message
        );
        assert!(issue.fix.is_none());
    }
    // One issue per element, however many faces are off.
    let far = validate(&doc, &with(actual * 4.0));
    assert_eq!(messages(&far, PRJ_DENSITY).len(), 1);
    let mut lax = with(actual * 4.0);
    lax.texel_ratio = 100.0;
    assert!(find(&validate(&doc, &lax), PRJ_DENSITY).is_none());
    // A project rule holds for every target.
    for target in [Target::Cubik, Target::Bbmodel { java_block: false }] {
        let mut profile = Profile::new(target);
        profile.project.texel_density = Some(actual * 4.0);
        assert!(
            find(&validate(&doc, &profile), PRJ_DENSITY).is_some(),
            "{target:?}"
        );
    }
}

#[test]
fn project_texture_size_limits_the_longest_side() {
    let (mut doc, _) = clean();
    doc.textures
        .entries
        .push(texture("big", "block/big", image(64, 32, false)));
    let limit = |side: u32| project(|p| p.texture_size = Some(side));
    assert!(find(&validate(&doc, &limit(64)), PRJ_TEXTURE_SIZE).is_none());
    let issues = validate(&doc, &limit(32));
    let found = messages(&issues, PRJ_TEXTURE_SIZE);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("\"big\"") && found[0].contains("64 x 32"),
        "{found:?}"
    );
    assert_eq!(
        find(&issues, PRJ_TEXTURE_SIZE).map(|i| i.severity),
        Some(Severity::Warning)
    );
    assert_eq!(
        messages(&validate(&doc, &limit(8)), PRJ_TEXTURE_SIZE).len(),
        2
    );
    assert!(find(&validate(&doc, &block_1_21_4()), PRJ_TEXTURE_SIZE).is_none());
}

/// A picture of `n` texels in distinct greens; the texels from `clear` on are fully clear.
fn greens(n: usize, clear: usize) -> Arc<cuboid_model::image::Image> {
    let mut buf = cuboid_model::image::RgbaBuf::new(n as u32, 1);
    for (at, px) in buf.pixels.chunks_exact_mut(4).enumerate() {
        let shade = u8::try_from(at).expect("few texels");
        px.copy_from_slice(&[10 + 20 * shade, 255, 0, if at >= clear { 0 } else { 255 }]);
    }
    Arc::new(cuboid_model::image::Image::from_rgba(buf))
}

#[test]
fn project_palette_measures_the_reach_and_may_be_strict() {
    let (doc, _) = clean();
    // The self-test texture is one orange, #c86432.
    let palette = |colors: &[[u8; 3]], reach: u32, strict: bool| {
        project(|p| {
            p.palette = colors.to_vec();
            p.palette_reach = reach;
            p.palette_strict = strict;
        })
    };
    let hit = |profile: &Profile| find(&validate(&doc, profile), PRJ_PALETTE).cloned();
    assert!(hit(&palette(&[[200, 100, 50]], 0, false)).is_none());
    assert!(hit(&palette(&[[205, 95, 55]], 10, false)).is_none());
    let issue = hit(&palette(&[[0, 0, 255]], 48, false)).expect("orange is not blue");
    assert_eq!(issue.severity, Severity::Warning);
    assert!(issue.message.contains("#c86432"), "{}", issue.message);
    assert!(issue.fix.is_none());
    // Strict makes it an error; a wide reach takes the orange in; no palette, no rule.
    let strict = hit(&palette(&[[0, 0, 255]], 48, true)).expect("strict");
    assert_eq!(strict.severity, Severity::Error);
    assert!(hit(&palette(&[[0, 0, 255]], 441, true)).is_none());
    assert!(hit(&palette(&[], 0, true)).is_none());
}

#[test]
fn project_palette_lists_eight_colours_and_counts_the_rest() {
    let (mut doc, _) = clean();
    doc.textures.entries.clear();
    // Ten greens and two clear texels, which are no colour.
    doc.textures
        .entries
        .push(texture("lime", "block/lime", greens(12, 10)));
    let profile = project(|p| {
        p.palette = vec![[0, 0, 255]];
        p.palette_reach = 48;
    });
    let found = messages(&validate(&doc, &profile), PRJ_PALETTE);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("has 10 colour(s)"), "{}", found[0]);
    assert!(found[0].ends_with(" and 2 more"), "{}", found[0]);
    assert_eq!(found[0].matches('#').count(), 8);
    // Equal counts list the lower colour first.
    assert!(found[0].contains(": #0aff00, #1eff00, "), "{}", found[0]);
}

#[test]
fn project_names_follow_the_case_and_the_prefix() {
    let (mut doc, id) = clean();
    edit(&mut doc, id, |el| el.base.label = Label::from("Big Cube"));
    let naming = |case: NameCase, prefix: &str| {
        project(|p| {
            p.case = case;
            p.prefix = prefix.to_owned();
            p.model_name = "Ink Blaster".to_owned();
        })
    };
    assert!(find(&validate(&doc, &block_1_21_4()), PRJ_NAME).is_none());
    let issues = validate(&doc, &naming(NameCase::Snake, "ink_"));
    let names = messages(&issues, PRJ_NAME);
    assert_eq!(names.len(), 3, "{names:?}");
    let file = names
        .iter()
        .find(|m| m.starts_with("the model file name \"Ink Blaster\""));
    assert!(
        file.is_some_and(|m| m.contains("snake_case") && m.contains("prefix")),
        "{names:?}"
    );
    let texture = names
        .iter()
        .find(|m| m.starts_with("texture \"selftest_ok\""));
    assert!(
        texture.is_some_and(|m| m.contains("prefix") && !m.contains("snake_case")),
        "{names:?}"
    );
    // The element follows the case only, and has a fix that renames it.
    let element = issues
        .iter()
        .find(|i| i.code == PRJ_NAME && i.objects == vec![id])
        .expect("the element");
    assert_eq!(element.severity, Severity::Info);
    assert!(element.message.contains("\"Big Cube\"") && !element.message.contains("prefix"));
    assert_eq!(
        element.fix,
        Some(vec![FixOp::RenameElement {
            id,
            name: "big_cube".to_owned()
        }])
    );
    // Kebab case. A model that was never saved has no file name to judge.
    let mut kebab = naming(NameCase::Kebab, "");
    kebab.project.model_name.clear();
    let names = messages(&validate(&doc, &kebab), PRJ_NAME);
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().all(|m| m.contains("kebab-case")), "{names:?}");
    // Names that comply say nothing.
    edit(&mut doc, id, |el| el.base.label = Label::from("big-cube"));
    doc.textures.entries[0].label = Label::from("ink-selftest");
    for el in doc.elements.ids().to_vec() {
        edit(&mut doc, el, |e| {
            for face in &mut e.faces {
                face.texture = Label::from("ink-selftest");
            }
        });
    }
    let mut fine = naming(NameCase::Kebab, "ink-");
    fine.project.model_name = "ink-blaster".to_owned();
    assert_eq!(
        messages(&validate(&doc, &fine), PRJ_NAME),
        Vec::<String>::new()
    );
}

#[test]
fn project_namespace_holds_for_texture_paths_and_the_item_id() {
    let (mut doc, _) = clean();
    let img = image(16, 16, false);
    for (label, path) in [
        ("mine", "splat:block/mine"),
        ("vanilla", "block/vanilla"),
        ("stone", "minecraft:block/stone"),
        ("other", "other:block/other"),
        ("alias", "#mine"),
        ("blank", ""),
    ] {
        doc.textures.entries.push(texture(label, path, img.clone()));
    }
    let profile = project(|p| p.namespace = "splat".to_owned());
    let issues = validate(&doc, &profile);
    let found = messages(&issues, PRJ_NAMESPACE);
    // The self-test texture has the path "selftest", which is in the namespace minecraft.
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .any(|m| m.contains("\"vanilla\"") && m.contains("minecraft"))
    );
    assert!(
        found
            .iter()
            .any(|m| m.contains("\"other\"") && m.contains("namespace other"))
    );
    assert!(
        !found
            .iter()
            .any(|m| m.contains("\"mine\"") || m.contains("\"alias\"") || m.contains("\"blank\""))
    );
    // A path that names `minecraft:` uses a vanilla texture on purpose.
    assert!(!found.iter().any(|m| m.contains("\"stone\"")), "{found:?}");
    let other = issues
        .iter()
        .find(|i| i.code == PRJ_NAMESPACE && i.message.contains("\"other\""))
        .expect("other");
    assert_eq!(other.severity, Severity::Warning);
    assert_eq!(
        other.fix,
        Some(vec![FixOp::SetTexturePath {
            label: Label::from("other"),
            path: "splat:block/other".to_owned()
        }])
    );
    assert!(find(&validate(&doc, &block_1_21_4()), PRJ_NAMESPACE).is_none());

    // The item id of a pack is judged too.
    let mut pack = pack_profile(
        v(1, 21, 4),
        PackInfo {
            meta: Some(PackMeta {
                pack_format: Some(PackFormat {
                    major: 46,
                    minor: 0,
                }),
                ..PackMeta::default()
            }),
            files: Vec::new(),
            model: Some(PackModel {
                namespace: "elsewhere".to_owned(),
                id: "gun".to_owned(),
            }),
        },
    );
    pack.project.namespace = "splat".to_owned();
    let found = messages(&validate(&doc, &pack), PRJ_NAMESPACE);
    assert!(
        found.iter().any(|m| m.contains("item id elsewhere:gun")),
        "{found:?}"
    );
}
