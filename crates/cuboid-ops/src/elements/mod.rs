//! Element operations: delete, rename, visibility and lock, copy, paste and duplicate, colour,
//! shading and the face switches (spec: tools 10.2-10.8, 11).

mod copy;
mod edit;
mod props;

pub use copy::{copy, duplicate, paste};
pub use edit::{delete, rename, set_hidden, set_locked, show_all, unlock_all};
pub(crate) use props::{edit_elements, wanted};
pub use props::{set_color, set_face_active, set_face_cull, set_face_tint, set_shade};
