//! Display preview anchors and slot transforms (spec: app 9.1-9.4).

use cuboid_model::{DisplaySlot, DisplaySlotId};
use glam::{Mat4, Quat, Vec3};

use crate::camera::Camera;

/// The nine display modes. The discriminants are the ids of the table in app 9.1.
///
/// spec: app 9.1
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum DisplayMode {
    /// The normal workspace.
    Default = 0,
    /// Third person, right hand.
    RightHand3rd = 1,
    /// Third person, left hand.
    LeftHand3rd = 2,
    /// First person, right hand.
    RightHand1st = 3,
    /// First person, left hand.
    LeftHand1st = 4,
    /// Item frame on a wall.
    Wall = 5,
    /// Inventory slot.
    Gui = 6,
    /// On the player's head.
    Head = 7,
    /// Item on the ground.
    Floor = 8,
}

impl DisplayMode {
    /// The display slot that poses the model in this mode; `None` for `Default`.
    ///
    /// spec: app 9.4 (slot mapping)
    pub fn slot(self) -> Option<DisplaySlotId> {
        match self {
            DisplayMode::Default => None,
            DisplayMode::RightHand3rd => Some(DisplaySlotId::ThirdRight),
            DisplayMode::LeftHand3rd => Some(DisplaySlotId::ThirdLeft),
            DisplayMode::RightHand1st => Some(DisplaySlotId::FirstRight),
            DisplayMode::LeftHand1st => Some(DisplaySlotId::FirstLeft),
            DisplayMode::Wall => Some(DisplaySlotId::Fixed),
            DisplayMode::Gui => Some(DisplaySlotId::Gui),
            DisplayMode::Head => Some(DisplaySlotId::Head),
            DisplayMode::Floor => Some(DisplaySlotId::Ground),
        }
    }
}

/// The preview state of a session: the current mode and the per-mode camera records, which live
/// for the session only.
///
/// spec: app 9.2
#[derive(Clone, Debug)]
pub struct PreviewState {
    /// The current mode.
    pub mode: DisplayMode,
    /// The saved Main camera of every mode, created on first save.
    pub cameras: [Option<Camera>; 9],
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Default,
            cameras: [None; 9],
        }
    }
}

/// The transform the anchor of `mode` gets every frame from its slot: translation
/// `(t.x, t.y, -t.z)` in the anchor frame, the rotation composed in written order, the scale, and
/// the floor animation of app 9.5 at `time_s` seconds.
///
/// spec: app 9.1, app 9.4, app 9.5
pub fn anchor_transform(mode: DisplayMode, slot: &DisplaySlot, time_s: f32) -> Mat4 {
    let (t, r) = (slot.translation, slot.rotation);
    let left = matches!(mode, DisplayMode::LeftHand3rd | DisplayMode::LeftHand1st);
    let (pos, rot) = if left {
        (
            Vec3::new(-t.x, t.y, -t.z),
            Quat::from_rotation_x((-r.x).to_radians())
                * Quat::from_rotation_y(r.y.to_radians())
                * Quat::from_rotation_z((-r.z).to_radians()),
        )
    } else {
        (
            Vec3::new(t.x, t.y, -t.z),
            Quat::from_rotation_x((-r.x).to_radians())
                * Quat::from_rotation_y((-r.y).to_radians())
                * Quat::from_rotation_z(r.z.to_radians()),
        )
    };
    // A zero scale component counts as 0.0001.
    let scale = slot.scale.map(|c| if c.abs() > 0.0 { c } else { 0.0001 });
    let local = Mat4::from_scale_rotation_translation(scale, rot, pos);
    if mode != DisplayMode::Floor {
        return local;
    }
    // The ground item bobs and turns about its own axis.
    let y = 8.0 + (2.0 * time_s).cos() - 7.0 + 4.0 * slot.scale.y;
    let anchor = Mat4::from_rotation_translation(
        Quat::from_rotation_y((-60.0 * time_s).to_radians()),
        Vec3::new(8.0, y, -8.0),
    );
    anchor * local
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(t: Vec3, r: Vec3, s: Vec3) -> DisplaySlot {
        DisplaySlot {
            translation: t,
            rotation: r,
            scale: s,
            ..DisplaySlot::default()
        }
    }

    #[test]
    fn the_default_slot_leaves_the_model_alone() {
        let m = anchor_transform(DisplayMode::Gui, &DisplaySlot::default(), 0.0);
        assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-6));
    }

    #[test]
    fn right_hand_slots_flip_z_and_left_hand_slots_mirror_x() {
        let s = slot(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, Vec3::ONE);
        let right = anchor_transform(DisplayMode::RightHand3rd, &s, 0.0);
        assert!(
            right
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(1.0, 2.0, -3.0), 1e-6)
        );
        let left = anchor_transform(DisplayMode::LeftHand3rd, &s, 0.0);
        assert!(
            left.transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(-1.0, 2.0, -3.0), 1e-6)
        );
    }

    #[test]
    fn rotations_compose_in_the_written_order() {
        let s = slot(Vec3::ZERO, Vec3::new(90.0, 0.0, 0.0), Vec3::ONE);
        // Rx(-90) turns +Y onto -Z... in the engine's convention onto +Z or -Z by the sign.
        let m = anchor_transform(DisplayMode::Head, &s, 0.0);
        let expected = Quat::from_rotation_x((-90.0_f32).to_radians()) * Vec3::Y;
        assert!(m.transform_vector3(Vec3::Y).abs_diff_eq(expected, 1e-5));
        let both = slot(Vec3::ZERO, Vec3::new(90.0, 90.0, 0.0), Vec3::ONE);
        let m = anchor_transform(DisplayMode::Head, &both, 0.0);
        let q = Quat::from_rotation_x((-90.0_f32).to_radians())
            * Quat::from_rotation_y((-90.0_f32).to_radians());
        assert!(m.transform_vector3(Vec3::X).abs_diff_eq(q * Vec3::X, 1e-5));
        let left = anchor_transform(DisplayMode::LeftHand1st, &both, 0.0);
        let q = Quat::from_rotation_x((-90.0_f32).to_radians())
            * Quat::from_rotation_y(90.0_f32.to_radians());
        assert!(
            left.transform_vector3(Vec3::X)
                .abs_diff_eq(q * Vec3::X, 1e-5)
        );
    }

    #[test]
    fn a_zero_scale_component_counts_as_one_ten_thousandth() {
        let s = slot(Vec3::ZERO, Vec3::ZERO, Vec3::new(2.0, 0.0, 1.0));
        let m = anchor_transform(DisplayMode::Gui, &s, 0.0);
        assert!(
            m.transform_vector3(Vec3::ONE)
                .abs_diff_eq(Vec3::new(2.0, 0.0001, 1.0), 1e-6)
        );
    }

    #[test]
    fn the_ground_item_bobs_between_four_and_six_and_turns() {
        let s = DisplaySlot::default();
        let at = |t: f32| anchor_transform(DisplayMode::Floor, &s, t);
        assert!(
            at(0.0)
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(8.0, 6.0, -8.0), 1e-5)
        );
        let half = std::f32::consts::FRAC_PI_2;
        assert!((at(half).transform_point3(Vec3::ZERO).y - 4.0).abs() < 1e-4);
        let turned = at(1.5).transform_vector3(Vec3::X);
        let expected = Quat::from_rotation_y((-90.0_f32).to_radians()) * Vec3::X;
        assert!(turned.abs_diff_eq(expected, 1e-4));
    }

    #[test]
    fn display_mode_ids_follow_the_table_of_app_9_1() {
        let modes = [
            DisplayMode::Default,
            DisplayMode::RightHand3rd,
            DisplayMode::LeftHand3rd,
            DisplayMode::RightHand1st,
            DisplayMode::LeftHand1st,
            DisplayMode::Wall,
            DisplayMode::Gui,
            DisplayMode::Head,
            DisplayMode::Floor,
        ];
        let ids: Vec<u8> = modes.iter().map(|m| *m as u8).collect();
        assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn display_modes_map_to_their_slots_as_in_app_9_4() {
        assert_eq!(DisplayMode::Default.slot(), None);
        assert_eq!(
            DisplayMode::RightHand3rd.slot(),
            Some(DisplaySlotId::ThirdRight)
        );
        assert_eq!(
            DisplayMode::LeftHand3rd.slot(),
            Some(DisplaySlotId::ThirdLeft)
        );
        assert_eq!(
            DisplayMode::RightHand1st.slot(),
            Some(DisplaySlotId::FirstRight)
        );
        assert_eq!(
            DisplayMode::LeftHand1st.slot(),
            Some(DisplaySlotId::FirstLeft)
        );
        assert_eq!(DisplayMode::Wall.slot(), Some(DisplaySlotId::Fixed));
        assert_eq!(DisplayMode::Gui.slot(), Some(DisplaySlotId::Gui));
        assert_eq!(DisplayMode::Head.slot(), Some(DisplaySlotId::Head));
        assert_eq!(DisplayMode::Floor.slot(), Some(DisplaySlotId::Ground));
    }
}
