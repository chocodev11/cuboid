//! The renderer behind the render tools. The server has no GPU of its own: the application
//! implements [`Renderer`] with an offscreen device, and this crate makes the contact sheet, the
//! background, the pictures on disk and the numbers about the picture (spec: agent toolkit R1,
//! 5.2; the tools are in `views`).

use cuboid_model::Document;

use crate::picture::Picture;

/// Where a picture is taken from, in the space of a Java model (y up, z south). The six faces
/// look at the face of that name from outside, orthographic: `front` the north face (-z), `back`
/// the south face, `left` the west face (-x), `right` the east face, `top` and `bottom` the
/// faces up and down. `iso` looks from above at the corner of the north and east faces, `gui`
/// is the `gui` display slot of the model: the orthographic view of an inventory slot with the
/// light of the GUI.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Camera {
    /// The north face.
    Front,
    /// The south face.
    Back,
    /// The west face.
    Left,
    /// The east face.
    Right,
    /// From above.
    Top,
    /// From below.
    Bottom,
    /// From above, at the corner of the north and east faces.
    Iso,
    /// The inventory slot.
    Gui,
}

impl Camera {
    /// Every camera, in the order the tools list them.
    pub const ALL: [Camera; 8] = [
        Camera::Front,
        Camera::Back,
        Camera::Left,
        Camera::Right,
        Camera::Top,
        Camera::Bottom,
        Camera::Iso,
        Camera::Gui,
    ];

    /// The name the tools use.
    pub fn name(self) -> &'static str {
        match self {
            Camera::Front => "front",
            Camera::Back => "back",
            Camera::Left => "left",
            Camera::Right => "right",
            Camera::Top => "top",
            Camera::Bottom => "bottom",
            Camera::Iso => "iso",
            Camera::Gui => "gui",
        }
    }

    /// The camera a name stands for.
    pub fn from_name(name: &str) -> Option<Camera> {
        Camera::ALL.into_iter().find(|camera| camera.name() == name)
    }
}

/// Why a picture could not be made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    /// There is no graphics adapter, not even a software one.
    NoGpu,
    /// `size` is over what the adapter can draw; the number is the largest side it takes.
    TooLarge(u32),
    /// The adapter or the renderer failed; the text says how.
    Failed(String),
}

/// What the render tools draw with.
pub trait Renderer {
    /// A picture of `size` by `size` pixels of the document seen from `camera`, framed so that
    /// the model fills it, on a transparent background. The orthographic cameras of one document
    /// share one scale, framed from the 3D bounds of the model and not from each projection, so
    /// that the views of a contact sheet can be compared. A `size` over the adapter limit is
    /// [`RenderError::TooLarge`], not `Failed`. The document is not changed and its cameras are
    /// not used.
    fn render(&mut self, doc: &Document, camera: Camera, size: u32)
    -> Result<Picture, RenderError>;
}

/// The renderer of a machine without an adapter: every render is `E_NO_GPU`.
pub struct NoRenderer;

impl Renderer for NoRenderer {
    fn render(
        &mut self,
        _doc: &Document,
        _camera: Camera,
        _size: u32,
    ) -> Result<Picture, RenderError> {
        Err(RenderError::NoGpu)
    }
}
