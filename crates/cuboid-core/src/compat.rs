//! `Compat::port()` and `Compat::strict()` switches (spec: arch 17).

/// The typed compatibility switches. A switch exists only where a specification or the
/// architecture names a decision; every QUIRK is otherwise reproduced unconditionally.
/// [`Compat::port`] is the shipped default, [`Compat::strict`] the profile for parity runs whose
/// reference comes from the original. `parity/compat.toml` mirrors the table of architecture
/// 17.2 and a test asserts the equality.
///
/// Format-profile matters (`[FIX-MODERN]`, `[IMPORT-MODE]`) belong to the profiles of
/// `cuboid-io`, never here.
///
/// spec: arch 17.1, arch 17.2, arch 6 (cuboid-core)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Compat {
    /// Drag and drop goes through the unsaved-changes prompt (port) instead of replacing the
    /// document silently (strict).
    ///
    /// spec: app 0.2 (D6), app 13
    pub dnd_unsaved_prompt: bool,
    /// The recent-files submenu works in a session that started without a settings file (port)
    /// instead of ignoring clicks (strict).
    ///
    /// spec: app 0.2 (D7), app 11
    pub recent_files_without_settings: bool,
    /// The stored custom key bindings are applied at start-up (port) instead of only after
    /// Preferences OK (strict).
    ///
    /// spec: menus 6.6, app 0.2 (D9)
    pub custom_keys_at_startup: bool,
    /// Every key-down is dispatched separately in arrival order (port) instead of the
    /// frame-based quirks: one key per frame, a mouse press cancels the frame (strict).
    ///
    /// spec: menus 6.2, arch 10.3
    pub event_ordered_shortcuts: bool,
    /// Escape closes only a visible message box (port) instead of re-running the handlers of the
    /// last one (strict).
    ///
    /// spec: menus 8.5
    pub escape_only_visible_box: bool,
    /// The Focus key is polled every frame (port) instead of at 50 Hz (strict).
    ///
    /// spec: app 5.5 (f)
    pub focus_poll_every_frame: bool,
    /// The Model Properties OK defect is reproduced (strict, after its runtime check) instead of
    /// fixed (port).
    ///
    /// spec: tools 9.5, tools 18 (item 4)
    pub model_properties_restrict_defect: bool,
    /// Paint strokes recut the atlas as ARGB32 textures (strict) instead of writing edited
    /// textures as RGBA32 (port).
    ///
    /// spec: arch 12.3 (Texture write policy)
    pub atlas_recut_argb32: bool,
    /// Abort in the Progress dialog cancels the running long operation. Off until the owner
    /// signs off deviation D-P3.
    ///
    /// spec: arch 17.3 (D-P3), menus 9.11
    pub abort_cancels_long_ops: bool,
    /// Diagnostic: draw every object with the literal two-pass sequence instead of batching
    /// runs (the proof switch of the batched-equals-literal test).
    ///
    /// spec: arch 13.4
    pub strict_transparency: bool,
}

impl Compat {
    /// The shipped defaults: the recommended fixes are on, the quirks of the original are off.
    pub fn port() -> Self {
        Self {
            dnd_unsaved_prompt: true,
            recent_files_without_settings: true,
            custom_keys_at_startup: true,
            event_ordered_shortcuts: true,
            escape_only_visible_box: true,
            focus_poll_every_frame: true,
            model_properties_restrict_defect: false,
            atlas_recut_argb32: false,
            abort_cancels_long_ops: false,
            strict_transparency: false,
        }
    }

    /// The profile for parity runs against the original: the quirks are reproduced.
    pub fn strict() -> Self {
        Self {
            dnd_unsaved_prompt: false,
            recent_files_without_settings: false,
            custom_keys_at_startup: false,
            event_ordered_shortcuts: false,
            escape_only_visible_box: false,
            focus_poll_every_frame: false,
            model_properties_restrict_defect: true,
            atlas_recut_argb32: true,
            abort_cancels_long_ops: false,
            strict_transparency: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows of the table of architecture 17.2 as `(port, strict)` pairs, in table order.
    fn table(c: &Compat) -> [bool; 10] {
        [
            c.dnd_unsaved_prompt,
            c.recent_files_without_settings,
            c.custom_keys_at_startup,
            c.event_ordered_shortcuts,
            c.escape_only_visible_box,
            c.focus_poll_every_frame,
            c.model_properties_restrict_defect,
            c.atlas_recut_argb32,
            c.abort_cancels_long_ops,
            c.strict_transparency,
        ]
    }

    #[test]
    fn port_values_follow_the_table_of_arch_17_2() {
        let expected = [
            true, true, true, true, true, true, false, false, false, false,
        ];
        assert_eq!(table(&Compat::port()), expected);
    }

    #[test]
    fn strict_values_follow_the_table_of_arch_17_2() {
        let expected = [
            false, false, false, false, false, false, true, true, false, false,
        ];
        assert_eq!(table(&Compat::strict()), expected);
    }
}
