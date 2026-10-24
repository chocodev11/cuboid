//! Vector icons drawn with the painter on a 24 x 24 grid. No font and no picture files.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

/// Every icon of the UI.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Icon {
    Plus,
    Minus,
    Close,
    Check,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    Menu,
    More,
    Search,
    Settings,
    Home,
    File,
    Folder,
    Save,
    Import,
    Export,
    Undo,
    Redo,
    Copy,
    Trash,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Cube,
    Plane,
    Group,
    Mesh,
    Voxel,
    Select,
    BoxSelect,
    Move,
    Rotate,
    Scale,
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rect,
    Mirror,
    Magnet,
    Grid,
    Palette,
    Image,
    Monitor,
    History,
    Frame,
    Warning,
    Info,
    Error,
    PanelLeft,
    PanelRight,
    PanelBottom,
    Keyboard,
}

/// Coordinates in the 24-unit grid mapped to the icon rectangle.
struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    stroke: Stroke,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + egui::vec2(x / 24.0 * self.rect.width(), y / 24.0 * self.rect.height())
    }

    fn path(&self, points: &[(f32, f32)]) -> Vec<Pos2> {
        points.iter().map(|&(x, y)| self.at(x, y)).collect()
    }

    fn line(&self, points: &[(f32, f32)]) {
        self.painter
            .add(Shape::line(self.path(points), self.stroke));
    }

    fn closed(&self, points: &[(f32, f32)]) {
        self.painter
            .add(Shape::closed_line(self.path(points), self.stroke));
    }

    fn filled(&self, points: &[(f32, f32)]) {
        let fill = self.stroke.color.gamma_multiply(0.35);
        self.painter.add(Shape::convex_polygon(
            self.path(points),
            fill,
            Stroke::new(self.stroke.width, self.stroke.color),
        ));
    }

    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_stroke(self.at(x, y), r / 24.0 * self.rect.width(), self.stroke);
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(
            self.at(x, y),
            r / 24.0 * self.rect.width(),
            self.stroke.color,
        );
    }

    fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let rect = Rect::from_two_pos(self.at(x0, y0), self.at(x1, y1));
        self.painter
            .rect_stroke(rect, 2.0, self.stroke, StrokeKind::Middle);
    }

    /// Points of an arc around `(cx, cy)`, angles in degrees, 0 pointing right, growing clockwise
    /// on screen.
    fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
        const STEPS: u8 = 14;
        (0..=STEPS)
            .map(|i| {
                let t = f32::from(i) / f32::from(STEPS);
                let a = (from + (to - from) * t).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }
}

const CUBE: [(f32, f32); 6] = [
    (12.0, 3.0),
    (20.0, 7.5),
    (20.0, 16.5),
    (12.0, 21.0),
    (4.0, 16.5),
    (4.0, 7.5),
];

impl Icon {
    /// Draws the icon inside `rect` with `color`.
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let width = (rect.width() / 24.0 * 1.9).clamp(1.0, 3.0);
        let pen = Pen {
            painter,
            rect,
            stroke: Stroke::new(width, color),
        };
        match self {
            Icon::Plus => {
                pen.line(&[(12.0, 5.0), (12.0, 19.0)]);
                pen.line(&[(5.0, 12.0), (19.0, 12.0)]);
            }
            Icon::Minus => pen.line(&[(5.0, 12.0), (19.0, 12.0)]),
            Icon::Close => {
                pen.line(&[(6.0, 6.0), (18.0, 18.0)]);
                pen.line(&[(18.0, 6.0), (6.0, 18.0)]);
            }
            Icon::Check => pen.line(&[(5.0, 13.0), (10.0, 18.0), (19.0, 7.0)]),
            Icon::ChevronRight => pen.line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
            Icon::ChevronDown => pen.line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
            Icon::ChevronUp => pen.line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
            Icon::Menu => {
                for y in [7.0, 12.0, 17.0] {
                    pen.line(&[(4.0, y), (20.0, y)]);
                }
            }
            Icon::More => {
                for x in [5.5, 12.0, 18.5] {
                    pen.dot(x, 12.0, 1.8);
                }
            }
            Icon::Search => {
                pen.circle(10.5, 10.5, 6.0);
                pen.line(&[(15.0, 15.0), (20.0, 20.0)]);
            }
            Icon::Settings => {
                pen.circle(12.0, 12.0, 3.4);
                pen.circle(12.0, 12.0, 7.6);
                for i in 0..8_u8 {
                    let a = (f32::from(i) * 45.0).to_radians();
                    let (s, c) = a.sin_cos();
                    pen.line(&[
                        (12.0 + 7.6 * c, 12.0 + 7.6 * s),
                        (12.0 + 10.4 * c, 12.0 + 10.4 * s),
                    ]);
                }
            }
            Icon::Home => {
                pen.line(&[(3.0, 12.0), (12.0, 4.0), (21.0, 12.0)]);
                pen.line(&[(6.0, 10.0), (6.0, 20.0), (18.0, 20.0), (18.0, 10.0)]);
            }
            Icon::File => {
                pen.closed(&[
                    (6.0, 3.0),
                    (14.0, 3.0),
                    (19.0, 8.0),
                    (19.0, 21.0),
                    (6.0, 21.0),
                ]);
                pen.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            }
            Icon::Folder => {
                pen.closed(&[
                    (3.0, 6.0),
                    (9.0, 6.0),
                    (11.0, 9.0),
                    (21.0, 9.0),
                    (21.0, 19.0),
                    (3.0, 19.0),
                ]);
            }
            Icon::Save => {
                pen.frame(4.0, 4.0, 20.0, 20.0);
                pen.frame(8.0, 4.0, 16.0, 9.0);
                pen.frame(7.0, 14.0, 17.0, 20.0);
            }
            Icon::Import => {
                pen.line(&[(6.0, 14.0), (6.0, 20.0), (18.0, 20.0), (18.0, 14.0)]);
                pen.line(&[(12.0, 3.0), (12.0, 15.0)]);
                pen.line(&[(8.0, 11.0), (12.0, 15.0), (16.0, 11.0)]);
            }
            Icon::Export => {
                pen.line(&[(6.0, 10.0), (6.0, 20.0), (18.0, 20.0), (18.0, 10.0)]);
                pen.line(&[(12.0, 15.0), (12.0, 3.0)]);
                pen.line(&[(8.0, 7.0), (12.0, 3.0), (16.0, 7.0)]);
            }
            Icon::Undo => {
                pen.line(&[(9.0, 5.5), (4.5, 10.0), (9.0, 14.5)]);
                pen.line(&[
                    (5.0, 10.0),
                    (14.0, 10.0),
                    (19.0, 12.5),
                    (19.0, 17.0),
                    (14.0, 19.5),
                    (9.0, 19.5),
                ]);
            }
            Icon::Redo => {
                pen.line(&[(15.0, 5.5), (19.5, 10.0), (15.0, 14.5)]);
                pen.line(&[
                    (19.0, 10.0),
                    (10.0, 10.0),
                    (5.0, 12.5),
                    (5.0, 17.0),
                    (10.0, 19.5),
                    (15.0, 19.5),
                ]);
            }
            Icon::Copy => {
                pen.frame(9.0, 9.0, 20.0, 20.0);
                pen.line(&[
                    (6.0, 15.0),
                    (4.0, 15.0),
                    (4.0, 4.0),
                    (15.0, 4.0),
                    (15.0, 6.0),
                ]);
            }
            Icon::Trash => {
                pen.line(&[(4.0, 7.0), (20.0, 7.0)]);
                pen.line(&[(9.0, 7.0), (9.0, 4.0), (15.0, 4.0), (15.0, 7.0)]);
                pen.line(&[(6.5, 7.0), (7.5, 20.0), (16.5, 20.0), (17.5, 7.0)]);
                pen.line(&[(10.0, 10.5), (10.0, 17.0)]);
                pen.line(&[(14.0, 10.5), (14.0, 17.0)]);
            }
            Icon::Eye | Icon::EyeOff => {
                pen.closed(&[
                    (2.0, 12.0),
                    (6.0, 7.5),
                    (12.0, 5.5),
                    (18.0, 7.5),
                    (22.0, 12.0),
                    (18.0, 16.5),
                    (12.0, 18.5),
                    (6.0, 16.5),
                ]);
                pen.circle(12.0, 12.0, 3.0);
                if self == Icon::EyeOff {
                    pen.line(&[(4.0, 20.0), (20.0, 4.0)]);
                }
            }
            Icon::Lock => {
                pen.frame(6.0, 11.0, 18.0, 20.0);
                pen.line(&[
                    (8.5, 11.0),
                    (8.5, 8.0),
                    (10.0, 5.5),
                    (12.0, 5.0),
                    (14.0, 5.5),
                    (15.5, 8.0),
                    (15.5, 11.0),
                ]);
            }
            Icon::Unlock => {
                pen.frame(6.0, 11.0, 18.0, 20.0);
                pen.line(&[
                    (8.5, 11.0),
                    (8.5, 8.0),
                    (10.0, 5.5),
                    (12.0, 5.0),
                    (14.0, 5.5),
                    (15.5, 8.0),
                ]);
            }
            Icon::Cube => {
                pen.closed(&CUBE);
                pen.line(&[(4.0, 7.5), (12.0, 12.0), (20.0, 7.5)]);
                pen.line(&[(12.0, 12.0), (12.0, 21.0)]);
            }
            Icon::Plane => pen.closed(&[(12.0, 4.0), (21.0, 12.0), (12.0, 20.0), (3.0, 12.0)]),
            Icon::Group => {
                pen.frame(3.0, 9.0, 15.0, 20.0);
                pen.line(&[
                    (7.0, 9.0),
                    (7.0, 4.0),
                    (20.0, 4.0),
                    (20.0, 15.0),
                    (15.0, 15.0),
                ]);
            }
            Icon::Mesh => {
                pen.closed(&[(12.0, 4.0), (21.0, 19.0), (3.0, 19.0)]);
                pen.line(&[(12.0, 4.0), (12.0, 19.0)]);
                pen.line(&[(7.5, 11.5), (12.0, 19.0), (16.5, 11.5)]);
            }
            Icon::Voxel => {
                pen.filled(&[(12.0, 3.0), (20.0, 7.5), (12.0, 12.0), (4.0, 7.5)]);
                pen.closed(&CUBE);
                pen.line(&[(12.0, 12.0), (12.0, 21.0)]);
            }
            Icon::Select => pen.closed(&[
                (6.0, 3.0),
                (6.0, 18.0),
                (10.0, 14.5),
                (13.0, 21.0),
                (15.5, 19.8),
                (12.6, 13.5),
                (18.0, 13.0),
            ]),
            Icon::BoxSelect => {
                for (cx, cy, dx, dy) in [
                    (4.0, 4.0, 1.0, 1.0),
                    (20.0, 4.0, -1.0, 1.0),
                    (4.0, 20.0, 1.0, -1.0),
                    (20.0, 20.0, -1.0, -1.0),
                ] {
                    pen.line(&[(cx + dx * 6.0, cy), (cx, cy), (cx, cy + dy * 6.0)]);
                }
            }
            Icon::Move => {
                pen.line(&[(12.0, 3.0), (12.0, 21.0)]);
                pen.line(&[(3.0, 12.0), (21.0, 12.0)]);
                pen.line(&[(9.0, 6.0), (12.0, 3.0), (15.0, 6.0)]);
                pen.line(&[(9.0, 18.0), (12.0, 21.0), (15.0, 18.0)]);
                pen.line(&[(6.0, 9.0), (3.0, 12.0), (6.0, 15.0)]);
                pen.line(&[(18.0, 9.0), (21.0, 12.0), (18.0, 15.0)]);
            }
            Icon::Rotate => {
                pen.line(&Pen::arc(12.0, 12.0, 8.0, -40.0, 235.0));
                pen.line(&[(17.5, 3.5), (18.5, 7.5), (14.5, 8.5)]);
            }
            Icon::Scale => {
                pen.frame(4.0, 10.0, 14.0, 20.0);
                pen.line(&[(11.0, 13.0), (20.0, 4.0)]);
                pen.line(&[(14.5, 4.0), (20.0, 4.0), (20.0, 9.5)]);
            }
            Icon::Pencil => {
                pen.closed(&[
                    (4.0, 20.0),
                    (5.0, 15.0),
                    (16.0, 4.0),
                    (20.0, 8.0),
                    (9.0, 19.0),
                ]);
                pen.line(&[(13.5, 6.5), (17.5, 10.5)]);
            }
            Icon::Eraser => {
                pen.closed(&[
                    (3.5, 15.0),
                    (12.0, 6.0),
                    (19.5, 13.0),
                    (13.0, 19.5),
                    (8.0, 19.5),
                ]);
                pen.line(&[(8.0, 10.5), (15.5, 17.0)]);
                pen.line(&[(13.0, 19.5), (21.0, 19.5)]);
            }
            Icon::Fill => {
                pen.closed(&[(4.0, 12.0), (11.0, 5.0), (18.0, 12.0), (11.0, 19.0)]);
                pen.line(&[(7.0, 9.0), (15.0, 9.0)]);
                pen.line(&[(11.0, 5.0), (11.0, 3.0), (13.0, 3.0)]);
                pen.dot(20.0, 17.0, 1.8);
            }
            Icon::Picker => {
                pen.line(&[(4.0, 20.0), (5.0, 16.0), (14.0, 9.0)]);
                pen.line(&[(4.0, 20.0), (8.0, 19.0), (15.0, 10.0)]);
                pen.circle(17.0, 7.0, 3.2);
            }
            Icon::Line => {
                pen.line(&[(5.0, 19.0), (19.0, 5.0)]);
                pen.dot(5.0, 19.0, 2.2);
                pen.dot(19.0, 5.0, 2.2);
            }
            Icon::Rect => {
                pen.frame(4.0, 6.0, 20.0, 18.0);
                pen.dot(4.0, 6.0, 1.8);
                pen.dot(20.0, 18.0, 1.8);
            }
            Icon::Mirror => {
                pen.line(&[(12.0, 3.0), (12.0, 21.0)]);
                pen.closed(&[(9.0, 7.0), (4.0, 12.0), (9.0, 17.0)]);
                pen.closed(&[(15.0, 7.0), (20.0, 12.0), (15.0, 17.0)]);
            }
            Icon::Magnet => {
                pen.line(&[(6.0, 3.5), (6.0, 12.0)]);
                pen.line(&[(18.0, 3.5), (18.0, 12.0)]);
                pen.line(&Pen::arc(12.0, 12.0, 6.0, 180.0, 0.0));
                pen.line(&[(3.5, 7.0), (8.5, 7.0)]);
                pen.line(&[(15.5, 7.0), (20.5, 7.0)]);
            }
            Icon::Grid => {
                pen.frame(4.0, 4.0, 20.0, 20.0);
                for v in [9.33, 14.67] {
                    pen.line(&[(v, 4.0), (v, 20.0)]);
                    pen.line(&[(4.0, v), (20.0, v)]);
                }
            }
            Icon::Palette => {
                pen.circle(12.0, 12.0, 9.0);
                pen.dot(8.5, 10.0, 1.5);
                pen.dot(12.0, 7.5, 1.5);
                pen.dot(15.5, 10.0, 1.5);
                pen.dot(9.0, 14.5, 1.5);
            }
            Icon::Image => {
                pen.frame(3.0, 5.0, 21.0, 19.0);
                pen.line(&[
                    (3.5, 17.0),
                    (9.0, 11.0),
                    (14.0, 16.0),
                    (17.0, 13.0),
                    (20.5, 17.0),
                ]);
                pen.dot(16.0, 9.0, 1.6);
            }
            Icon::Monitor => {
                pen.frame(3.0, 5.0, 21.0, 16.0);
                pen.line(&[(9.0, 20.0), (15.0, 20.0)]);
                pen.line(&[(12.0, 16.0), (12.0, 20.0)]);
            }
            Icon::History => {
                pen.circle(12.0, 12.0, 9.0);
                pen.line(&[(12.0, 7.0), (12.0, 12.0), (16.0, 14.0)]);
            }
            Icon::Frame => {
                for (cx, cy, dx, dy) in [
                    (4.0, 4.0, 1.0, 1.0),
                    (20.0, 4.0, -1.0, 1.0),
                    (4.0, 20.0, 1.0, -1.0),
                    (20.0, 20.0, -1.0, -1.0),
                ] {
                    pen.line(&[(cx + dx * 4.0, cy), (cx, cy), (cx, cy + dy * 4.0)]);
                }
                pen.dot(12.0, 12.0, 2.2);
            }
            Icon::Warning => {
                pen.closed(&[(12.0, 4.0), (21.0, 20.0), (3.0, 20.0)]);
                pen.line(&[(12.0, 10.0), (12.0, 14.5)]);
                pen.dot(12.0, 17.2, 1.1);
            }
            Icon::Info => {
                pen.circle(12.0, 12.0, 9.0);
                pen.line(&[(12.0, 11.0), (12.0, 17.0)]);
                pen.dot(12.0, 7.6, 1.1);
            }
            Icon::Error => {
                pen.circle(12.0, 12.0, 9.0);
                pen.line(&[(8.5, 8.5), (15.5, 15.5)]);
                pen.line(&[(15.5, 8.5), (8.5, 15.5)]);
            }
            Icon::PanelLeft => {
                pen.frame(3.0, 5.0, 21.0, 19.0);
                pen.line(&[(9.0, 5.0), (9.0, 19.0)]);
            }
            Icon::PanelRight => {
                pen.frame(3.0, 5.0, 21.0, 19.0);
                pen.line(&[(15.0, 5.0), (15.0, 19.0)]);
            }
            Icon::PanelBottom => {
                pen.frame(3.0, 5.0, 21.0, 19.0);
                pen.line(&[(3.0, 14.0), (21.0, 14.0)]);
            }
            Icon::Keyboard => {
                pen.frame(2.5, 6.0, 21.5, 18.0);
                for x in [6.0, 9.5, 13.0, 16.5] {
                    pen.dot(x, 10.0, 0.9);
                }
                pen.line(&[(7.0, 14.5), (17.0, 14.5)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, LayerId, RawInput};

    const ALL: [Icon; 56] = [
        Icon::Plus,
        Icon::Minus,
        Icon::Close,
        Icon::Check,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::ChevronUp,
        Icon::Menu,
        Icon::More,
        Icon::Search,
        Icon::Settings,
        Icon::Home,
        Icon::File,
        Icon::Folder,
        Icon::Save,
        Icon::Import,
        Icon::Export,
        Icon::Undo,
        Icon::Redo,
        Icon::Copy,
        Icon::Trash,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Lock,
        Icon::Unlock,
        Icon::Cube,
        Icon::Plane,
        Icon::Group,
        Icon::Mesh,
        Icon::Voxel,
        Icon::Select,
        Icon::BoxSelect,
        Icon::Move,
        Icon::Rotate,
        Icon::Scale,
        Icon::Pencil,
        Icon::Eraser,
        Icon::Fill,
        Icon::Picker,
        Icon::Line,
        Icon::Rect,
        Icon::Mirror,
        Icon::Magnet,
        Icon::Grid,
        Icon::Palette,
        Icon::Image,
        Icon::Monitor,
        Icon::History,
        Icon::Frame,
        Icon::Warning,
        Icon::Info,
        Icon::Error,
        Icon::PanelLeft,
        Icon::PanelRight,
        Icon::PanelBottom,
        Icon::Keyboard,
    ];

    #[test]
    fn every_icon_paints_inside_its_rect() {
        let ctx = Context::default();
        let layer = LayerId::background();
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(24.0, 24.0));
        let bounds = rect.expand(2.5);
        ctx.run_ui(RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(layer);
            for icon in ALL {
                let before = ui
                    .ctx()
                    .graphics(|g| g.get(layer).map_or(0, |l| l.all_entries().count()));
                icon.paint(&painter, rect, Color32::WHITE);
                let shapes: Vec<Rect> = ui.ctx().graphics(|g| {
                    g.get(layer).map_or_else(Vec::new, |list| {
                        list.all_entries()
                            .skip(before)
                            .map(|clipped| clipped.shape.visual_bounding_rect())
                            .collect()
                    })
                });
                assert!(!shapes.is_empty(), "{icon:?} paints nothing");
                for shape in shapes {
                    assert!(
                        bounds.contains_rect(shape),
                        "{icon:?} leaves its box: {shape:?}"
                    );
                }
            }
        })
        .drop_without_applying_deltas();
    }
}
