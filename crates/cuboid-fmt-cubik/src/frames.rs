//! The animated textures in the `cuboid.json` entry (spec: textures T1 3).
//!
//! The payload of a `.cubik` file knows no animation: the record of an animated texture holds its
//! first frame, so Cubik shows a still picture. The entry keeps, per animated texture, the strip
//! as PNG data and the animation, with a digest of the first frame that the payload holds. When
//! the texture of the payload does not match the digest any more (Cubik painted on it, or another
//! program rewrote the file), the strip is stale: it is left out with a warning and the texture
//! stays the still picture of the payload.

use std::sync::Arc;

use cuboid_io::text::base64;
use cuboid_model::image::{Image, ImageFileKind, decode_file, encode_png};
use cuboid_model::{FrameStep, Snapshot, TextureAnimation, TextureEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Limits;
use crate::container::MAX_EXTENSION;

fn no(flag: &bool) -> bool {
    !*flag
}

#[derive(Debug, Serialize, Deserialize)]
struct StepNode {
    index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Node {
    /// The label of the texture entry.
    label: String,
    /// Digest of the first frame of the strip, which is the texture of the payload.
    frame: String,
    /// The whole strip as a PNG file, in base64.
    png: String,
    frame_height: u32,
    frametime: u32,
    #[serde(default, skip_serializing_if = "no")]
    interpolate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frames: Option<Vec<StepNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<String>,
}

/// The `textures` member of the entry.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct List {
    strips: Vec<Node>,
}

/// A short digest of the size and the texels of a picture.
pub(crate) fn digest(image: &Image) -> String {
    let mut hash = Sha256::new();
    hash.update(image.w.to_le_bytes());
    hash.update(image.h.to_le_bytes());
    hash.update(image.rgba());
    hash.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The strips of the animated textures of a snapshot as the entry writes them; `None` when it has
/// none. A strip that cannot be encoded as PNG is left out: its texture is written as the first
/// frame only.
pub(crate) fn encode(snapshot: &Snapshot) -> Option<List> {
    let strips: Vec<Node> = snapshot
        .textures
        .as_deref()?
        .entries
        .iter()
        .filter_map(|entry| {
            let animation = entry.animation.as_ref()?;
            let png = encode_png(&entry.texture.image).ok()?;
            Some(Node {
                label: entry.label.to_string(),
                frame: digest(&entry.frame(0)),
                png: base64::encode(&png),
                frame_height: animation.frame_height,
                frametime: animation.frametime,
                interpolate: animation.interpolate,
                frames: animation.frames.as_ref().map(|steps| {
                    steps
                        .iter()
                        .map(|s| StepNode {
                            index: s.index,
                            time: s.time,
                        })
                        .collect()
                }),
                extra: animation.extra.as_ref().map(|e| e.to_string()),
            })
        })
        .collect();
    (!strips.is_empty()).then_some(List { strips })
}

/// Gives the textures of the sections their strips and animations. A strip that does not belong
/// to the payload texture, or is damaged, is left out and counted in the warnings.
pub(crate) fn decode(
    value: serde_json::Value,
    sections: &mut Snapshot,
    limits: &Limits,
) -> Vec<String> {
    let list: List = match serde_json::from_value(value) {
        Ok(list) => list,
        Err(error) => {
            return vec![format!(
                "the animated textures in the `cuboid.json` entry are damaged and ignored ({error})"
            )];
        }
    };
    let entries: &mut [TextureEntry] = match sections.textures.as_mut() {
        Some(textures) => &mut Arc::make_mut(textures).entries,
        None => &mut [],
    };
    let (mut stale, mut damaged) = (0_usize, 0_usize);
    let mut texels = 0_u64;
    for node in list.strips {
        let entry = entries
            .iter_mut()
            .find(|e| *e.label == *node.label && e.animation.is_none());
        let Some(entry) = entry.filter(|e| digest(&e.texture.image) == node.frame) else {
            stale += 1;
            continue;
        };
        let Some((strip, animation)) = strip_of(&node, limits, &mut texels) else {
            damaged += 1;
            continue;
        };
        // The first frame of the strip must be the texture the digest was made for.
        let still = std::mem::replace(&mut entry.texture.image, Arc::new(strip));
        entry.animation = Some(animation);
        if digest(&entry.frame(0)) != node.frame {
            entry.texture.image = still;
            entry.animation = None;
            damaged += 1;
        }
    }
    let mut warnings = Vec::new();
    if stale > 0 {
        warnings.push(format!(
            "{stale} animated textures in the `cuboid.json` entry do not belong to the textures of \
             the file, which was changed after cuboid saved it; they are shown as the picture the \
             file holds"
        ));
    }
    if damaged > 0 {
        warnings.push(format!(
            "{damaged} animated textures in the `cuboid.json` entry are damaged or over the limits \
             of a model and were left out"
        ));
    }
    warnings
}

/// The strip of a node and its animation; `texels` counts what the strips of the file hold.
fn strip_of(node: &Node, limits: &Limits, texels: &mut u64) -> Option<(Image, TextureAnimation)> {
    let max = usize::try_from(MAX_EXTENSION).ok()?;
    let bytes = base64::decode(&node.png, max)?;
    let image = decode_file(&bytes, ImageFileKind::Other).ok()?;
    *texels = texels.saturating_add(u64::from(image.w) * u64::from(image.h));
    if *texels > limits.max_texels {
        return None;
    }
    let animation = TextureAnimation {
        frame_width: image.w,
        frame_height: node.frame_height,
        frametime: node.frametime,
        interpolate: node.interpolate,
        frames: node.frames.as_ref().map(|steps| {
            steps
                .iter()
                .map(|s| FrameStep {
                    index: s.index,
                    time: s.time,
                })
                .collect()
        }),
        extra: node.extra.as_deref().map(Arc::from),
    };
    animation.check(image.w, image.h).ok()?;
    Some((image, animation))
}
