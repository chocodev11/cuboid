use std::fmt::Write;

use cuboid_num::numtext::nj;

use super::push_json_string;

/// A streaming writer with the layout of the original's JSON serializer, used by the `.js`,
/// `.jpm`, Zoxel and Stonehearth exporters.
///
/// - Indented mode puts every member and item on its own line, two spaces per level, with CRLF
///   between lines and `": "` after a name. Compact mode writes no white space at all.
/// - Primitive arrays ([`floats`](Self::floats), [`ints`](Self::ints),
///   [`strings`](Self::strings)) are compact in both modes, as in `[8.0,0.0,-8.0]`; arrays of
///   objects and arrays of arrays go through [`begin_array`](Self::begin_array).
/// - Floats use the NJ text, so `8.0` and `0.1`.
/// - The serializer omits null members, and so does the caller: a member that is not written is
///   not there. Members appear in the order they are written, the declaration order of the
///   original's classes.
/// - Empty arrays and objects are `[]` and `{}`. There is no final newline.
///
/// The calls must nest properly: every `begin_*` has its `end_*`, and a name is written only
/// inside an object and is followed by exactly one value.
///
/// spec: interop 3.3, interop 3.4, interop 11.1 (step 6)
pub struct NetJson {
    out: String,
    compact: bool,
    /// One flag per open container: whether it holds an item yet.
    open: Vec<bool>,
    /// A name was written and its value comes next.
    after_name: bool,
}

impl NetJson {
    /// A writer for the indented layout, or the compact one ("Compress data").
    pub fn new(compact: bool) -> Self {
        Self {
            out: String::new(),
            compact,
            open: Vec::new(),
            after_name: false,
        }
    }

    /// Ends the text and returns it.
    pub fn finish(self) -> String {
        self.out
    }

    fn line_break(&mut self) {
        if self.compact {
            return;
        }
        self.out.push_str("\r\n");
        for _ in 0..self.open.len() {
            self.out.push_str("  ");
        }
    }

    /// The separator and the line break before a member name or an array item.
    fn next_item(&mut self) {
        let Some(has_items) = self.open.last_mut() else {
            return;
        };
        if std::mem::replace(has_items, true) {
            self.out.push(',');
        }
        self.line_break();
    }

    fn before_value(&mut self) {
        if self.after_name {
            self.after_name = false;
        } else {
            self.next_item();
        }
    }

    fn begin(&mut self, bracket: char) {
        self.before_value();
        self.out.push(bracket);
        self.open.push(false);
    }

    fn end(&mut self, bracket: char) {
        if self.open.pop() == Some(true) {
            self.line_break();
        }
        self.out.push(bracket);
    }

    /// Opens an object.
    pub fn begin_object(&mut self) {
        self.begin('{');
    }

    /// Closes the innermost object.
    pub fn end_object(&mut self) {
        self.end('}');
    }

    /// Opens an array whose items are written one by one.
    pub fn begin_array(&mut self) {
        self.begin('[');
    }

    /// Closes the innermost array.
    pub fn end_array(&mut self) {
        self.end(']');
    }

    /// Writes the name of the next member.
    pub fn name(&mut self, name: &str) {
        self.next_item();
        push_json_string(&mut self.out, name);
        self.out.push(':');
        if !self.compact {
            self.out.push(' ');
        }
        self.after_name = true;
    }

    /// Writes a string value.
    pub fn string(&mut self, value: &str) {
        self.before_value();
        push_json_string(&mut self.out, value);
    }

    /// Writes `true` or `false`.
    pub fn bool(&mut self, value: bool) {
        self.before_value();
        self.out.push_str(if value { "true" } else { "false" });
    }

    /// Writes an integer value.
    pub fn int(&mut self, value: i64) {
        self.before_value();
        // Writing to a String cannot fail.
        let _ = write!(self.out, "{value}");
    }

    /// Writes a float value in NJ form.
    pub fn float(&mut self, value: f32) {
        self.before_value();
        self.out.push_str(&nj(value));
    }

    fn compact_array<T>(&mut self, items: &[T], mut write: impl FnMut(&mut String, &T)) {
        self.before_value();
        self.out.push('[');
        for (at, item) in items.iter().enumerate() {
            if at > 0 {
                self.out.push(',');
            }
            write(&mut self.out, item);
        }
        self.out.push(']');
    }

    /// Writes an array of floats on one line.
    pub fn floats(&mut self, items: &[f32]) {
        self.compact_array(items, |out, item| out.push_str(&nj(*item)));
    }

    /// Writes an array of integers on one line.
    pub fn ints(&mut self, items: &[i32]) {
        // Writing to a String cannot fail.
        self.compact_array(items, |out, item| {
            let _ = write!(out, "{item}");
        });
    }

    /// Writes an array of strings on one line.
    pub fn strings(&mut self, items: &[&str]) {
        self.compact_array(items, |out, item| push_json_string(out, item));
    }
}

#[cfg(test)]
mod tests;
