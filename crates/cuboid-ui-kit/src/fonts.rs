//! Fonts: egui is built without its bundled fonts, so the kit installs its own. Inter ships in
//! three weights, each as a family of its own because egui has no synthetic bold; Phosphor is the
//! icon family. A system font is the fallback for scripts Inter lacks.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use cuboid_assets::fonts::{INTER_MEDIUM, INTER_REGULAR, INTER_SEMIBOLD};
use cuboid_assets::phosphor::PHOSPHOR_REGULAR;
use egui::{Context, FontData, FontDefinitions, FontFamily};

/// Font files to install.
#[derive(Clone, Debug, Default)]
pub struct FontSet {
    /// A system font for the scripts Inter lacks, a TTF or OTF file. When `None`, a font of the
    /// operating system is looked for.
    pub ui: Option<Arc<Vec<u8>>>,
    /// More fallback fonts, such as CJK.
    pub fallback: Vec<Arc<Vec<u8>>>,
}

/// Inter weight 500.
pub(crate) static MEDIUM: LazyLock<FontFamily> =
    LazyLock::new(|| FontFamily::Name("cuboid-inter-medium".into()));
/// Inter weight 600.
pub(crate) static SEMIBOLD: LazyLock<FontFamily> =
    LazyLock::new(|| FontFamily::Name("cuboid-inter-semibold".into()));
/// The Phosphor icon font.
pub(crate) static ICONS: LazyLock<FontFamily> =
    LazyLock::new(|| FontFamily::Name("cuboid-icons".into()));

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

/// Installs the fonts: Inter for the proportional and monospace families, one family per other
/// weight, the icon family, and the system fonts as the end of every text chain. Reads at most
/// one system font file, once, at start-up.
pub fn install_fonts(ctx: &Context, set: &FontSet) {
    ctx.set_fonts(definitions(set));
}

fn definitions(set: &FontSet) -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let mut fallback_names = Vec::new();
    let system = set.ui.clone().or_else(|| {
        SYSTEM_FONTS
            .iter()
            .find_map(|path| std::fs::read(Path::new(path)).ok().map(Arc::new))
    });
    if system.is_none() {
        log::warn!("ui: no system fallback font found, scripts Inter lacks will show as boxes");
    }
    for (index, bytes) in system.iter().chain(&set.fallback).enumerate() {
        let name = format!("cuboid-fallback-{index}");
        defs.font_data.insert(
            name.clone(),
            Arc::new(FontData::from_blob(bytes.clone(), 0)),
        );
        fallback_names.push(name);
    }

    let inter = [
        ("cuboid-inter-regular", INTER_REGULAR),
        ("cuboid-inter-medium", INTER_MEDIUM),
        ("cuboid-inter-semibold", INTER_SEMIBOLD),
    ];
    for (name, bytes) in inter {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    defs.font_data.insert(
        "cuboid-phosphor".to_owned(),
        Arc::new(FontData::from_static(PHOSPHOR_REGULAR)),
    );

    let chain = |first: &str| {
        let mut names = vec![first.to_owned()];
        names.extend(fallback_names.iter().cloned());
        names
    };
    defs.families
        .insert(FontFamily::Proportional, chain("cuboid-inter-regular"));
    defs.families
        .insert(FontFamily::Monospace, chain("cuboid-inter-regular"));
    defs.families
        .insert(MEDIUM.clone(), chain("cuboid-inter-medium"));
    defs.families
        .insert(SEMIBOLD.clone(), chain("cuboid-inter-semibold"));
    defs.families
        .insert(ICONS.clone(), vec!["cuboid-phosphor".to_owned()]);
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_weight_and_the_icons_have_a_family() {
        let defs = definitions(&FontSet::default());
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            MEDIUM.clone(),
            SEMIBOLD.clone(),
            ICONS.clone(),
        ] {
            let names = &defs.families[&family];
            assert!(!names.is_empty(), "{family:?} has no font");
            for name in names {
                assert!(defs.font_data.contains_key(name), "{name} has no data");
            }
        }
        assert_eq!(defs.families[&ICONS], ["cuboid-phosphor"]);
        assert_eq!(defs.families[&MEDIUM][0], "cuboid-inter-medium");
        assert_eq!(defs.families[&SEMIBOLD][0], "cuboid-inter-semibold");
        assert_eq!(
            defs.families[&FontFamily::Proportional][0],
            "cuboid-inter-regular"
        );
    }
}
