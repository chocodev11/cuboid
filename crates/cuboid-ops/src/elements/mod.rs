//! Element operations: delete, rename, visibility and lock, copy, paste and duplicate (spec:
//! tools 10.2-10.6, 11).

mod copy;
mod edit;

pub use copy::{copy, duplicate, paste};
pub use edit::{delete, rename, set_hidden, set_locked, show_all, unlock_all};
