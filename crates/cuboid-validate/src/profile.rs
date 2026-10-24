//! What to validate against: the target, the budgets and the pack contents.

use cuboid_assets::mc_versions::{
    McVersion, PackFormat, RotationRule, TextureDirs, Tier, mc_versions,
};
use cuboid_model::ModelKind;

use crate::issue::Severity;

/// The output the model is checked for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// A `.cubik` file: only what Cubik cannot store is reported.
    Cubik,
    /// A Java block model for a game version.
    JavaBlock {
        /// The game version.
        version: McVersion,
    },
    /// A Java item model for a game version.
    JavaItem {
        /// The game version.
        version: McVersion,
    },
    /// A `.bbmodel` project. With `java_block` it follows the Java block rules every version
    /// accepts; free-form models take any geometry.
    Bbmodel {
        /// Java block format instead of the free format.
        java_block: bool,
    },
    /// A resource pack for a game version; the block or item kind comes from the document.
    ResourcePack {
        /// The game version.
        version: McVersion,
    },
}

impl Target {
    /// The game version, for the targets that have one.
    pub fn version(&self) -> Option<McVersion> {
        match *self {
            Target::JavaBlock { version }
            | Target::JavaItem { version }
            | Target::ResourcePack { version } => Some(version),
            Target::Cubik | Target::Bbmodel { .. } => None,
        }
    }
}

/// How a face is drawn, which decides whether semi-transparent pixels are a defect.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RenderType {
    /// A face is cutout when its texture crop has a fully transparent pixel.
    Auto,
    /// Every face is cutout.
    Cutout,
    /// Every face is translucent; semi-transparent pixels are intended.
    Translucent,
}

/// The `pack.mcmeta` the pack declares.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackMeta {
    /// `pack_format`.
    pub pack_format: Option<PackFormat>,
    /// `min_format`.
    pub min_format: Option<PackFormat>,
    /// `max_format`.
    pub max_format: Option<PackFormat>,
    /// The deprecated `supported_formats` key is present.
    pub supported_formats: bool,
}

/// The model the pack carries.
#[derive(Clone, Debug, PartialEq)]
pub struct PackModel {
    /// Namespace, `minecraft` for vanilla.
    pub namespace: String,
    /// Model id, possibly with folders.
    pub id: String,
}

/// What is known about the pack: the app reads it from the folder or archive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackInfo {
    /// The parsed `pack.mcmeta`, `None` when the file is missing.
    pub meta: Option<PackMeta>,
    /// Every file path in the pack, `/` separated, without a leading slash.
    pub files: Vec<String>,
    /// The model the pack should carry; without it the file checks are skipped.
    pub model: Option<PackModel>,
}

/// The target and the settings of one validation run.
#[derive(Clone, Debug)]
pub struct Profile {
    /// What to check for.
    pub target: Target,
    /// Distance in model units that the export adds around every element, for example the jitter
    /// of "Avoid z-fighting" (interop 7.5). The bounds check treats the box as this much larger.
    pub inflate: f32,
    /// Most elements allowed; no limit when `None`. The game sets none, so this is a budget.
    pub max_elements: Option<u32>,
    /// Most distinct colours over the textures in use; no limit when `None`.
    pub max_colors: Option<u32>,
    /// Texel density may differ from the model median by this factor before it is reported.
    pub texel_ratio: f32,
    /// How faces with transparent pixels are drawn.
    pub render_type: RenderType,
    /// The pack contents, for the [`Target::ResourcePack`] rules.
    pub pack: Option<PackInfo>,
    /// Issues of one code beyond this number are dropped and counted in one summary.
    pub max_issues_per_rule: usize,
    /// Issues below this severity are not reported, and rules that cannot reach it are skipped.
    pub min_severity: Severity,
    /// Codes to skip.
    pub disabled: Vec<String>,
    /// When not empty, run only these codes.
    pub only: Vec<&'static str>,
}

impl Profile {
    /// The default settings for a target.
    pub fn new(target: Target) -> Self {
        Self {
            target,
            inflate: 0.0,
            max_elements: None,
            max_colors: None,
            texel_ratio: 1.5,
            render_type: RenderType::Auto,
            pack: None,
            max_issues_per_rule: 500,
            min_severity: Severity::Info,
            disabled: Vec::new(),
            only: Vec::new(),
        }
    }

    pub(crate) fn params(&self) -> Params {
        let mut params = Params {
            flavor: Flavor::Java,
            version: self.target.version(),
            tier: None,
            rotation: RotationRule::Classic,
            rotation_max: 45.0,
            rotation_verified: true,
            dirs: None,
            item_defs: false,
            item_defs_verified: true,
            kind: match self.target {
                Target::JavaBlock { .. } => Some(ModelKind::Block),
                Target::JavaItem { .. } => Some(ModelKind::Item),
                _ => None,
            },
            pack: matches!(self.target, Target::ResourcePack { .. }),
        };
        match self.target {
            Target::Cubik => params.flavor = Flavor::Cubik,
            Target::Bbmodel { java_block: false } => params.flavor = Flavor::Free,
            Target::Bbmodel { java_block: true } => {}
            Target::JavaBlock { version }
            | Target::JavaItem { version }
            | Target::ResourcePack { version } => match mc_versions().tier_for(version) {
                Some(tier) => params.apply_tier(tier),
                // An unknown version gets the strictest rotation rule and no folder rule, as a
                // warning only.
                None => params.rotation_verified = false,
            },
        }
        params
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Flavor {
    Cubik,
    Java,
    Free,
}

/// The rules a profile resolves to.
#[derive(Clone, Debug)]
pub(crate) struct Params {
    pub flavor: Flavor,
    pub version: Option<McVersion>,
    pub tier: Option<&'static Tier>,
    pub rotation: RotationRule,
    pub rotation_max: f32,
    pub rotation_verified: bool,
    pub dirs: Option<TextureDirs>,
    pub item_defs: bool,
    pub item_defs_verified: bool,
    /// The kind the target demands, for the Java block and item targets.
    pub kind: Option<ModelKind>,
    pub pack: bool,
}

impl Params {
    fn apply_tier(&mut self, tier: &'static Tier) {
        self.tier = Some(tier);
        self.rotation = tier.rotation;
        self.rotation_max = tier.rotation_max_deg.unwrap_or(45.0);
        self.rotation_verified = tier.is_verified("rotation");
        self.dirs = Some(tier.texture_dirs);
        self.item_defs = tier.item_definitions;
        self.item_defs_verified = tier.is_verified("item_definitions");
    }
}
