//! Back-to-front sorting and transparency run batching (spec: app 8.6; arch 13.4).
//!
//! The scene crate sorts the transparent queue; the renderer keeps its order and only merges
//! neighbours that one draw can cover. Two binary element runs that follow each other in the queue
//! and in the instance buffer are one instanced draw: within a draw the GPU keeps primitive order
//! for depth and blending, so no pixel changes. Translucent runs stay apart, because each of them
//! is a pass pair in sorted order.

use cuboid_scene::{DrawItem, DrawTarget};

/// Merges consecutive binary element runs that touch in the instance buffer.
pub(crate) fn merge_runs(items: &[DrawItem]) -> Vec<DrawItem> {
    let mut merged: Vec<DrawItem> = Vec::with_capacity(items.len());
    for item in items {
        if let (
            Some(DrawItem {
                target: DrawTarget::Elements { first, count },
                translucent,
            }),
            DrawTarget::Elements {
                first: next_first,
                count: next_count,
            },
        ) = (merged.last_mut(), item.target)
            && !*translucent
            && !item.translucent
            && first.checked_add(*count) == Some(next_first)
        {
            *count += next_count;
            continue;
        }
        merged.push(*item);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(first: u32, count: u32, translucent: bool) -> DrawItem {
        DrawItem {
            target: DrawTarget::Elements { first, count },
            translucent,
        }
    }

    #[test]
    fn touching_binary_runs_merge_and_translucent_ones_do_not() {
        let merged = merge_runs(&[
            run(0, 2, false),
            run(2, 3, false),
            run(5, 1, true),
            run(6, 1, true),
        ]);
        assert_eq!(merged, [run(0, 5, false), run(5, 1, true), run(6, 1, true)]);
    }

    #[test]
    fn a_gap_or_a_translucent_neighbour_keeps_the_runs_apart() {
        let items = [run(0, 2, false), run(3, 1, false), run(4, 1, true)];
        assert_eq!(merge_runs(&items), items);
    }
}
