//! Icons: the Phosphor Regular font (MIT), mapped once in the [`Icon`] enum. Each variant names
//! its Phosphor icon; the codepoint comes from the generated official map in `cuboid-assets`.
//! Only the app mark and the view gizmo are still painted by hand, outside the kit.

use cuboid_assets::phosphor::codepoint;
use egui::{Align2, Color32, FontId, Painter, Rect};

use crate::fonts::ICONS;

macro_rules! icons {
    ($($variant:ident => $name:literal,)*) => {
        /// Every icon of the UI, with its Phosphor name.
        #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            /// Every icon.
            pub const ALL: &'static [Icon] = &[$(Icon::$variant,)*];

            /// The icon's name in the Phosphor set.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Icon::$variant => $name,)*
                }
            }
        }
    };
}

icons! {
    Plus => "plus",
    Minus => "minus",
    Close => "x",
    Check => "check",
    ChevronRight => "caret-right",
    ChevronDown => "caret-down",
    ChevronUp => "caret-up",
    ChevronLeft => "caret-left",
    CaretUpDown => "caret-up-down",
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    ArrowUp => "arrow-up",
    ArrowDown => "arrow-down",
    Menu => "list",
    More => "dots-three",
    Search => "magnifying-glass",
    Settings => "gear",
    Home => "house",
    File => "file",
    FilePlus => "file-plus",
    Folder => "folder",
    FolderOpen => "folder-open",
    Save => "floppy-disk",
    Import => "download-simple",
    Export => "upload-simple",
    Undo => "arrow-u-up-left",
    Redo => "arrow-u-up-right",
    Copy => "copy",
    Trash => "trash",
    Eye => "eye",
    EyeOff => "eye-slash",
    Lock => "lock-simple",
    Unlock => "lock-simple-open",
    Cube => "cube",
    Plane => "parallelogram",
    Group => "folder-simple",
    Mesh => "polygon",
    Voxel => "cube-transparent",
    Select => "cursor",
    BoxSelect => "selection",
    Move => "arrows-out-cardinal",
    Rotate => "arrows-clockwise",
    Scale => "arrows-out-simple",
    Pencil => "pencil-simple",
    Eraser => "eraser",
    Fill => "paint-bucket",
    Picker => "eyedropper",
    Line => "line-segment",
    Rect => "rectangle",
    Mirror => "flip-horizontal",
    Magnet => "magnet",
    Grid => "grid-four",
    Palette => "palette",
    Image => "image",
    Monitor => "monitor",
    History => "clock-counter-clockwise",
    Frame => "frame-corners",
    Warning => "warning",
    Info => "info",
    Error => "x-circle",
    CheckCircle => "check-circle",
    PanelLeft => "sidebar-simple",
    PanelRight => "square-half",
    PanelBottom => "square-half-bottom",
    Keyboard => "keyboard",
    Pin => "push-pin",
    Filter => "funnel",
    Sort => "sort-ascending",
    Clock => "clock",
    Layers => "stack",
    Texture => "checkerboard",
    FaceUv => "cube-focus",
    Display => "monitor-play",
    Outliner => "tree-structure",
    Validator => "seal-check",
    Sliders => "sliders-horizontal",
    Camera => "camera",
    Hand => "hand",
    Play => "play",
    Pause => "pause",
    Stop => "stop",
    Repeat => "repeat",
    Film => "film-strip",
    ListView => "list-bullets",
    GridView => "squares-four",
    Package => "package",
    Library => "books",
    Link => "link",
    Tools => "wrench",
    Help => "question",
    Star => "star",
    Brush => "paint-brush",
    Lightbulb => "lightbulb",
    Sword => "sword",
    Sun => "sun",
    Wand => "magic-wand",
    FlipVertical => "flip-vertical",
    RotateRight => "arrow-clockwise",
    Reload => "arrow-counter-clockwise",
    Replace => "swap",
}

impl Icon {
    /// The private-use character that shows the icon in the Phosphor font.
    pub fn glyph(self) -> char {
        codepoint(self.name()).unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// Draws the icon centred in `rect`, as large as the shorter side of `rect`.
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let size = rect.width().min(rect.height());
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.glyph(),
            FontId::new(size, ICONS.clone()),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cuboid_assets::phosphor::PHOSPHOR_REGULAR;

    use super::*;

    /// Whether the font maps `ch` to a glyph: a reader of the `cmap` table, formats 4 and 12.
    fn font_has_glyph(font: &[u8], ch: char) -> bool {
        let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
        let u32_at =
            |at: usize| u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]]);
        let tables = usize::from(u16_at(4));
        let cmap = (0..tables)
            .map(|i| 12 + i * 16)
            .find(|&at| &font[at..at + 4] == b"cmap")
            .map(|at| u32_at(at + 8) as usize)
            .expect("a cmap table");
        let code = ch as u32;
        (0..usize::from(u16_at(cmap + 2)))
            .map(|i| cmap + u32_at(cmap + 4 + i * 8 + 4) as usize)
            .any(|sub| match u16_at(sub) {
                4 => {
                    let segs = usize::from(u16_at(sub + 6)) / 2;
                    let (ends, starts) = (sub + 14, sub + 16 + segs * 2);
                    (0..segs).any(|s| {
                        let (end, start) = (
                            u32::from(u16_at(ends + s * 2)),
                            u32::from(u16_at(starts + s * 2)),
                        );
                        code >= start && code <= end && code != 0xFFFF
                    })
                }
                12 => {
                    let groups = u32_at(sub + 12) as usize;
                    (0..groups).any(|g| {
                        let at = sub + 16 + g * 12;
                        (u32_at(at)..=u32_at(at + 4)).contains(&code)
                    })
                }
                _ => false,
            })
    }

    #[test]
    fn every_icon_is_in_the_official_map_and_in_the_font() {
        let mut seen = HashSet::new();
        for &icon in Icon::ALL {
            assert!(seen.insert(icon.name()), "{} is used twice", icon.name());
            let glyph = codepoint(icon.name())
                .unwrap_or_else(|| panic!("{icon:?}: `{}` is not a Phosphor name", icon.name()));
            assert_eq!(icon.glyph(), glyph);
            assert!(
                ('\u{e000}'..='\u{f8ff}').contains(&glyph),
                "{icon:?} is not in the private use area"
            );
            assert!(
                font_has_glyph(PHOSPHOR_REGULAR, glyph),
                "{icon:?}: the font has no glyph for {}",
                icon.name()
            );
        }
        assert!(Icon::ALL.len() >= 60);
    }

    #[test]
    fn the_reader_rejects_a_missing_glyph() {
        assert!(!font_has_glyph(PHOSPHOR_REGULAR, 'A'));
        assert!(!font_has_glyph(PHOSPHOR_REGULAR, '\u{f8fe}'));
    }
}
