//! Ordered palette of distinct non-transparent voxel values, 0 first (spec: tools 7.3-7.4).

use std::collections::hash_map::Entry;

use rustc_hash::FxHashMap;

/// The ordered list of voxel values a world has used, starting `[0]`. Every write of a value
/// with non-zero alpha appends it when absent; reset, "Color all" and the removal of a swatch
/// are the only removals. Forced imports can add duplicates, so lookups return the first
/// occurrence.
///
/// spec: tools 7.4, arch 8.7
#[derive(Clone, Debug)]
pub struct Palette {
    values: Vec<u32>,
    /// The position of the first occurrence of each value, so a write checks membership in O(1).
    first: FxHashMap<u32, usize>,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            values: vec![0],
            first: FxHashMap::from_iter([(0, 0)]),
        }
    }
}

impl Palette {
    /// The values in palette order.
    pub fn values(&self) -> &[u32] {
        &self.values
    }

    /// The position of the first occurrence of `value`.
    pub fn position(&self, value: u32) -> Option<usize> {
        self.first.get(&value).copied()
    }

    /// Appends `value` when its alpha is non-zero and it is absent: the update of every write.
    ///
    /// spec: tools 7.4
    pub fn insert(&mut self, value: u32) {
        if value >> 24 == 0 {
            return;
        }
        if let Entry::Vacant(slot) = self.first.entry(value) {
            slot.insert(self.values.len());
            self.values.push(value);
        }
    }

    /// Appends `value` without the absence and the alpha test, the forced mode of some palette
    /// imports. The first occurrence stays the one that lookups find.
    ///
    /// spec: tools 7.4
    pub fn push_forced(&mut self, value: u32) {
        self.first.entry(value).or_insert(self.values.len());
        self.values.push(value);
    }

    /// Takes every entry whose colour channels are `rgb` out of the list, whatever its alpha, and
    /// keeps the order of the rest. Entry 0 stays. Returns whether an entry went.
    pub fn remove(&mut self, rgb: u32) -> bool {
        let before = self.values.len();
        let mut at = 0;
        self.values.retain(|&value| {
            at += 1;
            at == 1 || value & 0x00FF_FFFF != rgb
        });
        if self.values.len() == before {
            return false;
        }
        self.first.clear();
        for (at, &value) in self.values.iter().enumerate() {
            self.first.entry(value).or_insert(at);
        }
        true
    }

    /// Replaces the first occurrence of `old` by `new`, keeping its position: the global colour
    /// replacement of tools 7.4. Does nothing when `old` is absent; a later duplicate of `old`
    /// becomes the one that lookups find.
    ///
    /// spec: tools 7.4
    pub fn rename(&mut self, old: u32, new: u32) {
        if old == new {
            return;
        }
        let Some(at) = self.first.remove(&old) else {
            return;
        };
        if let Some(entry) = self.values.get_mut(at) {
            *entry = new;
        }
        if let Some(next) = self.values.iter().skip(at + 1).position(|&v| v == old) {
            self.first.insert(old, at + 1 + next);
        }
        self.first
            .entry(new)
            .and_modify(|first| *first = (*first).min(at))
            .or_insert(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_palette_starts_with_zero() {
        let palette = Palette::default();
        assert_eq!(palette.values(), [0]);
        assert_eq!(palette.position(0), Some(0));
        assert_eq!(palette.position(0xFF00_0000), None);
    }

    #[test]
    fn insert_appends_distinct_values_with_alpha_in_first_use_order() {
        let mut palette = Palette::default();
        for value in [0xFF11_2233, 0xFF44_5566, 0xFF11_2233, 0, 0x8000_0001] {
            palette.insert(value);
        }
        assert_eq!(palette.values(), [0, 0xFF11_2233, 0xFF44_5566, 0x8000_0001]);
        assert_eq!(palette.position(0xFF44_5566), Some(2));
    }

    #[test]
    fn a_value_without_alpha_is_never_inserted() {
        let mut palette = Palette::default();
        palette.insert(0x00FF_FFFF);
        assert_eq!(palette.values(), [0]);
        assert_eq!(palette.position(0x00FF_FFFF), None);
    }

    #[test]
    fn forced_values_may_repeat_and_lookups_find_the_first_occurrence() {
        let mut palette = Palette::default();
        palette.insert(0xFF11_2233);
        palette.push_forced(0xFF11_2233);
        palette.push_forced(0x0000_0007);
        palette.push_forced(0x0000_0007);
        assert_eq!(
            palette.values(),
            [0, 0xFF11_2233, 0xFF11_2233, 7, 7],
            "duplicates and transparent values stay"
        );
        assert_eq!(palette.position(0xFF11_2233), Some(1));
        assert_eq!(palette.position(7), Some(3));

        // A normal insert finds a forced value present.
        palette.push_forced(0xFF44_5566);
        palette.insert(0xFF44_5566);
        assert_eq!(palette.values().len(), 6);
    }

    #[test]
    fn rename_keeps_the_position_and_updates_the_lookup() {
        let mut palette = Palette::default();
        for value in [0xFF00_0001, 0xFF00_0002, 0xFF00_0003] {
            palette.insert(value);
        }
        palette.rename(0xFF00_0002, 0xFF00_0009);
        assert_eq!(palette.values(), [0, 0xFF00_0001, 0xFF00_0009, 0xFF00_0003]);
        assert_eq!(palette.position(0xFF00_0002), None);
        assert_eq!(palette.position(0xFF00_0009), Some(2));
        palette.rename(0xFF00_0777, 5);
        assert_eq!(palette.values().len(), 4);
    }

    #[test]
    fn remove_takes_every_entry_of_the_colour_and_keeps_the_order() {
        let mut palette = Palette::default();
        for value in [0xFF00_0001, 0xFF00_0002, 0xFF00_0003] {
            palette.insert(value);
        }
        palette.push_forced(0x8000_0002);
        assert!(palette.remove(0x00_0002));
        assert_eq!(palette.values(), [0, 0xFF00_0001, 0xFF00_0003]);
        assert_eq!(palette.position(0xFF00_0003), Some(2));
        assert_eq!(palette.position(0xFF00_0002), None);
        assert!(!palette.remove(0x00_0002));
        // Black is a colour of the list; the empty value at the front is not.
        palette.insert(0xFF00_0000);
        assert!(palette.remove(0));
        assert_eq!(palette.values(), [0, 0xFF00_0001, 0xFF00_0003]);
        assert!(!palette.remove(0));
    }

    #[test]
    fn rename_moves_the_first_occurrence_of_a_duplicate_and_merges_into_an_earlier_one() {
        let mut palette = Palette::default();
        palette.insert(0xFF00_0001);
        palette.insert(0xFF00_0002);
        palette.push_forced(0xFF00_0001);
        palette.rename(0xFF00_0001, 0xFF00_0002);
        assert_eq!(palette.values(), [0, 0xFF00_0002, 0xFF00_0002, 0xFF00_0001]);
        assert_eq!(palette.position(0xFF00_0001), Some(3));
        assert_eq!(palette.position(0xFF00_0002), Some(1));
    }
}
