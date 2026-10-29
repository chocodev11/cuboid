//! The window icon: the title bar of a decorated window, Alt+Tab and the taskbar button
//! (docs/design/logo.md 4).
//!
//! Windows takes the icons from resource 1 of the executable, the one `build.rs` of the application
//! embeds, so nothing is decoded and the system picks the image closest to the size it asks for.
//! Elsewhere the application hands in the picture ([`ShellOptions::icon`]). A failure costs the
//! icon, not the window: the generic one stays and a warning goes to the log.
//!
//! `set(window, scale_factor, picture)` gives `window` its icons. `scale_factor` is the one of its
//! monitor: the Windows icons have its size, so the call comes again when it changes. `picture` is
//! the icon of the platforms that have no resource.
//!
//! [`ShellOptions::icon`]: crate::ShellOptions::icon

pub(crate) use platform::set;

#[cfg(windows)]
mod platform {
    use winit::dpi::{LogicalSize, PhysicalSize};
    use winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use winit::window::{BadIcon, Icon, Window};

    use crate::RgbaImage;

    /// The ordinal of the icon resource of the executable (`build.rs` of `cuboid-app` and
    /// `cuboid-term`).
    const RESOURCE: u16 = 1;
    /// Edge in points of the small icon, `ICON_SMALL`: the title bar and Alt+Tab.
    const SMALL: u32 = 16;
    /// Edge in points of the big icon, `ICON_BIG`: the taskbar button.
    const BIG: u32 = 32;

    /// The size of an icon of `points` on a monitor of `scale_factor`.
    fn size(points: u32, scale_factor: f64) -> PhysicalSize<u32> {
        LogicalSize::new(f64::from(points), f64::from(points)).to_physical(scale_factor)
    }

    /// The icon of the resource at `points`; the system scales the nearest image when the size
    /// is not in the file.
    fn load(points: u32, scale_factor: f64) -> Result<Icon, BadIcon> {
        Icon::from_resource(RESOURCE, Some(size(points, scale_factor)))
    }

    /// The icon, or nothing and a warning that names `what`.
    fn or_warn(what: &str, icon: Result<Icon, BadIcon>) -> Option<Icon> {
        icon.map_err(|err| log::warn!("cannot load the {what} window icon: {err}"))
            .ok()
    }

    /// The small and the big icon for a monitor of `scale_factor`.
    fn icons(scale_factor: f64) -> (Option<Icon>, Option<Icon>) {
        (
            or_warn("small", load(SMALL, scale_factor)),
            or_warn("big", load(BIG, scale_factor)),
        )
    }

    pub(crate) fn set(window: &Window, scale_factor: f64, _picture: Option<&RgbaImage>) {
        let (small, big) = icons(scale_factor);
        if small.is_some() {
            window.set_window_icon(small);
        }
        if big.is_some() {
            window.set_taskbar_icon(big);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_icons_have_the_size_the_system_gives_its_own_at_that_scale() {
            let edges = |scale| {
                let [small, big] = [SMALL, BIG].map(|points| size(points, scale).width);
                (small, big)
            };
            assert_eq!(edges(1.0), (16, 32));
            assert_eq!(edges(1.25), (20, 40));
            assert_eq!(edges(1.5), (24, 48));
            assert_eq!(edges(2.0), (32, 64));
        }

        #[test]
        fn a_missing_resource_is_an_error_and_a_warning_and_never_a_panic() {
            // The test program has no icon resource: the load fails the way a broken build would.
            assert!(load(SMALL, 1.0).is_err());
            assert!(Icon::from_resource(RESOURCE, None).is_err());
            let (small, big) = icons(1.5);
            assert!(small.is_none() && big.is_none());
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use winit::window::{Icon, Window};

    use crate::RgbaImage;

    pub(crate) fn set(window: &Window, _scale_factor: f64, picture: Option<&RgbaImage>) {
        let Some(RgbaImage {
            size: (width, height),
            pixels,
        }) = picture
        else {
            return;
        };
        match Icon::from_rgba(pixels.clone(), *width, *height) {
            Ok(icon) => window.set_window_icon(Some(icon)),
            Err(err) => log::warn!("cannot use the window icon: {err}"),
        }
    }
}
