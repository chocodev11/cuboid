//! The timeline of the Animate mode: the bones and channels of the chosen animation as rows, the
//! keyframes as marks, the ruler and the playhead (spec: animate A3 3).
//!
//! The timeline only describes and asks. The app hands in the bones with their keyframes
//! ([`TimelineView`]) when the animations, the groups or the bone selection change; the timeline
//! keeps what is its own: the zoom, the scroll, the folded bones and the selected keyframes, which
//! it names by bone, channel and time ([`KeyId`]). Edits go out as [`AnimateEdit`], the playhead
//! as [`AnimationRequest`].

use std::collections::HashSet;
use std::ops::Range;

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    DragFieldOpts, EmptyState, Icon, IconButton, MenuItem, Tokens, TreeRow, Type, drag_field,
    format_number, submenu, text, tree_row,
};
use egui::{
    Align, Align2, Color32, Layout, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder,
    pos2, vec2,
};

use crate::common::Sink;
use crate::model::{
    AnimationRow, AnimationsView, BoneRow, ChannelKind, KeyKind, KeyMark, NodeId, TimelineView,
};
use crate::request::{AnimateEdit, AnimationRequest, Edit, KeyId, SelectOp, UiRequest};

/// Width of the column of bone and channel names.
const LEFT_WIDTH: f32 = 200.0;
/// Height of a row and of the ruler.
const ROW: f32 = height::ROW;
/// Half the size of a mark.
const MARK: f32 = 5.0;
/// How near the pointer must be to a mark to take it.
const REACH: f32 = 7.0;
/// Marks closer than this many points are one.
const MERGE: f32 = 2.0;
/// The slowest and the fastest zoom, in points per second.
const ZOOM: (f32, f32) = (10.0, 2000.0);
/// Room left at both ends when the animation is fitted to the width.
const FIT_MARGIN: f32 = space::LG;
/// From this many points per frame on the ruler counts frames.
const FRAME_MIN: f32 = 6.0;
/// The distance between two labels of the ruler.
const LABEL_GAP: f32 = 64.0;
/// The longest an animation can be: no keyframe moves past it.
const LONGEST: f32 = 3_600.0;

/// A row of the timeline: a bone, or one of its channels.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Row {
    bone: usize,
    channel: Option<ChannelKind>,
}

/// The marks of a row that fall on the same point: the keyframes of each channel from `ranges`.
#[derive(Clone, Debug, PartialEq)]
struct Cluster {
    x: f32,
    ranges: [Range<usize>; 3],
}

/// A pointer drag that started in the time area.
#[derive(Clone, Debug)]
enum Drag {
    /// The press began on the ruler: the playhead follows the pointer.
    Scrub,
    /// The press began on a selected mark: the selection follows the pointer.
    Marks {
        /// The mark that was grabbed; it lands on the frame grid.
        anchor: f32,
        /// The earliest and the latest of the selected keyframes.
        span: (f32, f32),
        /// The shift so far, in seconds.
        delta: f32,
    },
    /// The press began on empty space: a rubber band from the press.
    Band {
        /// Where the press was.
        origin: Pos2,
    },
}

/// The state of the timeline between frames.
#[derive(Default)]
pub(crate) struct TimelineState {
    /// Points per second.
    zoom: f32,
    /// Seconds at the left edge of the time area.
    origin: f32,
    /// Points the rows are scrolled up.
    scroll: f32,
    /// The animation the zoom was fitted for.
    fitted: Option<u64>,
    /// Bones whose channels are folded into the bone row.
    collapsed: HashSet<NodeId>,
    /// Changes whenever `collapsed` does.
    collapsed_rev: u64,
    selected: Vec<KeyId>,
    /// The revision of the view in which the selected keyframes were sent to move. Until a view
    /// of another revision comes, the selection is not checked against the view, which does not
    /// have the move yet (egui may draw a frame twice).
    moving: Option<u64>,
    drag: Option<Drag>,
    /// The playhead time last asked for by a scrub, so a still pointer asks for nothing.
    sought: Option<f32>,
    /// The mark under the pointer when the context menu opened.
    menu: Option<Vec<KeyId>>,
    rows: Vec<Row>,
    rows_key: Option<(u64, u64)>,
    /// The height of the rows area, for the scroll limit.
    view_height: f32,
}

/// The whole number a position on the rows or the ruler stands for; negative and out-of-range
/// values saturate.
#[allow(clippy::cast_possible_truncation)]
fn whole(value: f32) -> i64 {
    value as i64
}

/// The row a distance from the top of the rows falls on, 0 above them.
fn row_index(distance: f32) -> usize {
    usize::try_from(whole((distance / ROW).floor())).unwrap_or(0)
}

fn key_bits(key: &KeyId) -> (NodeId, ChannelKind, u32) {
    (key.bone, key.channel, key.time.to_bits())
}

/// The smallest of 1, 2 and 5 times a power of ten that is at least `least`.
fn nice_step(least: f32) -> f32 {
    let mut step = 0.001_f32;
    while step < 1.0e6 {
        for factor in [1.0, 2.0, 5.0] {
            if step * factor >= least {
                return step * factor;
            }
        }
        step *= 10.0;
    }
    step
}

impl TimelineState {
    /// The keyframes that are selected.
    pub fn selected(&self) -> &[KeyId] {
        &self.selected
    }

    /// Selects keyframes, for the tests of the panels that read the selection.
    #[cfg(test)]
    pub fn select_keys(&mut self, keys: Vec<KeyId>) {
        self.selected = keys;
    }

    /// Takes the selection out, for keyframes that are about to go.
    pub fn take_selection(&mut self) -> Vec<KeyId> {
        std::mem::take(&mut self.selected)
    }

    /// Whether a keyframe of the model is selected here.
    fn is_selected(&self, key: &KeyId) -> bool {
        self.selected.iter().any(|s| key_bits(s) == key_bits(key))
    }

    /// Moves the selection along with keyframes that a request sent with the view of revision
    /// `rev` moves by `delta`.
    pub fn follow_move(&mut self, delta: f32, rev: u64) {
        for key in &mut self.selected {
            key.time += delta;
        }
        self.moving = Some(rev);
    }

    /// Takes a selection that no longer fits the model out: keyframes that are gone, and times
    /// that moved by less than the time two keyframes must be apart.
    fn prune(&mut self, view: &TimelineView) {
        self.selected.retain_mut(|key| match find_key(view, key) {
            Some(found) => {
                key.time = found.time;
                true
            }
            None => false,
        });
    }

    /// Fits the length of the animation to the width of the time area.
    fn fit(&mut self, length: f32, width: f32) {
        let room = (width - 2.0 * FIT_MARGIN).max(1.0);
        self.zoom = (room / length.max(0.1)).clamp(ZOOM.0, ZOOM.1);
        self.origin = -FIT_MARGIN / self.zoom;
    }

    /// The rows of the bones, in order; rebuilt when the bones or the folds change.
    fn flatten(&mut self, view: &TimelineView) {
        let key = (view.rev, self.collapsed_rev);
        if self.rows_key == Some(key) {
            return;
        }
        self.rows_key = Some(key);
        self.rows.clear();
        for (bone, row) in view.bones.iter().enumerate() {
            self.rows.push(Row {
                bone,
                channel: None,
            });
            if !self.collapsed.contains(&row.id) {
                self.rows.extend(ChannelKind::ALL.map(|channel| Row {
                    bone,
                    channel: Some(channel),
                }));
            }
        }
    }

    /// Draws the timeline and sends what the user asks for.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        animations: Option<&AnimationsView>,
        view: Option<&TimelineView>,
        sink: &mut Sink,
    ) {
        let current = animations.and_then(AnimationsView::current);
        let (Some(animations), Some(view), Some(current)) = (animations, view, current) else {
            // Keyframes of no animation are no selection: Delete, Copy and Paste mean the
            // objects again.
            self.selected.clear();
            self.moving = None;
            if animations.is_none() || view.is_none() {
                return;
            }
            let empty = EmptyState::new(Icon::Film, "No animation")
                .body("Add an animation to key the pose.")
                .action(cuboid_ui_kit::Button::new("New animation").primary())
                .show(ui);
            if empty.is_some() {
                sink.push(UiRequest::Animate(AnimateEdit::NewAnimation));
            }
            return;
        };
        if self.moving != Some(view.rev) {
            self.moving = None;
            self.prune(view);
        }
        self.flatten(view);
        self.header(ui, animations, current, sink);
        let body = ui.available_rect_before_wrap();
        let (left, area) = (
            Rect::from_min_size(body.min, vec2(LEFT_WIDTH.min(body.width()), body.height())),
            Rect::from_min_max(pos2(body.left() + LEFT_WIDTH, body.top()), body.max),
        );
        if self.fitted != Some(current.id) {
            self.fitted = Some(current.id);
            self.fit(current.length, area.width());
        }
        let rows_area = Rect::from_min_max(pos2(area.left(), area.top() + ROW), area.max);
        self.view_height = rows_area.height();
        let content = self.rows.len() as f32 * ROW;
        self.scroll = self
            .scroll
            .clamp(0.0, (content - self.view_height).max(0.0));

        self.left_column(ui, left, view, sink);
        self.time_area(ui, area, animations, current, view, sink);
        ui.advance_cursor_after_rect(body);
    }

    /// Play, pause, stop, loop, the playhead and the fit button.
    fn header(
        &mut self,
        ui: &mut Ui,
        animations: &AnimationsView,
        current: &AnimationRow,
        sink: &mut Sink,
    ) {
        let ask = |sink: &mut Sink, request| sink.push(UiRequest::Animation(request));
        ui.horizontal(|ui| {
            ui.set_height(height::BAR);
            let (icon, tip, request) = if animations.playing {
                (Icon::Pause, "Pause", AnimationRequest::Pause)
            } else {
                (Icon::Play, "Play", AnimationRequest::Play)
            };
            if ui.add(IconButton::new(icon, tip)).clicked() {
                ask(sink, request);
            }
            if ui
                .add(IconButton::new(Icon::Stop, "Stop and show the rest pose"))
                .clicked()
            {
                ask(sink, AnimationRequest::Stop);
            }
            let repeat = IconButton::new(Icon::Repeat, "Start again at the end")
                .selected(animations.looping);
            if ui.add(repeat).clicked() {
                ask(sink, AnimationRequest::SetLoop(!animations.looping));
            }
            let rate = current.fps.max(1) as f32;
            let mut time = animations.time.clamp(0.0, current.length);
            let field = DragFieldOpts {
                name: "Playhead",
                speed: 0.01,
                range: 0.0..=current.length.max(f32::EPSILON),
                unit: "s",
                decimals: 3,
                snap: Some(1.0 / rate),
                width: Some(96.0),
                ..DragFieldOpts::default()
            };
            if drag_field(ui, "timeline_playhead", &mut time, &field).changed {
                if animations.playing {
                    ask(sink, AnimationRequest::Pause);
                }
                ask(sink, AnimationRequest::Seek(time));
            }
            let frame = format_number((animations.time * rate).round(), 0);
            ui.label(text(
                Type::Caption,
                format!("frame {frame} at {} fps", current.fps),
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let fit = IconButton::new(Icon::FitWidth, "Fit the animation to the width");
                if ui.add(fit).clicked() {
                    self.fitted = None;
                }
            });
        });
    }

    /// The names of the bones and channels, with the "+" of each channel.
    fn left_column(&mut self, ui: &mut Ui, left: Rect, view: &TimelineView, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        let top = left.top() + ROW;
        ui.painter().rect_filled(left, 0.0, tokens.bg_panel);
        let clip = Rect::from_min_max(pos2(left.left(), top), left.max);
        let (first, last) = self.visible_rows(top, left.bottom());
        for at in first..last {
            let row = self.rows[at];
            let y = top + at as f32 * ROW - self.scroll;
            let rect = Rect::from_min_size(pos2(left.left(), y), vec2(left.width(), ROW));
            let bone = &view.bones[row.bone];
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("timeline_row", bone.id, row.channel))
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            child.set_clip_rect(rect.intersect(clip));
            match row.channel {
                None => self.bone_row(&mut child, bone, sink),
                Some(channel) => channel_row(&mut child, rect, bone, channel, sink),
            }
        }
        if self.rows.is_empty() {
            let note = if view.has_groups {
                "Select a bone to key it."
            } else {
                "Only groups are animated: put the object into a group"
            };
            let mut child = ui.new_child(UiBuilder::new().max_rect(clip.shrink(space::SM)));
            child.add(
                egui::Label::new(text(Type::Caption, note))
                    .wrap()
                    .selectable(false),
            );
        }
    }

    fn bone_row(&mut self, ui: &mut Ui, bone: &BoneRow, sink: &mut Sink) {
        let open = !self.collapsed.contains(&bone.id);
        let shown = tree_row(
            ui,
            ("timeline_bone", bone.id),
            TreeRow {
                depth: bone.depth,
                label: &bone.name,
                icon: Icon::Group,
                expanded: Some(open),
                selected: bone.selected,
                primary: false,
                dimmed: false,
                toggles: &[],
                drop: None,
                editing: None,
            },
        );
        if shown.arrow {
            if open {
                self.collapsed.insert(bone.id);
            } else {
                self.collapsed.remove(&bone.id);
            }
            self.collapsed_rev += 1;
        } else if shown.row.clicked() {
            let mods = ui.input(|i| i.modifiers);
            let op = if mods.command {
                SelectOp::Toggle
            } else if mods.shift {
                SelectOp::Add
            } else {
                SelectOp::Replace
            };
            sink.edit(Edit::Select {
                ids: vec![bone.id],
                op,
            });
        }
    }

    /// The first row that shows and the one after the last, for rows between `top` and `bottom`.
    fn visible_rows(&self, top: f32, bottom: f32) -> (usize, usize) {
        let height = (bottom - top).max(0.0);
        let first = row_index(self.scroll);
        let last = row_index(self.scroll + height) + 1;
        (first.min(self.rows.len()), last.min(self.rows.len()))
    }

    fn x_of(&self, area: &Rect, time: f32) -> f32 {
        area.left() + (time - self.origin) * self.zoom
    }

    fn time_of(&self, area: &Rect, x: f32) -> f32 {
        (x - area.left()) / self.zoom + self.origin
    }

    /// The channels whose keyframes a row shows: its own, or all three of a folded bone.
    fn row_channels<'a>(&self, row: Row, bone: &'a BoneRow) -> [&'a [KeyMark]; 3] {
        let empty: &[KeyMark] = &[];
        let shown = |channel: ChannelKind| &bone.channels[channel.index()][..];
        match row.channel {
            Some(channel) => ChannelKind::ALL.map(|c| if c == channel { shown(c) } else { empty }),
            None if self.collapsed.contains(&bone.id) => ChannelKind::ALL.map(shown),
            None => [empty; 3],
        }
    }

    /// The marks of a row between two times, with the ones closer than [`MERGE`] points joined.
    fn clusters(&self, area: &Rect, channels: [&[KeyMark]; 3], span: (f32, f32)) -> Vec<Cluster> {
        let gap = MERGE / self.zoom;
        let mut at = channels.map(|c| c.partition_point(|k| k.time < span.0));
        let ends = channels.map(|c| c.partition_point(|k| k.time <= span.1));
        let mut out = Vec::new();
        loop {
            let next = (0..3)
                .filter(|&c| at[c] < ends[c])
                .map(|c| channels[c][at[c]].time)
                .min_by(f32::total_cmp);
            let Some(time) = next else {
                break;
            };
            let mut ranges = [0..0, 0..0, 0..0];
            for c in 0..3 {
                let stop =
                    at[c] + channels[c][at[c]..ends[c]].partition_point(|k| k.time < time + gap);
                ranges[c] = at[c]..stop;
                at[c] = stop;
            }
            out.push(Cluster {
                x: self.x_of(area, time),
                ranges,
            });
        }
        out
    }

    /// The visible span of time, with room for the half of a mark at both ends.
    fn span(&self, area: &Rect) -> (f32, f32) {
        let room = MARK / self.zoom;
        (
            self.time_of(area, area.left()) - room,
            self.time_of(area, area.right()) + room,
        )
    }

    /// The keyframes of a cluster.
    fn cluster_keys(
        &self,
        bone: &BoneRow,
        channels: [&[KeyMark]; 3],
        cluster: &Cluster,
    ) -> Vec<KeyId> {
        let mut keys = Vec::new();
        for channel in ChannelKind::ALL {
            let at = channel.index();
            for key in &channels[at][cluster.ranges[at].clone()] {
                keys.push(KeyId {
                    bone: bone.id,
                    channel,
                    time: key.time,
                });
            }
        }
        keys
    }

    /// The row and the point of the pointer in the rows, or `None` over the ruler.
    fn row_at(&self, area: &Rect, pos: Pos2) -> Option<usize> {
        let top = area.top() + ROW;
        (pos.y >= top)
            .then(|| row_index(pos.y - top + self.scroll))
            .filter(|&at| at < self.rows.len())
    }

    /// The keyframes of the mark under `pos`.
    fn mark_at(&self, area: &Rect, view: &TimelineView, pos: Pos2) -> Option<Vec<KeyId>> {
        let row = self.rows.get(self.row_at(area, pos)?).copied()?;
        let bone = &view.bones[row.bone];
        let channels = self.row_channels(row, bone);
        let reach = REACH / self.zoom;
        let time = self.time_of(area, pos.x);
        let near = self.clusters(area, channels, (time - reach, time + reach));
        let best = near
            .iter()
            .min_by(|a, b| (a.x - pos.x).abs().total_cmp(&(b.x - pos.x).abs()))?;
        ((best.x - pos.x).abs() <= REACH).then(|| self.cluster_keys(bone, channels, best))
    }

    /// The time area: ruler, rows, marks, playhead and the pointer on them.
    fn time_area(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        animations: &AnimationsView,
        current: &AnimationRow,
        view: &TimelineView,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        let response = ui.interact(
            area,
            ui.make_persistent_id("timeline_area"),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Group, ui.is_enabled(), "Timeline")
        });
        let hover = response.hover_pos();
        self.navigate(ui, &response, area);

        let shown_drag = self.pointer(ui, &response, area, animations, current, view, sink);

        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, tokens.bg_input);
        let rows_top = area.top() + ROW;
        // Time outside the animation is dimmed.
        let (start, end) = (self.x_of(&area, 0.0), self.x_of(&area, current.length));
        for (from, to) in [(area.left(), start), (end, area.right())] {
            let outside = Rect::from_min_max(
                pos2(from.max(area.left()), rows_top),
                pos2(to.min(area.right()), area.bottom()),
            );
            if outside.width() > 0.0 {
                painter.rect_filled(outside, 0.0, tokens.bg_app);
            }
        }
        self.ruler(&painter, &area, current);

        let selected: HashSet<(NodeId, ChannelKind, u32)> =
            self.selected.iter().map(key_bits).collect();
        let shift = match shown_drag {
            Some(Drag::Marks { delta, .. }) => delta * self.zoom,
            _ => 0.0,
        };
        let rows_clip = Rect::from_min_max(pos2(area.left(), rows_top), area.max);
        let rows_painter = painter.with_clip_rect(rows_clip);
        let span = self.span(&area);
        let (first, last) = self.visible_rows(rows_top, area.bottom());
        for (at, row) in self.rows.iter().copied().enumerate().take(last).skip(first) {
            let y = rows_top + at as f32 * ROW - self.scroll;
            let band = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), ROW));
            let bone = &view.bones[row.bone];
            if row.channel.is_none() {
                let fill = if bone.selected {
                    tokens.accent_soft
                } else {
                    tokens.bg_hover
                };
                rows_painter.rect_filled(band, 0.0, fill);
            }
            rows_painter.hline(
                band.x_range(),
                band.bottom(),
                Stroke::new(1.0, tokens.border),
            );
            let channels = self.row_channels(row, bone);
            for cluster in self.clusters(&area, channels, span) {
                let Some((channel, mark)) = first_mark(&channels, &cluster) else {
                    continue;
                };
                let chosen = selected.contains(&(bone.id, channel, mark.time.to_bits()));
                let x = cluster.x + if chosen { shift } else { 0.0 };
                paint_mark(
                    &rows_painter,
                    &tokens,
                    pos2(x, band.center().y),
                    mark,
                    chosen,
                );
            }
        }

        // The playhead over everything.
        let head = self.x_of(&area, animations.time);
        if (area.left()..=area.right()).contains(&head) {
            let line = Stroke::new(1.5, tokens.accent);
            painter.vline(head, area.y_range(), line);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(head - MARK, area.top()),
                    pos2(head + MARK, area.top()),
                    pos2(head, area.top() + MARK * 1.6),
                ],
                tokens.accent,
                Stroke::NONE,
            ));
        }
        if let Some(Drag::Band { origin }) = shown_drag
            && let Some(now) = hover
        {
            let band = Rect::from_two_pos(origin, now).intersect(rows_clip);
            painter.rect_filled(band, 0.0, tokens.accent_soft);
            painter.rect_stroke(
                band,
                0.0,
                Stroke::new(1.0, tokens.accent),
                egui::StrokeKind::Inside,
            );
        }
        self.context_menu(&response, area, view, sink);
    }

    /// Wheel, zoom and middle-drag navigation of the time area.
    fn navigate(&mut self, ui: &Ui, response: &egui::Response, area: Rect) {
        if response.hovered() {
            let (wheel, zoom, mods) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
            if (zoom - 1.0).abs() > f32::EPSILON
                && let Some(pos) = response.hover_pos()
            {
                // Zoom about the pointer: the time under it stays under it.
                let at = self.time_of(&area, pos.x);
                self.zoom = (self.zoom * zoom).clamp(ZOOM.0, ZOOM.1);
                self.origin = at - (pos.x - area.left()) / self.zoom;
            }
            if mods.shift {
                self.origin -= (wheel.x + wheel.y) / self.zoom;
            } else {
                self.origin -= wheel.x / self.zoom;
                self.scroll -= wheel.y;
            }
        }
        if response.dragged_by(PointerButton::Middle) {
            let delta = ui.input(|i| i.pointer.delta());
            self.origin -= delta.x / self.zoom;
            self.scroll -= delta.y;
        }
        let content = self.rows.len() as f32 * ROW;
        self.scroll = self
            .scroll
            .clamp(0.0, (content - self.view_height).max(0.0));
    }

    /// Clicks and drags of the primary button. Returns the drag in progress, to draw.
    #[allow(clippy::too_many_arguments)]
    fn pointer(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        area: Rect,
        animations: &AnimationsView,
        current: &AnimationRow,
        view: &TimelineView,
        sink: &mut Sink,
    ) -> Option<Drag> {
        let mods = ui.input(|i| i.modifiers);
        let rate = current.fps.max(1) as f32;
        let snap = |time: f32| {
            if mods.alt {
                time
            } else {
                (time * rate).round() / rate
            }
        };
        let press = ui.input(|i| i.pointer.press_origin());
        let on_ruler = |pos: Pos2| pos.y < area.top() + ROW;

        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = press
        {
            self.sought = None;
            // A drag in the timeline pauses the player.
            if animations.playing {
                sink.push(UiRequest::Animation(AnimationRequest::Pause));
            }
            self.drag = Some(if on_ruler(origin) {
                // The playhead goes where the press was, before the pointer moved.
                let time = snap(self.time_of(&area, origin.x)).clamp(0.0, current.length);
                self.sought = Some(time);
                sink.push(UiRequest::Animation(AnimationRequest::Seek(time)));
                Drag::Scrub
            } else if let Some(keys) = self.mark_at(&area, view, origin) {
                if !keys.iter().all(|k| self.is_selected(k)) {
                    self.select(keys.clone(), mods);
                }
                let times = self.selected.iter().map(|k| k.time);
                let span = (
                    times.clone().fold(f32::INFINITY, f32::min),
                    times.fold(f32::NEG_INFINITY, f32::max),
                );
                Drag::Marks {
                    anchor: keys.first().map_or(0.0, |k| k.time),
                    span,
                    delta: 0.0,
                }
            } else {
                Drag::Band { origin }
            });
        }

        let pos = response.interact_pointer_pos();
        if response.dragged_by(PointerButton::Primary) {
            match (&mut self.drag, pos, press) {
                (Some(Drag::Scrub), Some(pos), _) => {
                    let time = snap(self.time_of(&area, pos.x)).clamp(0.0, current.length);
                    if self.sought != Some(time) {
                        self.sought = Some(time);
                        sink.push(UiRequest::Animation(AnimationRequest::Seek(time)));
                    }
                }
                (
                    Some(Drag::Marks {
                        anchor,
                        span,
                        delta,
                    }),
                    Some(pos),
                    Some(origin),
                ) => {
                    let wanted = *anchor + (pos.x - origin.x) / self.zoom;
                    *delta = (snap(wanted) - *anchor).clamp(-span.0, LONGEST - span.1);
                }
                _ => {}
            }
        }

        if response.drag_stopped_by(PointerButton::Primary)
            && let Some(drag) = self.drag.take()
        {
            match drag {
                Drag::Marks { delta, .. } if delta.abs() > f32::EPSILON => {
                    sink.push(UiRequest::Animate(AnimateEdit::Move {
                        keys: self.selected.clone(),
                        delta,
                        commit: true,
                    }));
                    self.follow_move(delta, view.rev);
                }
                Drag::Band { origin } => {
                    if let Some(now) = pos {
                        let keys = self.keys_in(&area, view, Rect::from_two_pos(origin, now));
                        self.select(keys, mods);
                    }
                }
                _ => {}
            }
            return None;
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(pos) = pos
        {
            if on_ruler(pos) {
                let time = snap(self.time_of(&area, pos.x)).clamp(0.0, current.length);
                if animations.playing {
                    sink.push(UiRequest::Animation(AnimationRequest::Pause));
                }
                sink.push(UiRequest::Animation(AnimationRequest::Seek(time)));
            } else {
                match self.mark_at(&area, view, pos) {
                    Some(keys) => self.select(keys, mods),
                    None if !mods.command && !mods.shift => self.selected.clear(),
                    None => {}
                }
            }
        }
        if response.double_clicked_by(PointerButton::Primary)
            && let Some(pos) = pos
            && let Some(key) = self
                .mark_at(&area, view, pos)
                .and_then(|keys| keys.first().copied())
        {
            sink.push(UiRequest::Animation(AnimationRequest::Seek(key.time)));
        }
        self.drag.clone()
    }

    /// Combines keyframes with the selection: Ctrl toggles them, Shift adds them, otherwise they
    /// become the selection.
    fn select(&mut self, keys: Vec<KeyId>, mods: egui::Modifiers) {
        if mods.command {
            for key in keys {
                match self
                    .selected
                    .iter()
                    .position(|s| key_bits(s) == key_bits(&key))
                {
                    Some(at) => {
                        self.selected.remove(at);
                    }
                    None => self.selected.push(key),
                }
            }
        } else if mods.shift {
            for key in keys {
                if !self.is_selected(&key) {
                    self.selected.push(key);
                }
            }
        } else {
            self.selected = keys;
        }
    }

    /// The keyframes whose marks lie in a rectangle of the time area.
    fn keys_in(&self, area: &Rect, view: &TimelineView, band: Rect) -> Vec<KeyId> {
        let mut keys = Vec::new();
        let top = area.top() + ROW;
        let span = (
            self.time_of(area, band.left()),
            self.time_of(area, band.right()),
        );
        let (first, last) = self.visible_rows(top, area.bottom());
        for (at, row) in self.rows.iter().copied().enumerate().take(last).skip(first) {
            let y = top + at as f32 * ROW - self.scroll;
            if y + ROW < band.top() || y > band.bottom() {
                continue;
            }
            let bone = &view.bones[row.bone];
            let channels = self.row_channels(row, bone);
            for cluster in self.clusters(area, channels, span) {
                keys.extend(self.cluster_keys(bone, channels, &cluster));
            }
        }
        keys
    }

    /// Ruler: ticks, labels in seconds or in frames, by the zoom.
    fn ruler(&self, painter: &egui::Painter, area: &Rect, current: &AnimationRow) {
        let tokens = Tokens::of(painter.ctx());
        let bar = Rect::from_min_size(area.min, vec2(area.width(), ROW));
        painter.rect_filled(bar, 0.0, tokens.bg_panel);
        painter.hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, tokens.border));
        let rate = current.fps.max(1) as f32;
        let frames = self.zoom / rate >= FRAME_MIN;
        // One unit is a second, or a frame once a frame is wide enough to count them.
        let unit = if frames { 1.0 / rate } else { 1.0 };
        let per_unit = self.zoom * unit;
        let major = nice_step(LABEL_GAP / per_unit);
        let minor = if frames { 1.0 } else { major / 5.0 };
        let first = whole((self.time_of(area, area.left()) / unit / minor).floor());
        let last = whole((self.time_of(area, area.right()) / unit / minor).ceil());
        for k in first..=last {
            let units = k as f32 * minor;
            let x = self.x_of(area, units * unit);
            let is_major = (units / major - (units / major).round()).abs() < 1e-3;
            let tick = if is_major { ROW * 0.5 } else { ROW * 0.25 };
            painter.vline(
                x,
                bar.bottom() - tick..=bar.bottom(),
                Stroke::new(1.0, tokens.border_strong),
            );
            if is_major {
                let label = if frames {
                    format_number(units, 0)
                } else {
                    format!("{} s", format_number(units, 3))
                };
                painter.text(
                    pos2(x + space::XS, bar.top() + space::XS),
                    Align2::LEFT_TOP,
                    label,
                    Type::Caption.font(),
                    Type::Caption.color(&tokens),
                );
            }
        }
    }

    /// The menu of the right button: interpolation, copy, paste, delete and go to keyframe.
    fn context_menu(
        &mut self,
        response: &egui::Response,
        area: Rect,
        view: &TimelineView,
        sink: &mut Sink,
    ) {
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu = self.mark_at(&area, view, pos);
            if let Some(keys) = &self.menu
                && !keys.iter().all(|k| self.is_selected(k))
            {
                self.selected = keys.clone();
            }
        }
        let target = self.menu.clone();
        response.context_menu(|ui| {
            let primary = view.bones.iter().find(|b| b.selected).map(|b| b.id);
            let paste = |sink: &mut Sink| {
                sink.push(UiRequest::Animate(AnimateEdit::Paste { bone: primary }));
            };
            let Some(keys) = &target else {
                if ui.add(MenuItem::new("Paste")).clicked() {
                    paste(sink);
                    ui.close();
                }
                return;
            };
            submenu(ui, "Interpolation", |ui| {
                let current = shared_kind(view, keys);
                for kind in KeyKind::ALL {
                    let item = MenuItem::new(kind.name()).checked(current == Some(kind));
                    if ui.add(item).clicked() {
                        sink.push(UiRequest::Animate(AnimateEdit::Interp {
                            keys: self.selected.clone(),
                            kind,
                        }));
                        ui.close();
                    }
                }
            });
            if ui.add(MenuItem::new("Copy")).clicked() {
                sink.push(UiRequest::Animate(AnimateEdit::Copy {
                    keys: self.selected.clone(),
                }));
                ui.close();
            }
            if ui.add(MenuItem::new("Paste")).clicked() {
                paste(sink);
                ui.close();
            }
            if ui.add(MenuItem::new("Delete")).clicked() {
                sink.push(UiRequest::Animate(AnimateEdit::Delete {
                    keys: std::mem::take(&mut self.selected),
                }));
                ui.close();
            }
            if ui.add(MenuItem::new("Go to keyframe")).clicked() {
                if let Some(key) = keys.first() {
                    sink.push(UiRequest::Animation(AnimationRequest::Seek(key.time)));
                }
                ui.close();
            }
        });
    }
}

/// Two keyframes closer in time than this are the same one, as in the model.
const TIME_EPSILON: f32 = 1e-4;

/// The keyframe a name stands for, if the model still has it.
pub(crate) fn find_key<'a>(view: &'a TimelineView, key: &KeyId) -> Option<&'a KeyMark> {
    let bone = view.bones.iter().find(|b| b.id == key.bone)?;
    let list = &bone.channels[key.channel.index()];
    let at = list.partition_point(|k| k.time < key.time - TIME_EPSILON);
    list.get(at)
        .filter(|found| (found.time - key.time).abs() < TIME_EPSILON)
}

/// The interpolation the keyframes share, `None` when they differ or one is gone.
pub(crate) fn shared_kind(view: &TimelineView, keys: &[KeyId]) -> Option<KeyKind> {
    let first = find_key(view, keys.first()?)?.kind;
    keys.iter()
        .all(|k| find_key(view, k).is_some_and(|m| m.kind == first))
        .then_some(first)
}

/// The first keyframe of a cluster, with its channel: the one that gives the mark its look.
fn first_mark<'a>(
    channels: &[&'a [KeyMark]; 3],
    cluster: &Cluster,
) -> Option<(ChannelKind, &'a KeyMark)> {
    ChannelKind::ALL
        .into_iter()
        .filter_map(|channel| {
            let at = channel.index();
            let mark = channels[at].get(cluster.ranges[at].start)?;
            (!cluster.ranges[at].is_empty()).then_some((channel, mark))
        })
        .min_by(|a, b| a.1.time.total_cmp(&b.1.time))
}

/// A channel row: its name and the "+" that keys it at the playhead.
fn channel_row(ui: &mut Ui, rect: Rect, bone: &BoneRow, channel: ChannelKind, sink: &mut Sink) {
    ui.add_space(space::XS + (bone.depth + 2) as f32 * space::MD);
    ui.label(text(Type::Label, channel.name()));
    let side = height::CONTROL;
    let slot = Rect::from_min_size(
        pos2(
            rect.right() - space::XS - side,
            rect.top() + (ROW - side) / 2.0,
        ),
        vec2(side, side),
    );
    let tip = format!("Key {} at the playhead", channel.name().to_lowercase());
    if ui.put(slot, IconButton::new(Icon::Plus, &tip)).clicked() {
        sink.push(UiRequest::Animate(AnimateEdit::KeyHere {
            bone: bone.id,
            channel,
        }));
    }
}

/// The fill of a mark: the accent while it is selected, the panel (a hollow mark with an edge)
/// while it is not, which keeps the two 3:1 apart in every theme.
fn mark_fill(tokens: &Tokens, selected: bool) -> Color32 {
    if selected {
        tokens.accent
    } else {
        tokens.bg_panel
    }
}

/// Draws a keyframe: a diamond for linear, a circle for Catmull-Rom, a square for step and a
/// diamond with a bar for Bezier. An expression has a warning outline and a jump splits the mark.
fn paint_mark(painter: &egui::Painter, tokens: &Tokens, at: Pos2, mark: &KeyMark, selected: bool) {
    let fill = mark_fill(tokens, selected);
    let edge = if selected {
        tokens.accent
    } else {
        tokens.text_weak
    };
    let outline = if mark.expression() {
        Stroke::new(2.0, tokens.warn)
    } else {
        Stroke::new(1.5, edge)
    };
    let r = MARK;
    match mark.kind {
        KeyKind::Linear | KeyKind::Bezier => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(at.x, at.y - r),
                    pos2(at.x + r, at.y),
                    pos2(at.x, at.y + r),
                    pos2(at.x - r, at.y),
                ],
                fill,
                outline,
            ));
            if mark.kind == KeyKind::Bezier {
                painter.hline(
                    at.x - r - 3.0..=at.x + r + 3.0,
                    at.y,
                    Stroke::new(2.0, edge),
                );
            }
        }
        KeyKind::CatmullRom => {
            painter.circle(at, r, fill, outline);
        }
        KeyKind::Step => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(at.x - r, at.y - r),
                    pos2(at.x + r, at.y - r),
                    pos2(at.x + r, at.y + r),
                    pos2(at.x - r, at.y + r),
                ],
                fill,
                outline,
            ));
        }
    }
    if mark.jump {
        painter.vline(at.x, at.y - r..=at.y + r, Stroke::new(1.5, tokens.bg_input));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, Modifiers, Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::model::KeyNumber;

    #[test]
    fn labels_step_by_one_two_and_five() {
        assert!((nice_step(0.3) - 0.5).abs() < 1e-6);
        assert!((nice_step(1.0) - 1.0).abs() < 1e-6);
        assert!((nice_step(1.1) - 2.0).abs() < 1e-6);
        assert!((nice_step(6.4) - 10.0).abs() < 1e-6);
    }

    /// The timeline in a window of its own, with a fixture of bones and keyframes.
    struct Rig {
        state: TimelineState,
        animations: AnimationsView,
        view: TimelineView,
        sink: Sink,
        /// How many requests of the sink the app has seen.
        applied: usize,
    }

    fn mark(time: f32, kind: KeyKind) -> KeyMark {
        KeyMark {
            time,
            kind,
            value: [
                KeyNumber::Num(time),
                KeyNumber::Num(0.0),
                KeyNumber::Num(0.0),
            ],
            jump: false,
        }
    }

    fn keys(marks: &[(f32, KeyKind)]) -> Arc<[KeyMark]> {
        marks.iter().map(|&(t, k)| mark(t, k)).collect()
    }

    fn bone(id: u64, name: &str, depth: usize, channels: [Arc<[KeyMark]>; 3]) -> BoneRow {
        BoneRow {
            id: NodeId(id),
            name: Arc::from(name),
            depth,
            selected: false,
            channels,
        }
    }

    impl Rig {
        /// An animation of two seconds at 24 frames per second; `root` has rotation keyframes at
        /// 0.5 and 1.0 and position keyframes at 0.25, `arm` below it has a scale keyframe.
        fn new() -> Rig {
            let none = || keys(&[]);
            let root = bone(
                1,
                "root",
                0,
                [
                    keys(&[(0.5, KeyKind::Linear), (1.0, KeyKind::Linear)]),
                    keys(&[(0.25, KeyKind::Linear)]),
                    none(),
                ],
            );
            let arm = bone(2, "arm", 1, [none(), none(), keys(&[(1.5, KeyKind::Step)])]);
            Rig {
                state: TimelineState::default(),
                animations: AnimationsView {
                    rows: Arc::from(vec![AnimationRow {
                        id: 11,
                        name: Arc::from("walk"),
                        length: 2.0,
                        loop_kind: crate::model::LoopKind::Once,
                        fps: 24,
                    }]),
                    selected: Some(0),
                    previewing: true,
                    time: 0.75,
                    speed: 1.0,
                    ..AnimationsView::default()
                },
                view: TimelineView {
                    rev: 1,
                    bones: Arc::from(vec![root, arm]),
                    has_groups: true,
                    pose: None,
                },
                sink: Sink::default(),
                applied: 0,
            }
        }
    }

    impl Rig {
        /// Plays the app: a move of keyframes that the timeline asked for shifts them in the view
        /// before the next frame, as the core does.
        fn follow(&mut self) {
            while self.applied < self.sink.requests.len() {
                let request = &self.sink.requests[self.applied];
                self.applied += 1;
                let UiRequest::Animate(AnimateEdit::Move {
                    keys,
                    delta,
                    commit: true,
                }) = request
                else {
                    continue;
                };
                let mut bones = self.view.bones.to_vec();
                for key in keys {
                    let Some(bone) = bones.iter_mut().find(|b| b.id == key.bone) else {
                        continue;
                    };
                    let mut list = bone.channels[key.channel.index()].to_vec();
                    for found in &mut list {
                        if (found.time - key.time).abs() < TIME_EPSILON {
                            found.time += delta;
                        }
                    }
                    bone.channels[key.channel.index()] = Arc::from(list);
                }
                self.view.bones = Arc::from(bones);
                self.view.rev += 1;
            }
        }
    }

    fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
        rig.follow();
        rig.state
            .show(ui, Some(&rig.animations), Some(&rig.view), &mut rig.sink);
    }

    fn harness(rig: Rig) -> Harness<'static, Rig> {
        // A frame takes 1/60 s, so two clicks of the test are a double click.
        let mut harness = Harness::builder()
            .with_size(vec2(1000.0, 420.0))
            .with_step_dt(1.0 / 60.0)
            .allow_missing_glyphs()
            .build_ui_state(frame, rig);
        harness.run_steps(4);
        harness
    }

    /// Where a keyframe of a row is on the screen: `row` counts the labels `channel` has.
    fn mark_at(
        harness: &Harness<'static, Rig>,
        channel: ChannelKind,
        row: usize,
        time: f32,
    ) -> Pos2 {
        let y = harness
            .get_all_by_label(channel.name())
            .nth(row)
            .expect("the row")
            .rect()
            .center()
            .y;
        x_at(harness, time, y)
    }

    fn x_at(harness: &Harness<'static, Rig>, time: f32, y: f32) -> Pos2 {
        let left = harness
            .get_by_role_and_label(Role::TreeItem, "root")
            .rect()
            .left();
        let state = &harness.state().state;
        pos2(left + LEFT_WIDTH + (time - state.origin) * state.zoom, y)
    }

    fn ruler_at(harness: &Harness<'static, Rig>, time: f32) -> Pos2 {
        let root = harness.get_by_role_and_label(Role::TreeItem, "root").rect();
        x_at(harness, time, root.top() - ROW / 2.0)
    }

    fn press(
        harness: &mut Harness<'static, Rig>,
        at: Pos2,
        button: PointerButton,
        mods: Modifiers,
    ) {
        harness.event(Event::ModifiersChanged(mods));
        harness.event(Event::PointerMoved(at));
        harness.run_steps(2);
        harness.event(Event::PointerButton {
            pos: at,
            button,
            pressed: true,
            modifiers: mods,
        });
        harness.run_steps(2);
    }

    fn release(
        harness: &mut Harness<'static, Rig>,
        at: Pos2,
        button: PointerButton,
        mods: Modifiers,
    ) {
        harness.event(Event::PointerButton {
            pos: at,
            button,
            pressed: false,
            modifiers: mods,
        });
        harness.run_steps(3);
        harness.event(Event::ModifiersChanged(Modifiers::NONE));
        harness.run_steps(1);
    }

    fn click(harness: &mut Harness<'static, Rig>, at: Pos2, mods: Modifiers) {
        press(harness, at, PointerButton::Primary, mods);
        release(harness, at, PointerButton::Primary, mods);
    }

    /// Presses at `from`, moves to `to` in steps and lets go.
    fn drag(harness: &mut Harness<'static, Rig>, from: Pos2, to: Pos2, mods: Modifiers) {
        press(harness, from, PointerButton::Primary, mods);
        for step in 1..=4 {
            let at = from + (to - from) * (step as f32 / 4.0);
            harness.event(Event::PointerMoved(at));
            harness.run_steps(2);
        }
        release(harness, to, PointerButton::Primary, mods);
    }

    /// A turn of the wheel by `delta` points.
    fn wheel(harness: &mut Harness<'static, Rig>, delta: f32, modifiers: Modifiers) {
        harness.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        });
        harness.run_steps(8);
    }

    fn take(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
        harness.run_steps(2);
        let rig = harness.state_mut();
        rig.applied = 0;
        std::mem::take(&mut rig.sink.requests)
    }

    fn selected_times(harness: &Harness<'static, Rig>) -> Vec<(u64, ChannelKind, f32)> {
        let mut keys: Vec<_> = harness
            .state()
            .state
            .selected()
            .iter()
            .map(|k| (k.bone.0, k.channel, k.time))
            .collect();
        keys.sort_by(|a, b| a.2.total_cmp(&b.2));
        keys
    }

    #[test]
    fn rows_are_built_for_the_visible_bones_only() {
        let mut rig = Rig::new();
        let many: Vec<BoneRow> = (0..400)
            .map(|i| {
                bone(
                    i + 10,
                    &format!("bone{i}"),
                    0,
                    [keys(&[(0.5, KeyKind::Linear)]), keys(&[]), keys(&[])],
                )
            })
            .collect();
        rig.view.bones = Arc::from(many);
        let harness = harness(rig);
        let built = harness.get_all_by_role(Role::TreeItem).count();
        assert!(built > 2 && built < 20, "{built} bone rows were built");
        // The state knows all the rows, and a scroll brings others into view.
        assert_eq!(harness.state().state.rows.len(), 400 * 4);
    }

    #[test]
    fn a_row_draws_at_most_its_width_over_two_marks_however_many_keyframes_it_has() {
        let marks: Vec<KeyMark> = (0..100_000)
            .map(|i| mark(i as f32 * 0.001, KeyKind::Linear))
            .collect();
        let none: &[KeyMark] = &[];
        let state = TimelineState {
            zoom: 10.0,
            origin: 0.0,
            ..TimelineState::default()
        };
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 200.0));
        let clusters = state.clusters(&area, [&marks, none, none], state.span(&area));
        assert!(
            clusters.len() as f32 <= area.width() / MERGE + 2.0,
            "{}",
            clusters.len()
        );
        let covered: usize = clusters.iter().map(|c| c.ranges[0].len()).sum();
        assert!(
            covered >= 99_000,
            "every visible keyframe is in a mark: {covered}"
        );
        // Zoomed in, every keyframe has its own mark.
        let state = TimelineState {
            zoom: 2000.0,
            ..state
        };
        let spaced: Vec<KeyMark> = (0..40)
            .map(|i| mark(i as f32 * 0.01, KeyKind::Linear))
            .collect();
        let few = state.clusters(&area, [&spaced, none, none], state.span(&area));
        assert_eq!(few.len(), 40);
    }

    #[test]
    fn a_click_selects_a_mark_ctrl_toggles_shift_adds_and_empty_space_clears() {
        let mut harness = harness(Rig::new());
        let first = mark_at(&harness, ChannelKind::Rotation, 0, 0.5);
        let second = mark_at(&harness, ChannelKind::Rotation, 0, 1.0);
        click(&mut harness, first, Modifiers::NONE);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Rotation, 0.5)]);
        click(&mut harness, second, Modifiers::SHIFT);
        assert_eq!(selected_times(&harness).len(), 2);
        click(&mut harness, first, Modifiers::COMMAND);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Rotation, 1.0)]);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, second, Modifiers::NONE);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Rotation, 1.0)]);
        // The position row is another row: its mark is not the one under the rotation mark.
        let position = mark_at(&harness, ChannelKind::Position, 0, 0.25);
        click(&mut harness, position, Modifiers::NONE);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Position, 0.25)]);
        let empty = mark_at(&harness, ChannelKind::Scale, 0, 1.2);
        click(&mut harness, empty, Modifiers::SHIFT);
        assert_eq!(
            selected_times(&harness).len(),
            1,
            "shift on nothing keeps it"
        );
        click(&mut harness, empty, Modifiers::NONE);
        assert!(selected_times(&harness).is_empty());
        assert_eq!(take(&mut harness), [], "selecting asks for nothing");
    }

    #[test]
    fn a_box_selects_the_marks_across_rows_and_time() {
        let mut harness = harness(Rig::new());
        let from = mark_at(&harness, ChannelKind::Rotation, 0, 0.1) - vec2(0.0, 8.0);
        let to = mark_at(&harness, ChannelKind::Position, 0, 0.8) + vec2(0.0, 8.0);
        drag(&mut harness, from, to, Modifiers::NONE);
        // The rotation mark at 0.5 and the position mark at 0.25; the one at 1.0 is outside.
        assert_eq!(
            selected_times(&harness),
            [
                (1, ChannelKind::Position, 0.25),
                (1, ChannelKind::Rotation, 0.5)
            ]
        );
        assert_eq!(take(&mut harness), [], "a box asks for nothing");
    }

    #[test]
    fn dragging_selected_marks_sends_one_move_on_release_with_the_snapped_delta() {
        let mut harness = harness(Rig::new());
        let (a, b) = (
            mark_at(&harness, ChannelKind::Rotation, 0, 0.5),
            mark_at(&harness, ChannelKind::Rotation, 0, 1.0),
        );
        click(&mut harness, a, Modifiers::NONE);
        click(&mut harness, b, Modifiers::SHIFT);
        take(&mut harness);
        // Grab the mark at 1.0 and drag it to a little after 1.25: the frame grid is 1/24 s.
        let to = mark_at(&harness, ChannelKind::Rotation, 0, 1.26);
        press(&mut harness, b, PointerButton::Primary, Modifiers::NONE);
        for step in 1..=4 {
            harness.event(Event::PointerMoved(b + (to - b) * (step as f32 / 4.0)));
            harness.run_steps(2);
        }
        assert_eq!(
            take(&mut harness),
            [],
            "nothing is sent while the marks follow the pointer"
        );
        release(&mut harness, to, PointerButton::Primary, Modifiers::NONE);
        let requests = take(&mut harness);
        let [
            UiRequest::Animate(AnimateEdit::Move {
                keys,
                delta,
                commit,
            }),
        ] = requests.as_slice()
        else {
            panic!("one move: {requests:?}");
        };
        assert!(*commit);
        assert_eq!(keys.len(), 2);
        // 1.26 s is between frame 30 (1.25 s) and 31: the grabbed mark lands on a frame.
        let landed = 1.0 + delta;
        assert!(
            ((landed * 24.0).round() - landed * 24.0).abs() < 1e-3,
            "{delta}"
        );
        assert!((delta - 0.25).abs() < 0.05, "{delta}");
        // The app moved the keyframes: the selection moved with them and stays.
        let times: Vec<f32> = selected_times(&harness).iter().map(|k| k.2).collect();
        assert_eq!(times.len(), 2);
        assert!((times[0] - (0.5 + delta)).abs() < 1e-6 && (times[1] - (1.0 + delta)).abs() < 1e-6);
    }

    #[test]
    fn alt_turns_the_snapping_of_a_drag_off() {
        let mut harness = harness(Rig::new());
        let a = mark_at(&harness, ChannelKind::Rotation, 0, 0.5);
        let to = mark_at(&harness, ChannelKind::Rotation, 0, 0.6 + 0.01);
        drag(&mut harness, a, to, Modifiers::ALT);
        let requests = take(&mut harness);
        let [UiRequest::Animate(AnimateEdit::Move { delta, .. })] = requests.as_slice() else {
            panic!("one move: {requests:?}");
        };
        assert!(
            ((0.5 + delta) * 24.0 - ((0.5 + delta) * 24.0).round()).abs() > 1e-3,
            "{delta}"
        );
    }

    #[test]
    fn a_drag_cannot_take_a_mark_before_the_start() {
        let mut harness = harness(Rig::new());
        let a = mark_at(&harness, ChannelKind::Position, 0, 0.25);
        let to = a - vec2(300.0, 0.0);
        drag(&mut harness, a, to, Modifiers::NONE);
        let requests = take(&mut harness);
        let [UiRequest::Animate(AnimateEdit::Move { delta, .. })] = requests.as_slice() else {
            panic!("one move: {requests:?}");
        };
        assert!(
            (0.25 + delta).abs() < 1e-6,
            "the mark stops at time 0: {delta}"
        );
    }

    #[test]
    fn the_ruler_scrubs_the_playhead_on_the_frame_grid() {
        let mut harness = harness(Rig::new());
        let start = ruler_at(&harness, 0.51);
        press(&mut harness, start, PointerButton::Primary, Modifiers::NONE);
        let to = ruler_at(&harness, 1.0);
        for step in 1..=3 {
            harness.event(Event::PointerMoved(
                start + (to - start) * (step as f32 / 3.0),
            ));
            harness.run_steps(2);
        }
        release(&mut harness, to, PointerButton::Primary, Modifiers::NONE);
        let times: Vec<f32> = take(&mut harness)
            .into_iter()
            .filter_map(|r| match r {
                UiRequest::Animation(AnimationRequest::Seek(t)) => Some(t),
                _ => None,
            })
            .collect();
        assert!(times.len() >= 2, "{times:?}");
        assert!(
            times
                .iter()
                .all(|t| ((t * 24.0).round() - t * 24.0).abs() < 1e-3)
        );
        assert!((times[0] - 0.5).abs() < 0.05, "{times:?}");
        assert!((times[times.len() - 1] - 1.0).abs() < 0.05, "{times:?}");
        assert!(
            times.windows(2).all(|w| (w[0] - w[1]).abs() > 1e-6),
            "a still pointer asks for nothing"
        );
        // A click on the ruler is one seek, and a playing animation is paused first.
        harness.state_mut().animations.playing = true;
        harness.run_steps(2);
        let end = ruler_at(&harness, 1.5);
        click(&mut harness, end, Modifiers::NONE);
        let requests = take(&mut harness);
        assert!(matches!(
            requests[0],
            UiRequest::Animation(AnimationRequest::Pause)
        ));
        assert!(
            matches!(requests.last(), Some(UiRequest::Animation(AnimationRequest::Seek(t))) if (*t - 1.5).abs() < 0.05)
        );
    }

    #[test]
    fn a_double_click_puts_the_playhead_on_the_keyframe() {
        let mut harness = harness(Rig::new());
        let at = mark_at(&harness, ChannelKind::Rotation, 0, 1.0);
        click(&mut harness, at, Modifiers::NONE);
        click(&mut harness, at, Modifiers::NONE);
        let seeks: Vec<f32> = take(&mut harness)
            .into_iter()
            .filter_map(|r| match r {
                UiRequest::Animation(AnimationRequest::Seek(t)) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(seeks, [1.0]);
    }

    #[test]
    fn the_context_menu_sets_the_interpolation_of_the_keyframes() {
        let mut harness = harness(Rig::new());
        let at = mark_at(&harness, ChannelKind::Rotation, 0, 0.5);
        press(&mut harness, at, PointerButton::Secondary, Modifiers::NONE);
        release(&mut harness, at, PointerButton::Secondary, Modifiers::NONE);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Rotation, 0.5)]);
        harness.get_by_label("Interpolation").click();
        harness.run_steps(3);
        harness.get_by_label("Step").click();
        harness.run_steps(3);
        let requests = take(&mut harness);
        let [UiRequest::Animate(AnimateEdit::Interp { keys, kind })] = requests.as_slice() else {
            panic!("one request: {requests:?}");
        };
        assert_eq!(*kind, KeyKind::Step);
        assert_eq!(keys.len(), 1);
        assert_eq!(
            (keys[0].bone, keys[0].channel),
            (NodeId(1), ChannelKind::Rotation)
        );
    }

    #[test]
    fn the_context_menu_copies_deletes_and_goes_to_a_keyframe() {
        let mut harness = harness(Rig::new());
        let at = mark_at(&harness, ChannelKind::Rotation, 0, 1.0);
        for (label, expect) in [("Copy", 0), ("Go to keyframe", 1), ("Delete", 2)] {
            press(&mut harness, at, PointerButton::Secondary, Modifiers::NONE);
            release(&mut harness, at, PointerButton::Secondary, Modifiers::NONE);
            harness.get_by_label(label).click();
            harness.run_steps(3);
            let requests = take(&mut harness);
            let sent = requests.last().expect("a request");
            match (expect, sent) {
                (0, UiRequest::Animate(AnimateEdit::Copy { keys })) => assert_eq!(keys.len(), 1),
                (1, UiRequest::Animation(AnimationRequest::Seek(t))) => {
                    assert!((t - 1.0).abs() < 1e-6)
                }
                (2, UiRequest::Animate(AnimateEdit::Delete { keys })) => assert_eq!(keys.len(), 1),
                other => panic!("{label}: {other:?}"),
            }
        }
        // Pasting works from empty space too.
        let empty = mark_at(&harness, ChannelKind::Scale, 0, 0.3);
        press(
            &mut harness,
            empty,
            PointerButton::Secondary,
            Modifiers::NONE,
        );
        release(
            &mut harness,
            empty,
            PointerButton::Secondary,
            Modifiers::NONE,
        );
        harness.get_by_label("Paste").click();
        harness.run_steps(3);
        assert!(matches!(
            take(&mut harness).last(),
            Some(UiRequest::Animate(AnimateEdit::Paste { .. }))
        ));
    }

    #[test]
    fn the_plus_of_a_channel_keys_it_at_the_playhead_and_a_bone_row_folds() {
        let mut harness = harness(Rig::new());
        harness
            .get_all_by_label("Key position at the playhead")
            .next()
            .expect("a button")
            .click();
        assert_eq!(
            take(&mut harness),
            [UiRequest::Animate(AnimateEdit::KeyHere {
                bone: NodeId(1),
                channel: ChannelKind::Position
            })]
        );
        let rows = harness.state().state.rows.len();
        harness
            .get_by_role_and_label(Role::TreeItem, "root")
            .click();
        assert_eq!(
            take(&mut harness),
            [UiRequest::Edit(Edit::Select {
                ids: vec![NodeId(1)],
                op: SelectOp::Replace
            })]
        );
        assert_eq!(harness.state().state.rows.len(), rows);
        // The arrow folds the bone into one row that carries the keyframes of its channels.
        let state = &mut harness.state_mut().state;
        state.collapsed.insert(NodeId(1));
        state.collapsed_rev += 1;
        harness.run_steps(3);
        assert_eq!(harness.state().state.rows.len(), rows - 3);
        let summary = harness
            .get_by_role_and_label(Role::TreeItem, "root")
            .rect()
            .center();
        let hit = x_at(&harness, 0.25, summary.y);
        click(&mut harness, hit, Modifiers::NONE);
        assert_eq!(selected_times(&harness), [(1, ChannelKind::Position, 0.25)]);
    }

    #[test]
    fn keyframes_that_are_gone_leave_the_selection_and_a_new_animation_is_fitted() {
        let mut harness = harness(Rig::new());
        let at = mark_at(&harness, ChannelKind::Rotation, 0, 1.0);
        click(&mut harness, at, Modifiers::NONE);
        assert_eq!(selected_times(&harness).len(), 1);
        let rig = harness.state_mut();
        let fresh = keys(&[(0.5, KeyKind::Linear)]);
        let mut bones = rig.view.bones.to_vec();
        bones[0].channels[0] = fresh;
        rig.view.bones = Arc::from(bones);
        rig.view.rev += 1;
        harness.run_steps(3);
        assert!(selected_times(&harness).is_empty());
        // The whole animation fits the area: the end of the length is inside it.
        let state = &harness.state().state;
        let right = x_at(&harness, 2.0, 0.0).x;
        let area_right = harness
            .get_by_role_and_label(Role::TreeItem, "root")
            .rect()
            .left()
            + 1000.0;
        assert!(right <= area_right && state.zoom > ZOOM.0);
    }

    #[test]
    fn a_wheel_scrolls_the_rows_and_ctrl_zooms_about_the_pointer() {
        let mut rig = Rig::new();
        rig.view.bones = Arc::from(
            (0..60)
                .map(|i| {
                    bone(
                        i + 10,
                        &format!("b{i}"),
                        0,
                        [keys(&[(0.5, KeyKind::Linear)]), keys(&[]), keys(&[])],
                    )
                })
                .collect::<Vec<_>>(),
        );
        let mut harness = harness(rig);
        let over = harness
            .get_by_role_and_label(Role::TreeItem, "b0")
            .rect()
            .center()
            + vec2(LEFT_WIDTH + 100.0, 0.0);
        harness.event(Event::PointerMoved(over));
        harness.run_steps(2);
        wheel(&mut harness, -90.0, Modifiers::NONE);
        assert!(
            harness.state().state.scroll > 10.0,
            "{}",
            harness.state().state.scroll
        );
        let (zoom, origin) = (harness.state().state.zoom, harness.state().state.origin);
        wheel(&mut harness, 60.0, Modifiers::COMMAND);
        let after = harness.state().state.zoom;
        assert!(after > zoom, "{zoom} -> {after}");
        assert!((ZOOM.0..=ZOOM.1).contains(&after));
        assert!((harness.state().state.origin - origin).abs() > 1e-6);
        wheel(&mut harness, -60.0, Modifiers::SHIFT);
        assert!(harness.state().state.origin > origin - 10.0);
    }

    #[test]
    fn a_model_without_groups_and_without_an_animation_say_what_to_do() {
        let mut rig = Rig::new();
        rig.view = TimelineView {
            has_groups: false,
            ..TimelineView::default()
        };
        let harness_a = harness(rig);
        harness_a.get_by_label("Only groups are animated: put the object into a group");
        let mut rig = Rig::new();
        rig.animations.rows = Arc::from(Vec::new());
        rig.animations.selected = None;
        let mut harness_b = harness(rig);
        harness_b.get_by_label("No animation");
        harness_b.get_by_label("New animation").click();
        assert_eq!(
            take(&mut harness_b),
            [UiRequest::Animate(AnimateEdit::NewAnimation)]
        );
    }

    #[test]
    fn a_selected_mark_stands_out_from_an_unselected_one_in_every_theme() {
        use cuboid_ui_kit::{DarkFlavor, Theme, ThemeFamily, contrast_ratio};

        for family in ThemeFamily::ALL {
            for flavor in [DarkFlavor::Frappe, DarkFlavor::Macchiato, DarkFlavor::Mocha] {
                for (dark, high_contrast) in
                    [(true, false), (false, false), (true, true), (false, true)]
                {
                    let theme = Theme {
                        family,
                        flavor,
                        high_contrast,
                        accent: family.default_accent(),
                        ..Theme::default()
                    };
                    let tokens = theme.tokens(dark);
                    let label = format!("{family:?} {flavor:?} dark={dark} high={high_contrast}");
                    let (selected, unselected) =
                        (mark_fill(&tokens, true), mark_fill(&tokens, false));
                    assert!(contrast_ratio(selected, unselected) >= 3.0, "{label}");
                    // Both show against the row behind them: the hollow one by its edge.
                    assert!(contrast_ratio(selected, tokens.bg_input) >= 3.0, "{label}");
                    assert!(
                        contrast_ratio(tokens.text_weak, tokens.bg_input) >= 3.0,
                        "{label}"
                    );
                }
            }
        }
    }

    #[test]
    fn keyframes_of_an_animation_that_went_are_no_selection() {
        let mut rig = Rig::new();
        rig.state.select_keys(vec![KeyId {
            bone: NodeId(1),
            channel: ChannelKind::Rotation,
            time: 0.5,
        }]);
        // The animation was deleted or its creation undone: Delete, Copy and Paste must mean the
        // objects again.
        rig.animations.rows = Arc::from(Vec::new());
        rig.animations.selected = None;
        let harness = harness(rig);
        harness.get_by_label("No animation");
        assert!(harness.state().state.selected().is_empty());
    }
}
