//! The per-frame update of the interaction state machine (spec: tools 8.3-8.6).

use cuboid_model::ObjRef;
use cuboid_ops::transform::{base_of, world_center};
use cuboid_scene::{GizmoPart, Hit, HitTarget, Raycaster, Rect};
use glam::Vec2;

use super::gizmo::Drag;
use super::input::{InputFrame, Modifiers, ViewInput};
use super::{CursorSprite, SelectMode, State, TickOutput, ToolCursor, gizmo_state, select};
use crate::commands::{Effects, Refresh, SelectOp};
use crate::contract::{Button, StrokeStep};
use crate::core::Core;
use crate::interact::Interaction;
use crate::tools::ToolCx;

/// The reach of the hover ray in units.
const HOVER_RAY_LEN: f32 = 1000.0;

/// Pixels the pointer may move between press and release for the release to count as a click.
const CLICK_SLOP: f32 = 4.0;

/// A left press that did not grab a gizmo.
#[derive(Debug)]
pub(crate) struct Armed {
    press: Vec2,
    hovered: Option<ObjRef>,
    /// The pointer moved too far for a click; the release selects nothing.
    moved: bool,
}

/// An open box selection.
#[derive(Debug)]
pub(crate) struct BoxSelect {
    start: Vec2,
    end: Vec2,
    op: SelectOp,
    /// The selection when the box opened, for add and toggle.
    base: Vec<ObjRef>,
}

impl BoxSelect {
    pub(crate) fn rect(&self) -> Rect {
        let min = self.start.min(self.end);
        let size = (self.start - self.end).abs();
        Rect {
            x: min.x,
            y: min.y,
            w: size.x,
            h: size.y,
        }
    }
}

fn object_of(target: HitTarget) -> Option<ObjRef> {
    match target {
        HitTarget::Element(id) => Some(ObjRef::Element(id)),
        HitTarget::Mesh(id) => Some(ObjRef::Mesh(id)),
        HitTarget::Blueprint => Some(ObjRef::Blueprint),
        HitTarget::ModelOrigin => Some(ObjRef::ModelOrigin),
        HitTarget::Voxel => None,
    }
}

/// Runs one frame: hover, gizmo hover, the press that starts a drag, a click or a box, and the
/// continuation of whatever is in progress. Tool strokes take the pointer when a tool is active.
pub(crate) fn update(
    core: &mut Core,
    it: &mut Interaction,
    input: &InputFrame,
    rc: &dyn Raycaster,
) -> TickOutput {
    let mut out = TickOutput::default();
    it.mode = if core.tool.is_some() {
        SelectMode::Custom
    } else if it.mode == SelectMode::Custom {
        SelectMode::Move
    } else {
        it.mode
    };
    let state = std::mem::take(&mut it.state);
    it.state = match state {
        State::Idle => idle(core, it, input, rc, &mut out),
        State::Armed(armed) => armed_frame(core, armed, input, &mut out),
        State::Boxing(boxed) => box_frame(core, boxed, input, &mut out),
        State::Dragging(drag) => drag_frame(core, it, *drag, input, rc, &mut out),
        State::Stroke => stroke_frame(core, input, rc, &mut out),
    };
    if core.tool.is_none() && it.mode == SelectMode::Custom {
        it.mode = SelectMode::Move;
    }
    out.hovered = it.hovered;
    out.select_box = it.select_box();
    out.gizmo = gizmo_state(core, it.mode);
    out.cursor = cursor_for(it, input);
    out
}

fn cursor_for(it: &Interaction, input: &InputFrame) -> ToolCursor {
    let inside = input.view.is_some_and(|v| v.ray.is_some()) && !input.over_ui;
    if !inside {
        return ToolCursor::default();
    }
    let sprite = match it.mode {
        SelectMode::Move => CursorSprite::Move,
        SelectMode::Rotate | SelectMode::Custom => CursorSprite::Ring,
        SelectMode::Scale => CursorSprite::Scale,
    };
    ToolCursor {
        sprite,
        tint: [1.0; 4],
    }
}

/// The view and ray of the pointer, when the pointer works on the viewport.
fn pointer_ray(input: &InputFrame) -> Option<(ViewInput, cuboid_scene::Ray, Vec2)> {
    if input.over_ui {
        return None;
    }
    let view = input.view?;
    Some((view, view.ray?, input.pointer?))
}

fn idle(
    core: &mut Core,
    it: &mut Interaction,
    input: &InputFrame,
    rc: &dyn Raycaster,
    out: &mut TickOutput,
) -> State {
    let Some((view, ray, pointer)) = pointer_ray(input) else {
        it.hovered = None;
        it.active_axis = Default::default();
        return State::Idle;
    };
    let hit = rc.pick_model(ray, HOVER_RAY_LEN);
    if core.tool.is_some() {
        it.hovered = None;
        let button = if input.left.pressed {
            Some(Button::Left)
        } else if input.right.pressed {
            Some(Button::Right)
        } else {
            None
        };
        return match (button, hit) {
            (Some(button), Some(hit)) => begin_stroke(core, button, &hit, out),
            _ => State::Idle,
        };
    }
    it.hovered = hit.and_then(|h| object_of(h.target));
    let gizmo = gizmo_state(core, it.mode).and_then(|state| rc.pick_gizmo(ray, view.id, &state));
    it.active_axis = gizmo.map(|g| g.axes).unwrap_or_default();
    if !input.left.pressed {
        return State::Idle;
    }
    if let Some(grab) = gizmo
        && !matches!(grab.part, GizmoPart::BoxHandle(_))
    {
        let gesture = it.next_gesture;
        if let Some(drag) = Drag::start(core, &grab, ray, pointer, rc, it.mode, gesture) {
            it.next_gesture += 1;
            return State::Dragging(Box::new(drag));
        }
        // A locked or hidden object refuses the drag; the press does not select either.
        return State::Idle;
    }
    State::Armed(Armed {
        press: pointer,
        hovered: it.hovered,
        moved: false,
    })
}

fn armed_frame(
    core: &mut Core,
    mut armed: Armed,
    input: &InputFrame,
    out: &mut TickOutput,
) -> State {
    let pointer = input.pointer.unwrap_or(armed.press);
    if input.left.released || !input.left.down {
        if !armed.moved {
            click(core, armed.hovered, input.mods, out);
        }
        return State::Idle;
    }
    if !armed.moved && pointer.distance(armed.press) > CLICK_SLOP {
        armed.moved = true;
        if armed.hovered.is_none() {
            let op = match (input.mods.ctrl, input.mods.shift) {
                (true, _) => SelectOp::Toggle,
                (false, true) => SelectOp::Add,
                (false, false) => SelectOp::Replace,
            };
            let base = if op == SelectOp::Replace {
                Vec::new()
            } else {
                select::list(&core.sel)
            };
            let mut boxed = BoxSelect {
                start: armed.press,
                end: pointer,
                op,
                base,
            };
            apply_box(core, &mut boxed, input, out);
            return State::Boxing(boxed);
        }
    }
    State::Armed(armed)
}

/// The bone an object belongs to in Animate: a group is its own, an object's is the group that
/// holds it. The blueprint, the model origin and an object outside every group have none.
fn bone_of(doc: &cuboid_model::Document, obj: ObjRef) -> Option<ObjRef> {
    match obj {
        ObjRef::Group(_) => Some(obj),
        ObjRef::Element(_) | ObjRef::Mesh(_) => doc.groups.parent_of(obj).map(ObjRef::Group),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

/// A click: select exclusively, add with shift, toggle with ctrl; empty space clears. In Animate
/// the click lands on the bone of the object.
fn click(core: &mut Core, hovered: Option<ObjRef>, mods: Modifiers, out: &mut TickOutput) {
    let before = (select::list(&core.sel), core.sel.primary);
    let hovered = if core.animate.is_some() {
        let bone = hovered.and_then(|obj| bone_of(&core.doc, obj));
        if bone.is_none() && matches!(hovered, Some(ObjRef::Element(_) | ObjRef::Mesh(_))) {
            core.status.set(
                "Only groups are animated: put the object into a group",
                core.clock.now,
            );
        } else if core
            .animate
            .as_ref()
            .is_some_and(|a| a.animation(&core.doc).is_none())
        {
            core.status
                .set("Add an animation to key the pose", core.clock.now);
        }
        bone
    } else {
        hovered
    };
    match hovered {
        Some(obj) if mods.ctrl => {
            select::apply(&mut core.doc, &mut core.sel, &[obj], SelectOp::Toggle)
        }
        Some(obj) if mods.shift => {
            select::apply(&mut core.doc, &mut core.sel, &[obj], SelectOp::Add)
        }
        Some(obj) => select::set(&mut core.doc, &mut core.sel, &[obj]),
        None if !mods.ctrl && !mods.shift => select::set(&mut core.doc, &mut core.sel, &[]),
        None => {}
    }
    if before != (select::list(&core.sel), core.sel.primary) {
        out.effects.refresh |= Refresh::LISTS | Refresh::GIZMOS;
    }
}

fn box_frame(
    core: &mut Core,
    mut boxed: BoxSelect,
    input: &InputFrame,
    out: &mut TickOutput,
) -> State {
    if let Some(pointer) = input.pointer {
        boxed.end = pointer;
    }
    apply_box(core, &mut boxed, input, out);
    if input.left.released || !input.left.down {
        return State::Idle;
    }
    State::Boxing(boxed)
}

/// Selects the visible objects whose centre projects into the box.
fn apply_box(core: &mut Core, boxed: &mut BoxSelect, input: &InputFrame, out: &mut TickOutput) {
    let Some(view) = input.view else {
        return;
    };
    let rect = boxed.rect();
    let centre = |obj: ObjRef| match &core.animate {
        Some(posed) => posed.centre_of(&core.doc, obj),
        None => base_of(&core.doc, obj).map(world_center),
    };
    let mut inside: Vec<ObjRef> = select::visible_objects(&core.doc)
        .into_iter()
        .filter(|&obj| {
            centre(obj)
                .and_then(|centre| view.project(centre))
                .is_some_and(|p| {
                    p.x >= rect.x
                        && p.x <= rect.x + rect.w
                        && p.y >= rect.y
                        && p.y <= rect.y + rect.h
                })
        })
        .collect();
    if core.animate.is_some() {
        // The groups of the objects the box catches; an object outside every group is no bone.
        let mut bones: Vec<ObjRef> = Vec::new();
        for bone in inside.iter().filter_map(|&obj| bone_of(&core.doc, obj)) {
            if !bones.contains(&bone) {
                bones.push(bone);
            }
        }
        inside = bones;
    }
    let mut chosen = match boxed.op {
        SelectOp::Replace | SelectOp::Remove => inside,
        SelectOp::Add => {
            let mut all = boxed.base.clone();
            all.extend(inside.into_iter().filter(|o| !boxed.base.contains(o)));
            all
        }
        SelectOp::Toggle => {
            let mut all: Vec<ObjRef> = boxed
                .base
                .iter()
                .copied()
                .filter(|o| !inside.contains(o))
                .collect();
            all.extend(inside.into_iter().filter(|o| !boxed.base.contains(o)));
            all
        }
    };
    chosen.dedup();
    let before = select::list(&core.sel);
    select::set(&mut core.doc, &mut core.sel, &chosen);
    if before != select::list(&core.sel) {
        out.effects.refresh |= Refresh::LISTS | Refresh::GIZMOS;
    }
}

fn drag_frame(
    core: &mut Core,
    it: &mut Interaction,
    mut drag: Drag,
    input: &InputFrame,
    rc: &dyn Raycaster,
    out: &mut TickOutput,
) -> State {
    if input.left.released || !input.left.down {
        drag.finish(core);
        out.effects.refresh |= Refresh::GIZMOS | Refresh::LISTS;
        it.active_axis = Default::default();
        return State::Idle;
    }
    let ray = input.view.and_then(|v| v.ray);
    if let (Some(ray), Some(pointer)) = (ray, input.pointer) {
        let was_pushed = drag.pushed();
        if drag.update(core, rc, ray, pointer, it.snap) {
            out.effects.refresh |= Refresh::GIZMOS;
            if !was_pushed {
                out.effects.refresh |= Refresh::EDIT_MENU | Refresh::TITLE | Refresh::LISTS;
            }
        }
    }
    State::Dragging(Box::new(drag))
}

fn begin_stroke(core: &mut Core, button: Button, hit: &Hit, out: &mut TickOutput) -> State {
    match run_tool(core, out, |tool, cx| tool.begin(cx, button, hit)) {
        Some(StrokeStep::Continue) => State::Stroke,
        Some(step) => {
            end_stroke(core, out, step);
            State::Idle
        }
        None => State::Idle,
    }
}

fn stroke_frame(
    core: &mut Core,
    input: &InputFrame,
    rc: &dyn Raycaster,
    out: &mut TickOutput,
) -> State {
    if input.left.released || input.right.released || !(input.left.down || input.right.down) {
        end_stroke(core, out, StrokeStep::End);
        return State::Idle;
    }
    let hit = pointer_ray(input).and_then(|(_, ray, _)| rc.pick_model(ray, HOVER_RAY_LEN));
    match run_tool(core, out, |tool, cx| tool.drag(cx, hit.as_ref())) {
        Some(StrokeStep::Continue) => State::Stroke,
        Some(step) => {
            end_stroke(core, out, step);
            State::Idle
        }
        None => State::Idle,
    }
}

/// Ends the stroke; a tool that asked for it hands over by selecting an object.
fn end_stroke(core: &mut Core, out: &mut TickOutput, step: StrokeStep) {
    run_tool(core, out, |tool, cx| {
        tool.end(cx);
        StrokeStep::End
    });
    if let StrokeStep::SelectLater(obj) = step {
        core.tool = None;
        select::set(&mut core.doc, &mut core.sel, &[obj]);
        out.effects.refresh |= Refresh::LISTS | Refresh::GIZMOS;
    }
}

/// Calls the active tool with the context of a stroke; `None` when no tool is active.
fn run_tool<R>(
    core: &mut Core,
    out: &mut TickOutput,
    call: impl FnOnce(&mut dyn crate::contract::CustomTool, &mut ToolCx<'_>) -> R,
) -> Option<R> {
    let Core {
        doc,
        sel,
        env,
        rng,
        hist,
        dirty,
        colors,
        sizes,
        tool,
        ..
    } = core;
    let tool = tool.as_mut()?;
    let mut effects = Effects::default();
    let result = {
        let mut cx = ToolCx {
            edit: cuboid_model::EditCtx { doc, sel, env, rng },
            history: hist,
            dirty,
            effects: &mut effects,
            colors,
            sizes,
        };
        call(tool.as_mut(), &mut cx)
    };
    out.effects.refresh |= effects.refresh;
    out.effects.colliders.extend(effects.colliders);
    out.effects.notices.extend(effects.notices);
    if effects.status.is_some() {
        out.effects.status = effects.status;
    }
    Some(result)
}
