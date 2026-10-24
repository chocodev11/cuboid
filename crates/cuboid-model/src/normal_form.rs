//! Record normal form applied on restore and paste: colour packing, UV clamp and fold, rotation
//! requantisation (spec: native 4.4.3, 4.10, 5.3, 6.3; tools 15.2; arch 9.1).
//!
//! A snapshot has the shape of the document's records, so restoring or pasting one must give what
//! a round trip through the records gives. Each function returns the normalised copy of a record,
//! or `None` when the record is already a fixed point, so the apply path keeps the `Arc` of such a
//! record and the renderer re-uploads only what changed.

use std::sync::Arc;

use cuboid_num::{engine_eq, pack, snap_angle, wrap360};
use glam::Vec3;

use crate::contract::{DisplaySlots, ModelProps, ViewProps};
use crate::objects::{Blueprint, Element, MIN_SIZE, Mesh, MeshKind};
use crate::uv::UvRect;

/// Stored for a zero component of a display slot scale (native 5.3, `md.*.s`).
const MIN_DISPLAY_SCALE: f32 = 0.0001;

/// U+201D, which replaces an ASCII double quote in the model strings (native 4.2.1).
fn closing_quote() -> char {
    char::from_u32(0x201D).unwrap_or('"')
}

/// The rotation a record gets: each component `(angle + 360) mod 360` with the C remainder, then
/// rounded half to even to the 22.5 degree grid when `restricted` or to 5 degrees. A rotation
/// equal to the object's current one is not normalised at all; a new object's current rotation is
/// zero, so `current` is `Vec3::ZERO` for objects that are created and the live rotation for the
/// blueprint.
///
/// spec: native 6.3 (rotations)
pub fn rotation(e: Vec3, restricted: bool, current: Vec3) -> Vec3 {
    if engine_eq(e, current) {
        return e;
    }
    e.map(|c| snap_angle(wrap360(c), restricted))
}

/// The UV rectangle a record gets: the folded corners become the stored ones (the record holds
/// the values the getters return) and the rotation and the clamp are those of the record setters.
///
/// spec: native 4.4.3, arch 9.1
pub fn uv(rect: UvRect) -> UvRect {
    UvRect::new(rect.rot(), rect.tl(), rect.br())
}

/// An element after its record round trip: colour packed to bytes with the alpha forced to 255
/// (a fully transparent colour loads as opaque black), rotation requantised for a new object
/// and UV rectangles folded. Everything else is stored as it is: sizes are assigned directly.
///
/// spec: native 4.10, native 6.3 (`el`, rotations, sizes)
pub fn element(el: &Element, restricted: bool) -> Option<Element> {
    let color = pack::unpack_element(pack::element(el.color));
    let e = rotation(el.base.e, restricted, Vec3::ZERO);
    let uvs = el.faces.each_ref().map(|face| uv(face.uv));
    let unchanged = color == el.color
        && e == el.base.e
        && el
            .faces
            .iter()
            .zip(&uvs)
            .all(|(face, rect)| face.uv == *rect);
    if unchanged {
        return None;
    }
    let mut out = el.clone();
    out.color = color;
    out.base.e = e;
    for (face, rect) in out.faces.iter_mut().zip(uvs) {
        face.uv = rect;
    }
    Some(out)
}

/// The material colour a primitive builder gives a new mesh: white (tools 6.5).
pub const BUILDER_TINT: [f32; 4] = [1.0; 4];

/// A mesh after its record round trip: the rotation uses the 5 degree grid whatever the
/// restriction setting says, a primitive takes the builder's tint (the record has none) and the
/// sub-mesh colours are packed. The scale stays unclamped.
///
/// spec: native 4.10, native 6.3 (rotations, `me`), tools 6.1, tools 6.2
pub fn mesh(m: &Mesh) -> Option<Mesh> {
    let e = rotation(m.base.e, false, Vec3::ZERO);
    let tint = if m.kind == MeshKind::Custom {
        m.tint
    } else {
        BUILDER_TINT
    };
    let packed = |color: [f32; 4]| pack::unpack_submesh(pack::submesh(color));
    let colours_change = m.submeshes.iter().any(|sub| packed(sub.color) != sub.color);
    if e == m.base.e && tint == m.tint && !colours_change {
        return None;
    }
    let mut out = m.clone();
    out.base.e = e;
    out.tint = tint;
    if colours_change {
        let mut submeshes = m.submeshes.to_vec();
        for sub in &mut submeshes {
            sub.color = packed(sub.color);
        }
        out.submeshes = submeshes.into();
    }
    Some(out)
}

/// The blueprint after its record is applied: the size goes through the editor setter (components
/// below 0.001 are raised), the rotation is requantised unless it equals the live rotation
/// `current`, and the origin is the position.
///
/// spec: native 6.3 (`bp`, sizes), tools 2.3
pub fn blueprint(bp: &Blueprint, restricted: bool, current: Vec3) -> Option<Blueprint> {
    let base = &bp.base;
    let e = rotation(base.e, restricted, current);
    let s = base.s.max(Vec3::splat(MIN_SIZE));
    if e == base.e && s == base.s && base.o == base.p {
        return None;
    }
    let mut out = bp.clone();
    out.base.e = e;
    out.base.s = s;
    out.base.o = out.base.p;
    Some(out)
}

/// The model properties without ASCII double quotes in the description, the name and the
/// Minecraft parent: they become U+201D whenever the strings are assigned, a file read included.
///
/// spec: native 4.2.1, native 5.3 (`mp.g.d`, `mp.g.n`, `mp.mc.p`)
pub fn props(p: &ModelProps) -> Option<ModelProps> {
    let quoted = [&p.general.description, &p.general.name, &p.minecraft.parent]
        .into_iter()
        .any(|text| text.contains('"'));
    if !quoted {
        return None;
    }
    let mut out = p.clone();
    let replace = |text: &mut String| *text = text.replace('"', &closing_quote().to_string());
    replace(&mut out.general.description);
    replace(&mut out.general.name);
    replace(&mut out.minecraft.parent);
    Some(out)
}

/// The view properties as a save and a capture write them: `gu` and `gl` with their defaults.
///
/// spec: native 5.2 (`vp`), native 5.3 (`vp.gu`, `vp.gl`)
pub fn saved_view(v: &ViewProps) -> Option<ViewProps> {
    let fresh = ViewProps::default();
    if v.uv_step == fresh.uv_step && v.large_grid == fresh.large_grid {
        return None;
    }
    Some(ViewProps {
        uv_step: fresh.uv_step,
        large_grid: fresh.large_grid,
        ..v.clone()
    })
}

/// The display slots as a save writes them: a zero scale component becomes 0.0001. The helper
/// angle `a` is not stored.
///
/// spec: native 5.3 (`md.*.a`, `md.*.s`)
pub fn saved_display(d: &DisplaySlots) -> Option<DisplaySlots> {
    let has_zero = d
        .slots
        .iter()
        .any(|slot| slot.scale.to_array().contains(&0.0));
    if !has_zero {
        return None;
    }
    let mut out = d.clone();
    for slot in &mut out.slots {
        slot.scale = slot
            .scale
            .map(|c| if c == 0.0 { MIN_DISPLAY_SCALE } else { c });
    }
    Some(out)
}

/// `normalised` in an `Arc`, or `shared` itself when there is nothing to normalise.
pub(crate) fn share<T>(shared: &Arc<T>, normalised: Option<T>) -> Arc<T> {
    normalised.map_or_else(|| Arc::clone(shared), Arc::new)
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::contract::DisplaySlotId;
    use crate::objects::{ElementType, Label, MeshBounds, ObjectBase, Perms, Submesh};

    fn element_at(e: Vec3) -> Element {
        let mut el = Element::new(
            ElementType::Cube,
            &Blueprint::default(),
            1,
            [1.0; 4],
            &Label::from("texture"),
        );
        el.base.e = e;
        el
    }

    fn mesh_at(e: Vec3) -> Mesh {
        let mut m = Mesh {
            base: ObjectBase::new("Mesh", Vec3::ZERO, Vec3::ONE, Perms::PRIMITIVE),
            kind: MeshKind::Cube,
            geometry: Arc::default(),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        };
        m.base.e = e;
        m
    }

    #[test]
    fn rotation_wraps_with_the_c_remainder_and_rounds_half_to_even() {
        let r = |c: f32, restricted| rotation(Vec3::new(c, 0.0, 0.0), restricted, Vec3::ZERO).x;
        assert_eq!(r(725.0, false), 5.0);
        assert_eq!(r(-90.0, false), 270.0);
        // Below -360 the remainder stays negative: (-400 + 360) % 360 = -40.
        assert_eq!(r(-400.0, false), -40.0);
        assert_eq!(r(7.4, false), 5.0);
        assert_eq!(r(7.5, false), 10.0, "7.5 / 5 = 1.5 rounds to 2");
        assert_eq!(r(2.5, false), 0.0, "0.5 rounds to 0, the even neighbour");
        assert_eq!(r(30.0, true), 22.5);
        assert_eq!(
            r(11.25, true),
            0.0,
            "half a step rounds to the even multiple"
        );
        assert_eq!(r(33.75, true), 45.0, "1.5 steps round to 2");
        assert_eq!(
            r(359.0, false),
            360.0,
            "rounding can reach 360 and nothing wraps it"
        );
    }

    #[test]
    fn a_rotation_equal_to_the_current_one_is_not_normalised() {
        let odd = Vec3::new(7.0, 0.0, 0.0);
        assert_eq!(rotation(odd, false, odd), odd);
        assert_eq!(rotation(odd, false, Vec3::ZERO), Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(rotation(Vec3::ZERO, true, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn element_colours_pack_to_bytes_with_the_alpha_forced() {
        let mut el = element_at(Vec3::ZERO);
        el.color = [0.5, 0.25, 1.0, 0.3];
        let n = element(&el, true).expect("not a fixed point");
        assert_eq!(n.color, pack::unpack_element(pack::element(el.color)));
        assert_eq!(n.color[3], 1.0);

        el.color = [0.2, 0.4, 0.6, 0.0];
        assert_eq!(
            element(&el, true)
                .expect("fully transparent loads as black")
                .color,
            [0.0, 0.0, 0.0, 1.0]
        );

        el.color = pack::unpack_element(pack::element([0.2, 0.4, 0.6, 1.0]));
        assert!(
            element(&el, true).is_none(),
            "a packed colour is a fixed point"
        );
    }

    #[test]
    fn element_rotations_follow_the_restriction_and_meshes_always_use_five_degrees() {
        let el = element_at(Vec3::new(0.0, 30.0, 0.0));
        assert_eq!(
            element(&el, true).expect("30 is off the 22.5 grid").base.e,
            Vec3::new(0.0, 22.5, 0.0)
        );
        assert!(element(&el, false).is_none(), "30 is on the 5 degree grid");

        let m = mesh_at(Vec3::new(0.0, 30.0, 0.0));
        assert!(mesh(&m).is_none());
        let m = mesh_at(Vec3::new(0.0, 22.5, 0.0));
        assert_eq!(mesh(&m).expect("22.5 is off the 5 grid").base.e.y, 20.0);
    }

    #[test]
    fn a_primitive_takes_the_builder_tint_and_an_imported_mesh_keeps_its_own() {
        let mut m = mesh_at(Vec3::ZERO);
        assert!(mesh(&m).is_none());
        m.tint = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(mesh(&m).expect("tinted").tint, BUILDER_TINT);

        m.kind = MeshKind::Custom;
        assert!(mesh(&m).is_none(), "the tint of an imported mesh stays");
    }

    #[test]
    fn sub_mesh_colours_are_packed_without_a_clamp() {
        let sub = |color| Submesh {
            triangles: vec![0, 1, 2],
            color,
            texture: None,
        };
        let mut m = mesh_at(Vec3::ZERO);
        m.kind = MeshKind::Custom;
        m.submeshes = Arc::from([sub([0.5, 1.2, 0.0, 1.0]), sub([1.0; 4]), sub([0.3; 4])]);
        let n = mesh(&m).expect("0.5 is not a byte value");
        assert_eq!(n.submeshes[0].color[0], 127.0 / 255.0);
        assert_eq!(n.submeshes[0].color[1], 50.0 / 255.0, "306 wraps to 50");
        assert_eq!(n.submeshes[1].color, [1.0; 4]);
        assert_eq!(n.submeshes[2].triangles, [0, 1, 2]);
        assert!(mesh(&n).is_none(), "packed colours are a fixed point");

        m.submeshes = Arc::from([sub([0.5, 0.5, 0.5, 0.001])]);
        assert_eq!(mesh(&m).expect("alpha 0").submeshes[0].color, [0.0; 4]);

        m.submeshes = Arc::from([sub([1.0; 4])]);
        let before = Arc::as_ptr(&m.submeshes);
        assert!(mesh(&m).is_none());
        assert_eq!(Arc::as_ptr(&m.submeshes), before);
    }

    #[test]
    fn uv_rectangles_keep_the_folded_corners_and_a_quarter_turn_rotation() {
        let v = Vec2::new;
        let dragged = UvRect::new(450, v(-0.75, 1.25), v(-0.25, 1.75));
        let n = uv(dragged);
        assert_eq!((n.tl_raw(), n.br_raw()), (dragged.tl(), dragged.br()));
        assert_eq!((n.tl(), n.br()), (dragged.tl(), dragged.br()));
        assert_eq!(n.rot(), 90);
        assert_eq!(uv(n), n, "folded values are a fixed point");

        // A stored value below -405 keeps a negative twin until a record round trip.
        let negative = UvRect::new(-450, Vec2::ZERO, Vec2::ONE);
        assert_eq!(negative.rot(), -90);
        assert_eq!(uv(negative).rot(), 270);

        let mut el = element_at(Vec3::ZERO);
        el.faces[3].uv = dragged;
        let n = element(&el, true).expect("an unfolded face changes");
        assert_eq!(n.faces[3].uv, uv(dragged));
        assert_eq!(n.faces[0].uv, el.faces[0].uv);
    }

    #[test]
    fn the_blueprint_size_is_raised_the_origin_follows_and_a_live_rotation_stays() {
        let mut bp = Blueprint::default();
        assert!(blueprint(&bp, true, Vec3::ZERO).is_none());

        bp.base.s = Vec3::new(0.0, 2.0, 0.0005);
        bp.base.o = Vec3::new(9.0, 9.0, 9.0);
        bp.base.e = Vec3::new(5.0, 0.0, 0.0);
        let n = blueprint(&bp, true, Vec3::ZERO).expect("changes");
        assert_eq!(n.base.s, Vec3::new(MIN_SIZE, 2.0, MIN_SIZE));
        assert_eq!(n.base.o, n.base.p);
        assert_eq!(
            n.base.e,
            Vec3::ZERO,
            "5 degrees rounds to 0 on the 22.5 grid"
        );

        let n = blueprint(&bp, true, bp.base.e).expect("changes");
        assert_eq!(
            n.base.e,
            Vec3::new(5.0, 0.0, 0.0),
            "equal to the live rotation"
        );
    }

    #[test]
    fn double_quotes_in_the_model_strings_become_closing_quotes() {
        let mut p = ModelProps::default();
        assert!(props(&p).is_none());
        p.general.description = "say \"hi\"".to_owned();
        p.minecraft.parent = "a\"b".to_owned();
        let n = props(&p).expect("quoted");
        let quote = closing_quote();
        assert_eq!(n.general.description, format!("say {quote}hi{quote}"));
        assert_eq!(n.minecraft.parent, format!("a{quote}b"));
        assert_eq!(n.general.name, "Cubik Model");
        assert!(props(&n).is_none());
    }

    #[test]
    fn a_save_writes_gu_and_gl_with_their_defaults() {
        let mut v = ViewProps::default();
        assert!(saved_view(&v).is_none());
        v.uv_step = 0.25;
        v.large_grid = true;
        v.mouse_step = 2.0;
        let n = saved_view(&v).expect("changes");
        assert_eq!((n.uv_step, n.large_grid), (0.5, false));
        assert_eq!(n.mouse_step, 2.0);
    }

    #[test]
    fn zero_display_scale_components_are_saved_as_a_tiny_value() {
        let mut d = DisplaySlots::default();
        assert!(saved_display(&d).is_none());
        d[DisplaySlotId::Head].scale = Vec3::new(0.0, 1.0, 2.0);
        let n = saved_display(&d).expect("changes");
        assert_eq!(n[DisplaySlotId::Head].scale, Vec3::new(0.0001, 1.0, 2.0));
        assert_eq!(n[DisplaySlotId::Gui], d[DisplaySlotId::Gui]);
        assert!(saved_display(&n).is_none());
    }

    #[test]
    fn share_keeps_the_pointer_of_a_fixed_point() {
        let a = Arc::new(3);
        assert!(Arc::ptr_eq(&share(&a, None), &a));
        assert_eq!(*share(&a, Some(4)), 4);
    }
}
