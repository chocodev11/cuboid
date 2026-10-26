//! The issue codes. A code never changes its meaning; a retired rule keeps its name unused.

/// A position, size, rotation or pivot is not a finite number (every target).
pub const NUM_NONFINITE: &str = "NUM_NONFINITE";
/// An element box leaves the -16..32 range of the Java model JSON.
pub const GEO_BOUNDS: &str = "GEO_BOUNDS";
/// An element rotation is not one the version accepts.
pub const GEO_ROTATION: &str = "GEO_ROTATION";
/// An element has a negative size, which Java accepts (inside-out box).
pub const GEO_INVERTED: &str = "GEO_INVERTED";
/// Two faces of different elements lie in one plane, face the same way and overlap.
pub const GEO_ZFIGHT: &str = "GEO_ZFIGHT";
/// A face rectangle reaches outside the texture (0..16).
pub const UV_RANGE: &str = "UV_RANGE";
/// A face has a texel density far from the model median.
pub const UV_DENSITY: &str = "UV_DENSITY";
/// A face cull direction is not a single direction.
pub const FACE_CULL: &str = "FACE_CULL";
/// A visible face names no texture.
pub const TEX_FACE_NONE: &str = "TEX_FACE_NONE";
/// A texture reference names no entry of the texture list.
pub const TEX_UNRESOLVED: &str = "TEX_UNRESOLVED";
/// A texture entry no visible face uses.
pub const TEX_UNUSED: &str = "TEX_UNUSED";
/// A texture entry has no Minecraft path.
pub const TEX_NO_PATH: &str = "TEX_NO_PATH";
/// A texture is not square.
pub const TEX_NOT_SQUARE: &str = "TEX_NOT_SQUARE";
/// The frame of an animated texture is not as wide as its picture, or the strip is not a whole
/// number of frames.
pub const TEX_ANIM_SIZE: &str = "TEX_ANIM_SIZE";
/// A step of an animated texture names a frame the strip does not have, or there is no step.
pub const TEX_ANIM_FRAMES: &str = "TEX_ANIM_FRAMES";
/// A frame time of an animated texture is below one tick.
pub const TEX_ANIM_TIME: &str = "TEX_ANIM_TIME";
/// A cutout face shows semi-transparent pixels.
pub const TEX_ALPHA_CUTOUT: &str = "TEX_ALPHA_CUTOUT";
/// The textures in use hold more colours than the budget.
pub const TEX_COLORS: &str = "TEX_COLORS";
/// A texture path uses the folder names of another game version.
pub const TEX_LAYOUT: &str = "TEX_LAYOUT";
/// A namespace or path uses characters a resource location may not have.
pub const ID_CHARS: &str = "ID_CHARS";
/// The model has more elements than the budget.
pub const BUDGET_ELEMENTS: &str = "BUDGET_ELEMENTS";
/// A display transform is outside what the game accepts.
pub const DISPLAY_RANGE: &str = "DISPLAY_RANGE";
/// The model kind is not the one the target needs.
pub const MODEL_KIND: &str = "MODEL_KIND";
/// The model has no element to write.
pub const MODEL_EMPTY: &str = "MODEL_EMPTY";
/// Item overrides need an item definition from 1.21.4.
pub const MODEL_OVERRIDES: &str = "MODEL_OVERRIDES";
/// The game version is not in the version table.
pub const VERSION_UNKNOWN: &str = "VERSION_UNKNOWN";
/// `.cubik` does not store item overrides.
pub const CUBIK_OVERRIDES: &str = "CUBIK_OVERRIDES";
/// `.cubik` stores face rectangles folded into 0..1.
pub const CUBIK_UV_FOLDED: &str = "CUBIK_UV_FOLDED";
/// `.cubik` writes a zero display scale as 0.0001.
pub const CUBIK_SCALE_ZERO: &str = "CUBIK_SCALE_ZERO";
/// A texture is larger than `.cubik` can store.
pub const CUBIK_TEXTURE_SIZE: &str = "CUBIK_TEXTURE_SIZE";
/// The pack has no usable `pack.mcmeta`.
pub const PACK_MCMETA: &str = "PACK_MCMETA";
/// The declared pack format does not cover the version.
pub const PACK_FORMAT: &str = "PACK_FORMAT";
/// The `pack.mcmeta` keys do not suit the format number.
pub const PACK_FORMAT_KEYS: &str = "PACK_FORMAT_KEYS";
/// The version has no pack format in the table.
pub const PACK_FORMAT_UNKNOWN: &str = "PACK_FORMAT_UNKNOWN";
/// An item model has no item definition file.
pub const PACK_ITEMDEF: &str = "PACK_ITEMDEF";
/// A file the pack needs is missing.
pub const PACK_FILE_MISSING: &str = "PACK_FILE_MISSING";
/// A face's texel density differs from the project's target.
pub const PRJ_DENSITY: &str = "PRJ_DENSITY";
/// A texture is larger than the project allows.
pub const PRJ_TEXTURE_SIZE: &str = "PRJ_TEXTURE_SIZE";
/// A texture uses colours outside the project palette.
pub const PRJ_PALETTE: &str = "PRJ_PALETTE";
/// The model file, a texture or an element is named against the project's naming rule.
pub const PRJ_NAME: &str = "PRJ_NAME";
/// A texture path or the item id is not in the project's namespace.
pub const PRJ_NAMESPACE: &str = "PRJ_NAMESPACE";
