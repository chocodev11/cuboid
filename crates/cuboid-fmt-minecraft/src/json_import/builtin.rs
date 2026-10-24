//! The parent models that need no file: the `generated` family, which the game extrudes from a
//! sprite, and the common block parents, written here as geometry (interop 8.2, 31.2).
//!
//! The numbers marked verified were checked against the 1.21.11 client; the rest were written
//! from memory and are marked unverified where they stand.

use serde_json::Value;

/// What a built-in parent id stands for.
pub(crate) enum Builtin {
    /// `builtin/generated`, `item/generated` and the handheld items: the game extrudes the sprite
    /// of `layer0`. Holds the lighting and display settings of `item/generated`.
    Generated(Value),
    /// An entity model; nothing to merge.
    Unmerged,
    /// A model with parts of its own, possibly with a parent.
    Model(Value),
}

/// Whether a parent id (without `minecraft:`) is in the `generated` family.
pub(crate) fn is_generated(id: &str) -> bool {
    matches!(
        id,
        "builtin/generated" | "item/generated" | "item/handheld" | "item/handheld_rod"
    )
}

/// `item/generated` (parent `builtin/generated`). Verified: `gui_light` front, ground
/// (translation `[0, 2, 0]`, scale 0.5) and fixed (rotation `[0, 180, 0]`). Unverified: the head,
/// thirdperson_righthand and firstperson_righthand values, and that `item/handheld` and
/// `item/handheld_rod` take them as they are (they have hand transforms of their own).
const GENERATED: &str = r##"{"gui_light":"front","display":{
"ground":{"rotation":[0,0,0],"translation":[0,2,0],"scale":[0.5,0.5,0.5]},
"head":{"rotation":[0,180,0],"translation":[0,13,7],"scale":[1,1,1]},
"thirdperson_righthand":{"rotation":[0,0,0],"translation":[0,3,1],"scale":[0.55,0.55,0.55]},
"firstperson_righthand":{"rotation":[0,-90,25],"translation":[1.13,3.2,1.13],"scale":[0.68,0.68,0.68]},
"fixed":{"rotation":[0,180,0],"translation":[0,0,0],"scale":[1,1,1]}}}"##;

/// `block/block`. Verified: `gui_light` side and the gui display. Unverified and left out: its
/// other display slots (ground, head, fixed, thirdperson, firstperson).
const BLOCK: &str = r##"{"gui_light":"side","display":{
"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.625,0.625,0.625]}}}"##;

/// One element over the whole block, every face culled by its own side. The `particle` entry is
/// unverified.
const CUBE: &str = r##"{"parent":"block/block","textures":{"particle":"#down"},"elements":[
{"from":[0,0,0],"to":[16,16,16],"faces":{
"down":{"texture":"#down","cullface":"down"},"up":{"texture":"#up","cullface":"up"},
"north":{"texture":"#north","cullface":"north"},"south":{"texture":"#south","cullface":"south"},
"west":{"texture":"#west","cullface":"west"},"east":{"texture":"#east","cullface":"east"}}}]}"##;

const CUBE_ALL: &str = r##"{"parent":"block/cube","textures":{"particle":"#all","down":"#all",
"up":"#all","north":"#all","east":"#all","south":"#all","west":"#all"}}"##;

const CUBE_COLUMN: &str = r##"{"parent":"block/cube","textures":{"particle":"#side","down":"#end",
"up":"#end","north":"#side","east":"#side","south":"#side","west":"#side"}}"##;

// Unverified: cube_bottom_top, cube_top and orientable.
const CUBE_BOTTOM_TOP: &str = r##"{"parent":"block/cube","textures":{"particle":"#side",
"down":"#bottom","up":"#top","north":"#side","east":"#side","south":"#side","west":"#side"}}"##;

const CUBE_TOP: &str = r##"{"parent":"block/cube","textures":{"particle":"#side","down":"#top",
"up":"#top","north":"#side","east":"#side","south":"#side","west":"#side"}}"##;

const ORIENTABLE: &str = r##"{"parent":"block/cube","textures":{"particle":"#front","down":"#top",
"up":"#top","north":"#front","east":"#side","south":"#side","west":"#side"}}"##;

/// Two crossed planes, verified. The `particle` entry is unverified.
const CROSS: &str = r##"{"ambientocclusion":false,"textures":{"particle":"#cross"},"elements":[
{"from":[0.8,0,8],"to":[15.2,16,8],"rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
"shade":false,"faces":{"north":{"uv":[0,0,16,16],"texture":"#cross"},"south":{"uv":[0,0,16,16],"texture":"#cross"}}},
{"from":[8,0,0.8],"to":[8,16,15.2],"rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
"shade":false,"faces":{"west":{"uv":[0,0,16,16],"texture":"#cross"},"east":{"uv":[0,0,16,16],"texture":"#cross"}}}]}"##;

// Unverified: the tint index of tinted_cross; its shape is the one of cross.
const TINTED_CROSS: &str = r##"{"ambientocclusion":false,"textures":{"particle":"#cross"},"elements":[
{"from":[0.8,0,8],"to":[15.2,16,8],"rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
"shade":false,"faces":{"north":{"uv":[0,0,16,16],"texture":"#cross","tintindex":0},
"south":{"uv":[0,0,16,16],"texture":"#cross","tintindex":0}}},
{"from":[8,0,0.8],"to":[8,16,15.2],"rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
"shade":false,"faces":{"west":{"uv":[0,0,16,16],"texture":"#cross","tintindex":0},
"east":{"uv":[0,0,16,16],"texture":"#cross","tintindex":0}}}]}"##;

/// The built-in parent for an id such as `block/cube_all`; the `minecraft:` prefix is already
/// gone. `None` for a parent that must come from a file.
pub(crate) fn lookup(id: &str) -> Option<Builtin> {
    let source = match id {
        _ if is_generated(id) => GENERATED,
        "builtin/entity" => return Some(Builtin::Unmerged),
        "block/block" => BLOCK,
        "block/cube" => CUBE,
        "block/cube_all" => CUBE_ALL,
        "block/cube_column" => CUBE_COLUMN,
        "block/cube_bottom_top" => CUBE_BOTTOM_TOP,
        "block/cube_top" => CUBE_TOP,
        "block/orientable" => ORIENTABLE,
        "block/cross" => CROSS,
        "block/tinted_cross" => TINTED_CROSS,
        _ => return None,
    };
    let json = serde_json::from_str(source).ok()?;
    Some(if is_generated(id) {
        Builtin::Generated(json)
    } else {
        Builtin::Model(json)
    })
}
