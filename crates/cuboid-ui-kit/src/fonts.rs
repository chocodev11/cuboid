//! Fonts: egui is built without its bundled fonts, so the application hands in its own.

use std::path::Path;
use std::sync::Arc;

use egui::{Context, FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};

/// Font files to install.
#[derive(Clone, Debug, Default)]
pub struct FontSet {
    /// The interface font, a TTF or OTF file (the app embeds Open Sans from `cuboid-assets`).
    /// When `None`, a font of the operating system is looked for.
    pub ui: Option<Arc<Vec<u8>>>,
    /// Extra fonts used for glyphs the interface font lacks, such as CJK.
    pub fallback: Vec<Arc<Vec<u8>>>,
}

/// Files tried when the app supplies no font; the first readable one wins.
const SYSTEM_FONTS: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

/// Installs the fonts for the proportional and monospace families; returns whether any font was
/// installed. Reads at most one system font file, once, at start-up.
pub fn install_fonts(ctx: &Context, set: &FontSet) -> bool {
    let primary = set.ui.clone().or_else(|| {
        SYSTEM_FONTS
            .iter()
            .find_map(|path| match std::fs::read(Path::new(path)) {
                Ok(bytes) => Some(Arc::new(bytes)),
                Err(_) => None,
            })
    });
    let mut installed = false;
    if let Some(bytes) = primary {
        add(ctx, "cuboid-ui", bytes, FontPriority::Highest);
        installed = true;
    } else {
        log::warn!("ui: no interface font found, text will show as boxes");
    }
    for (index, bytes) in set.fallback.iter().enumerate() {
        add(
            ctx,
            &format!("cuboid-fallback-{index}"),
            bytes.clone(),
            FontPriority::Lowest,
        );
        installed = true;
    }
    installed
}

fn add(ctx: &Context, name: &str, bytes: Arc<Vec<u8>>, priority: FontPriority) {
    let families = [FontFamily::Proportional, FontFamily::Monospace]
        .into_iter()
        .map(|family| InsertFontFamily { family, priority })
        .collect();
    ctx.add_font(FontInsert::new(
        name,
        FontData::from_blob(bytes, 0),
        families,
    ));
}
