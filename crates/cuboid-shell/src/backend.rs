//! Which graphics backends and flags the wgpu instance is created with (docs/design/l6_release.md 3).

use wgpu::{Backends, InstanceFlags};

/// What the instance is for: the window, or the offscreen device of `cuboid mcp`.
#[derive(Copy, Clone)]
enum Target {
    Window,
    Offscreen,
}

/// The flags of every instance the app creates, the window's and the offscreen one. The release
/// default also validates indirect draws and dispatches, which builds compute pipelines whenever a
/// device is created. Neither the renderer nor egui makes such a call. `WGPU_*` variables still
/// apply.
pub fn instance_flags() -> InstanceFlags {
    (InstanceFlags::from_build_config() - InstanceFlags::VALIDATION_INDIRECT_CALL).with_env()
}

/// The backends an instance is created with, and the set to try when the first one gives no
/// device.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BackendPlan {
    pub first: Backends,
    /// `None` when the first set is already everything the platform needs, or the user chose it.
    pub fallback: Option<Backends>,
}

impl BackendPlan {
    /// The plan for the window: `WGPU_BACKEND` when it names a backend, else the platform default.
    pub fn window() -> Self {
        Self::of(std::env::consts::OS, Backends::from_env(), Target::Window)
    }

    /// The plan for the offscreen device, which differs from the window's on Windows only.
    pub fn offscreen() -> Self {
        Self::of(
            std::env::consts::OS,
            Backends::from_env(),
            Target::Offscreen,
        )
    }

    fn of(os: &str, env: Option<Backends>, target: Target) -> Self {
        // A list that names no backend is a typo; wgpu has logged it.
        if let Some(chosen) = env.filter(|chosen| !chosen.is_empty()) {
            return Self {
                first: chosen,
                fallback: None,
            };
        }
        match (os, target) {
            // Vulkan is the backend the app has used since L3 and the only one under the idle
            // memory target (133 MB against 164 to 171 with DX12); DX12 and GL cover a machine
            // without a Vulkan driver.
            ("windows", Target::Window) => Self {
                first: Backends::VULKAN,
                fallback: Some(Backends::DX12 | Backends::GL),
            },
            // An instance that also loads Vulkan makes the process end about 45 ms later, and a
            // game or capture tool may have broken its layers; WARP is the software adapter of
            // DX12 on every Windows 10 and later.
            ("windows", Target::Offscreen) => Self {
                first: Backends::DX12,
                fallback: None,
            },
            ("macos", _) => Self {
                first: Backends::METAL,
                fallback: None,
            },
            _ => Self {
                first: Backends::VULKAN | Backends::GL,
                fallback: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: [Target; 2] = [Target::Window, Target::Offscreen];

    #[test]
    fn the_instance_flags_leave_out_the_indirect_validation_unless_the_user_asks_for_it() {
        let asked = std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_some();
        let flags = instance_flags();
        assert!(asked || !flags.contains(InstanceFlags::VALIDATION_INDIRECT_CALL));
    }

    #[test]
    fn the_windows_window_starts_on_vulkan_and_falls_back_to_dx12_and_gl() {
        let plan = BackendPlan::of("windows", None, Target::Window);
        assert_eq!(plan.first, Backends::VULKAN);
        assert_eq!(plan.fallback, Some(Backends::DX12 | Backends::GL));
    }

    #[test]
    fn the_windows_offscreen_device_uses_dx12_alone() {
        let plan = BackendPlan::of("windows", None, Target::Offscreen);
        assert_eq!(plan.first, Backends::DX12);
        assert_eq!(plan.fallback, None);
    }

    #[test]
    fn the_other_systems_have_no_narrower_set_to_fall_back_from() {
        for target in TARGETS {
            let linux = BackendPlan::of("linux", None, target);
            assert_eq!(linux.first, Backends::VULKAN | Backends::GL);
            assert_eq!(linux.fallback, None);
            let mac = BackendPlan::of("macos", None, target);
            assert_eq!(mac.first, Backends::METAL);
            assert_eq!(mac.fallback, None);
        }
    }

    #[test]
    fn a_backend_the_user_chose_is_used_alone_on_every_system() {
        for os in ["windows", "macos", "linux"] {
            for target in TARGETS {
                let plan = BackendPlan::of(os, Some(Backends::DX12), target);
                assert_eq!(plan.first, Backends::DX12, "{os}");
                assert_eq!(plan.fallback, None, "{os}");
            }
        }
    }

    #[test]
    fn a_list_without_a_backend_is_ignored() {
        for target in TARGETS {
            let plan = BackendPlan::of("windows", Some(Backends::empty()), target);
            assert_eq!(plan, BackendPlan::of("windows", None, target));
        }
    }
}
