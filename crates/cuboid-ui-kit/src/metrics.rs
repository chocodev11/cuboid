//! Sizes of the UI v2 specification (section 3), in points at 100 % UI scale. The zoom factor of
//! the theme multiplies all of them. Code outside the kit takes its numbers from here or from a
//! component; it does not write literals.

/// The spacing scale for paddings and gaps.
pub mod space {
    /// 2: inside chips and badges.
    pub const XXS: f32 = 2.0;
    /// 4: between related controls, the base of the grid.
    pub const XS: f32 = 4.0;
    /// 8: between controls of a row.
    pub const SM: f32 = 8.0;
    /// 12: between sections.
    pub const MD: f32 = 12.0;
    /// 16: dialog padding.
    pub const LG: f32 = 16.0;
    /// 24: between page groups.
    pub const XL: f32 = 24.0;
    /// 32: the largest gap.
    pub const XXL: f32 = 32.0;
    /// The whole scale.
    pub const SCALE: [f32; 7] = [XXS, XS, SM, MD, LG, XL, XXL];
}

/// Heights of controls and rows.
pub mod height {
    /// 24: fields, buttons, property rows and the status bar.
    pub const CONTROL: f32 = 24.0;
    /// 28: panel headers, toolbars, tabs and list rows.
    pub const BAR: f32 = 28.0;
    /// 32: primary actions in dialogs and on the start screen, and the top bar.
    pub const ACTION: f32 = 32.0;
    /// 28: a list row.
    pub const ROW: f32 = 28.0;
    /// 44: a list row with a thumbnail.
    pub const ROW_THUMB: f32 = 44.0;
    /// 36: the width of a vertical tool or tab strip.
    pub const STRIP: f32 = 36.0;
}

/// Icon edge lengths.
pub mod icon_size {
    /// 16: in controls and rows.
    pub const CONTROL: f32 = 16.0;
    /// 20: in toolbars.
    pub const TOOLBAR: f32 = 20.0;
    /// 40: in empty states.
    pub const EMPTY: f32 = 40.0;
}

/// Corner radii.
pub mod radius {
    /// 3: controls.
    pub const CONTROL: u8 = 3;
    /// 4: cards, panels and menus.
    pub const CARD: u8 = 4;
    /// 6: modals.
    pub const MODAL: u8 = 6;
}

/// Distances between regions.
pub mod gutter {
    /// 8: at window edges and inside panels.
    pub const EDGE: f32 = 8.0;
    /// 12: between sections.
    pub const SECTION: f32 = 12.0;
    /// 24: between page groups.
    pub const GROUP: f32 = 24.0;
}

/// Property grid label column: default, smallest and largest width.
pub mod label_column {
    /// 96: the default.
    pub const DEFAULT: f32 = 96.0;
    /// 72: the smallest.
    pub const MIN: f32 = 72.0;
    /// 128: the largest.
    pub const MAX: f32 = 128.0;
}

/// Width of a toast.
pub const TOAST_WIDTH: f32 = 320.0;
/// Most toasts on screen at once.
pub const TOAST_MAX: usize = 3;
/// Dialog widths (section 10).
pub const DIALOG_WIDTHS: [f32; 3] = [360.0, 520.0, 720.0];
/// Thickness of the active tab indicator.
pub const TAB_UNDERLINE: f32 = 2.0;
/// Thickness of the keyboard focus ring.
pub const FOCUS_RING: f32 = 1.0;
/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.35;
/// Opacity of the modal backdrop, 40 %.
pub const BACKDROP_ALPHA: u8 = 102;

#[cfg(test)]
mod tests {
    use super::*;

    fn same(got: &[f32], want: &[f32]) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < f32::EPSILON, "{got:?} is not {want:?}");
        }
    }

    #[test]
    fn values_match_the_specification() {
        same(&space::SCALE, &[2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0]);
        same(
            &[height::CONTROL, height::BAR, height::ACTION],
            &[24.0, 28.0, 32.0],
        );
        same(&[height::ROW, height::ROW_THUMB], &[28.0, 44.0]);
        same(&[height::STRIP], &[36.0]);
        same(
            &[icon_size::CONTROL, icon_size::TOOLBAR, icon_size::EMPTY],
            &[16.0, 20.0, 40.0],
        );
        assert_eq!([radius::CONTROL, radius::CARD, radius::MODAL], [3, 4, 6]);
        same(
            &[gutter::EDGE, gutter::SECTION, gutter::GROUP],
            &[8.0, 12.0, 24.0],
        );
        same(
            &[label_column::MIN, label_column::DEFAULT, label_column::MAX],
            &[72.0, 96.0, 128.0],
        );
        same(&[TOAST_WIDTH], &[320.0]);
        assert_eq!(TOAST_MAX, 3);
    }

    #[test]
    fn spacing_sits_on_the_grid() {
        for value in space::SCALE.into_iter().filter(|v| *v >= space::XS) {
            assert!(
                (value % 4.0).abs() < f32::EPSILON,
                "{value} is off the grid"
            );
        }
        for value in [
            height::CONTROL,
            height::BAR,
            height::ACTION,
            height::ROW,
            height::ROW_THUMB,
            height::STRIP,
        ] {
            assert!(
                (value % 4.0).abs() < f32::EPSILON,
                "{value} is off the grid"
            );
        }
    }
}
