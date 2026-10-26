//! The paint tools on textures: pencil, eraser, fill and colour picker, with a brush size and an
//! alpha (spec: tools 13.1, 13.2, 13.4).
//!
//! A stroke is one undo step, pushed on its first change. Each pointer move paints a patch the
//! size of the stretch it covers; the texture is never copied or scanned as a whole.

use cuboid_model::{Categories, Snapshot};
use cuboid_num::pack::from_byte;
use cuboid_ops::paint::{
    Canvas, FILL_LIMIT, MAX_BRUSH, MAX_STEP, PaintError, TexKey, flood, segment, texel_at,
    tool_color,
};
use cuboid_scene::{Hit, HitTarget};

use super::ToolCx;
use crate::contract::{Button, CustomTool, StrokeStep};

/// What a paint tool does.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PaintToolKind {
    /// Paints the left colour with the left button, the right colour with the right one (which
    /// erases while it is transparent).
    Pencil,
    /// Clears texels to transparent with either button.
    Eraser,
    /// Floods the connected texels of the clicked colour with the colour of the button.
    Fill,
    /// Sets the left (left button) or right (right button) colour to the texel under the
    /// pointer.
    Picker,
}

/// The settings of a paint tool.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PaintOptions {
    /// Diameter of the pencil and the eraser in texels, 1 to 128.
    pub brush: i32,
    /// Opacity of the painted colour, 0 to 255: it scales the alpha of the tool colour.
    pub alpha: u8,
    /// The most texels one fill may change; a larger region is left alone.
    pub fill_limit: usize,
}

impl Default for PaintOptions {
    fn default() -> Self {
        Self {
            brush: 1,
            alpha: 255,
            fill_limit: FILL_LIMIT,
        }
    }
}

/// The state of the stroke in progress.
struct Stroke {
    /// The texels this stroke writes.
    color: [u8; 4],
    /// The canvas and the texel of the last step, to join the steps with a line.
    last: Option<(Canvas, (i32, i32))>,
    /// The state before the stroke; pushed as the undo step by the first change.
    before: Option<Snapshot>,
}

/// A paint tool for [`Core::set_tool`](crate::Core::set_tool).
pub struct PaintTool {
    kind: PaintToolKind,
    options: PaintOptions,
    stroke: Option<Stroke>,
}

impl PaintTool {
    /// A tool of this kind.
    pub fn new(kind: PaintToolKind, options: PaintOptions) -> Self {
        Self {
            kind,
            options,
            stroke: None,
        }
    }

    fn label(&self) -> &'static str {
        match self.kind {
            PaintToolKind::Eraser => "Erase pixel",
            PaintToolKind::Fill => "Fill pixels",
            _ => "Paint pixel",
        }
    }
}

/// The canvas under a hit, the frame of a strip that the views show at the texture tick, and the
/// texel on it. `None` for a hit that carries no texture coordinate, a voxel or a face without a
/// texture in the list.
fn texel_of(cx: &ToolCx<'_>, hit: &Hit) -> Option<(Canvas, (i32, i32))> {
    let doc = &*cx.edit.doc;
    let key = match (hit.target, hit.face) {
        (HitTarget::Element(id), Some(face)) => TexKey::of_face(doc, id, face)?,
        (HitTarget::Mesh(id), _) => TexKey::of_mesh(doc, id)?,
        _ => return None,
    };
    let canvas = Canvas::of(doc, key, cx.tick)?;
    let image = canvas.key.image(doc)?;
    let texel = texel_at(hit.uv?, image.w, canvas.h)?;
    Some((canvas, texel))
}

impl CustomTool for PaintTool {
    fn begin(&mut self, cx: &mut ToolCx<'_>, button: Button, hit: &Hit) -> StrokeStep {
        self.stroke = None;
        let Some((canvas, texel)) = texel_of(cx, hit) else {
            cx.say("Nothing to paint here");
            return StrokeStep::End;
        };
        if self.kind == PaintToolKind::Picker {
            let picked = canvas
                .view(cx.edit.doc)
                .and_then(|view| view.get(texel.0, texel.1));
            if let Some(color) = picked {
                let color = color.map(from_byte);
                match button {
                    Button::Left => cx.colors.left = color,
                    Button::Right => cx.colors.right = color,
                }
                cx.say("Picked texel colour");
            }
            return StrokeStep::End;
        }
        let tool = match button {
            Button::Left => cx.colors.left,
            Button::Right => cx.colors.right,
        };
        let color = match self.kind {
            PaintToolKind::Eraser => [0; 4],
            _ => tool_color(tool, self.options.alpha),
        };
        self.stroke = Some(Stroke {
            color,
            last: None,
            before: Some(cx.snapshot_of(Categories::TEXTURES_AND_MESHES)),
        });
        self.paint(cx, canvas, texel)
    }

    fn drag(&mut self, cx: &mut ToolCx<'_>, hit: Option<&Hit>) -> StrokeStep {
        if self.stroke.is_none() {
            return StrokeStep::End;
        }
        match hit.and_then(|hit| texel_of(cx, hit)) {
            Some((canvas, texel)) => self.paint(cx, canvas, texel),
            None => {
                // The pointer left the texture: the next step starts a new line.
                if let Some(stroke) = self.stroke.as_mut() {
                    stroke.last = None;
                }
                StrokeStep::Continue
            }
        }
    }

    fn end(&mut self, _cx: &mut ToolCx<'_>) {
        self.stroke = None;
    }
}

impl PaintTool {
    /// Paints at `texel` of `canvas`, joined to the last texel of the stroke on the same canvas.
    fn paint(&mut self, cx: &mut ToolCx<'_>, canvas: Canvas, texel: (i32, i32)) -> StrokeStep {
        let label = self.label();
        let (brush, limit) = (
            self.options.brush.clamp(1, MAX_BRUSH),
            self.options.fill_limit,
        );
        let kind = self.kind;
        let Some(stroke) = self.stroke.as_mut() else {
            return StrokeStep::End;
        };
        if kind == PaintToolKind::Fill {
            let planned = canvas
                .view(cx.edit.doc)
                .map(|view| flood(view, texel, stroke.color, limit));
            match planned {
                Some(Ok(Some(patch))) => {
                    if let Some(before) = stroke.before.take() {
                        cx.push_undo(label, before);
                    }
                    canvas.apply(&mut cx.edit, &patch);
                }
                Some(Err(e @ PaintError::TooLarge(_))) => cx.say(e.to_string()),
                _ => {}
            }
            return StrokeStep::End;
        }
        let from = match stroke.last.take() {
            Some((last_canvas, last)) if last_canvas == canvas => last,
            _ => texel,
        };
        let (dx, dy) = (texel.0 - from.0, texel.1 - from.1);
        let pieces = (dx.abs().max(dy.abs()) + MAX_STEP - 1) / MAX_STEP;
        let pieces = pieces.max(1);
        let mut start = from;
        for k in 1..=pieces {
            let end = (from.0 + dx * k / pieces, from.1 + dy * k / pieces);
            let patch = canvas
                .view(cx.edit.doc)
                .and_then(|view| segment(view, start, end, brush, stroke.color));
            if let Some(patch) = patch {
                if let Some(before) = stroke.before.take() {
                    cx.push_undo(label, before);
                }
                canvas.apply(&mut cx.edit, &patch);
            }
            start = end;
        }
        stroke.last = Some((canvas, texel));
        StrokeStep::Continue
    }
}
