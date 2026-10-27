//! The signs of keyframe values. Blockbench 5 keeps the X of position and rotation and the Y of
//! rotation with the sign opposite to the one a project older than 5.0 has, and converts such a
//! project when it loads it. The model has the convention of Blockbench 5: the reader converts a
//! project older than 5.0, the writer, which writes version 4.10, converts back. See
//! `docs/design/bbmodel_animation_fixes.md`.

use cuboid_model::{Channel, Handles, Value as Data};
use serde_json::{Number, Value};

use crate::animations::count;

/// Whether a project of this version has the old signs.
pub(crate) fn is_old(version: (u32, u32)) -> bool {
    version < (5, 0)
}

/// The axes of a channel whose sign differs between the versions.
pub(crate) fn flipped(channel: Channel) -> [bool; 3] {
    match channel {
        Channel::Rotation => [true, true, false],
        Channel::Position => [true, false, false],
        Channel::Scale => [false; 3],
    }
}

/// The handles of a keyframe with the signs of the other version.
pub(crate) fn flip_handles(handles: &mut Handles, channel: Channel) {
    for values in [&mut handles.left_value, &mut handles.right_value] {
        for (value, flip) in values.iter_mut().zip(flipped(channel)) {
            if flip {
                *value = -*value;
            }
        }
    }
}

/// The value of an old project's text in the convention of the model. The text of the project is
/// kept on the value when the conversion changes it.
pub(crate) fn read_text(text: &str) -> Data {
    let inverted = invert_molang(text);
    if inverted == text {
        Data::from_text(text)
    } else {
        Data::converted(&inverted, text)
    }
}

/// A value as a project of the old signs writes it: text that was read from such a project as it
/// was, any other number or text inverted.
pub(crate) fn write_old(value: &Data) -> Value {
    match value {
        Data::Num(n) => count(-n),
        Data::Text {
            source: Some(source),
            ..
        } => Value::String(source.to_string()),
        Data::Text { text, .. } => Value::String(invert_molang(text)),
    }
}

/// Gives the signs of 4.10, which the writer uses, to an animator that the model keeps as JSON
/// from a project of version 5.0 or later. The loader of the editor converts every animator of an
/// older project, whatever its uuid or type: the position and rotation keyframes, the X of both
/// and the Y of rotation of every data point (a number is negated, text inverted, zero and empty
/// text stay), and the same axes of the handles of a Bezier keyframe.
pub(crate) fn to_old_animator(animator: &mut Value) {
    let Some(keyframes) = animator.get_mut("keyframes").and_then(Value::as_array_mut) else {
        return;
    };
    for keyframe in keyframes {
        let channel = keyframe
            .get("channel")
            .and_then(Value::as_str)
            .and_then(Channel::from_key);
        let Some(flips) = channel.map(flipped).filter(|flips| flips.contains(&true)) else {
            continue;
        };
        let points = keyframe
            .get_mut("data_points")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten();
        for point in points {
            for (axis, flip) in ["x", "y", "z"].into_iter().zip(flips) {
                if let Some(value) = point.get_mut(axis).filter(|_| flip) {
                    invert_json(value);
                }
            }
        }
        if keyframe.get("interpolation").and_then(Value::as_str) != Some("bezier") {
            continue;
        }
        for member in ["bezier_left_value", "bezier_right_value"] {
            let values = keyframe
                .get_mut(member)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten();
            for (value, flip) in values.zip(flips) {
                if flip {
                    invert_json(value);
                }
            }
        }
    }
}

/// [`to_old_animator`] for every animator of an animation.
pub(crate) fn to_old_animation(animation: &mut Value) {
    let animators = animation
        .get_mut("animators")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|animators| animators.values_mut());
    for animator in animators {
        to_old_animator(animator);
    }
}

/// A number or a text of a project with the opposite value; anything else stays.
fn invert_json(value: &mut Value) {
    match value {
        Value::Number(n) => {
            let negated = match n.as_i64() {
                Some(0) => None,
                Some(i) => i.checked_neg().map(Number::from),
                None => n
                    .as_f64()
                    .filter(|f| *f != 0.0)
                    .and_then(|f| Number::from_f64(-f)),
            };
            if let Some(negated) = negated {
                *n = negated;
            }
        }
        Value::String(text) => *text = invert_molang(text),
        _ => {}
    }
}

/// `text`, the value of a keyframe in Molang, changed so that it gives the opposite value. A
/// number is negated; an expression that is one product loses a leading minus or gains one; any
/// other is put in parentheses with a minus before it. Of a script, only the value it returns
/// changes: the `return` statements, else the last statement unless that assigns.
pub(crate) fn invert_molang(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return text.to_owned();
    }
    if let Some(number) = plain_number(trimmed) {
        return if number == 0.0 {
            "0".to_owned()
        } else {
            (-number).to_string()
        };
    }
    if trimmed.contains("return ") {
        return invert_returns(trimmed);
    }
    let body = trimmed.trim_end_matches(';');
    match last_semicolon(body) {
        Some(at) => {
            let (head, rest) = body.split_at(at);
            let tail = rest.strip_prefix(';').unwrap_or(rest);
            if is_assignment(tail) {
                trimmed.to_owned()
            } else {
                format!("{head};{}", negate(tail))
            }
        }
        None if is_assignment(body) => trimmed.to_owned(),
        None => negate(body),
    }
}

/// The offset of the last `;` that is not inside brackets.
fn last_semicolon(script: &str) -> Option<usize> {
    let mut depth = 0_u32;
    let mut last = None;
    for (at, c) in script.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => last = Some(at),
            _ => {}
        }
    }
    last
}

/// The offset where the statement at the start of `text` ends: a `;` that is not inside brackets,
/// or the bracket that closes the block it is in. The length of `text` when neither comes.
fn statement_end(text: &str) -> usize {
    let mut depth = 0_u32;
    for (at, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => match depth.checked_sub(1) {
                Some(open) => depth = open,
                None => return at,
            },
            ';' if depth == 0 => return at,
            _ => {}
        }
    }
    text.len()
}

/// A decimal number as a keyframe writes it: digits, then a fraction that may end in `f`, with a
/// minus before it.
fn plain_number(text: &str) -> Option<f64> {
    let body = text.strip_prefix('-').unwrap_or(text);
    let body = match body.strip_suffix('f') {
        Some(rest) if rest.contains('.') => rest,
        _ => body,
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let well_formed = match body.split_once('.') {
        Some((whole, fraction)) => digits(whole) && digits(fraction),
        None => digits(body),
    };
    if !well_formed {
        return None;
    }
    let value: f64 = body.parse().ok()?;
    Some(if text.starts_with('-') { -value } else { value })
}

/// Negates the expression of every `return` of a script.
fn invert_returns(script: &str) -> String {
    const RETURN: &str = "return ";
    let mut out = String::with_capacity(script.len() + 2);
    let mut rest = script;
    while let Some(at) = rest.find(RETURN) {
        let (head, tail) = rest.split_at(at + RETURN.len());
        out.push_str(head);
        let (expression, after) = tail.split_at(statement_end(tail));
        let value = expression.trim_end();
        out.push_str(&negate(value));
        out.push_str(expression.split_at(value.len()).1);
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Whether a statement assigns: a single `=` outside brackets, not part of a comparison.
fn is_assignment(statement: &str) -> bool {
    let bytes = statement.as_bytes();
    let mut depth = 0_u32;
    bytes.iter().enumerate().any(|(at, &b)| {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let before = at.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let after = bytes.get(at + 1).copied();
        depth == 0
            && b == b'='
            && after != Some(b'=')
            && !matches!(before, Some(b'=' | b'!' | b'<' | b'>'))
    })
}

/// An expression with the opposite value.
fn negate(expression: &str) -> String {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return expression.to_owned();
    }
    let (minus, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, trimmed),
    };
    if !is_product(rest) {
        return format!("-({trimmed})");
    }
    if minus {
        rest.to_owned()
    } else {
        format!("-{rest}")
    }
}

/// Whether an expression has no operator outside brackets that binds weaker than a product, so
/// that a minus before it applies to all of it.
fn is_product(expression: &str) -> bool {
    let mut depth = 0_u32;
    for c in expression.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '+' | '-' | '?' | ':' | '<' | '>' | '=' | '!' | '&' | '|' | ';' if depth == 0 => {
                return false;
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::invert_molang as inverted;
    use super::*;

    #[test]
    fn numbers_in_text_are_negated_and_zero_stays() {
        assert_eq!(inverted("1.5"), "-1.5");
        assert_eq!(inverted("-1.5"), "1.5");
        assert_eq!(inverted("0.50"), "-0.5");
        assert_eq!(inverted("12"), "-12");
        assert_eq!(inverted("2.5f"), "-2.5");
        assert_eq!(inverted(" 3 "), "-3");
        assert_eq!(inverted("0"), "0");
        assert_eq!(inverted("0.0"), "0");
        assert_eq!(inverted("-0"), "0");
        assert_eq!(inverted(""), "");
    }

    #[test]
    fn a_product_gains_or_loses_a_minus() {
        assert_eq!(
            inverted("math.sin(query.anim_time * 90)"),
            "-math.sin(query.anim_time * 90)"
        );
        assert_eq!(inverted("-math.sin(q.t) * 30"), "math.sin(q.t) * 30");
        assert_eq!(inverted("v.a * v.b / 2"), "-v.a * v.b / 2");
        assert_eq!(inverted(".5"), "-.5");
    }

    #[test]
    fn a_sum_or_a_condition_is_wrapped() {
        assert_eq!(inverted("v.a + 1"), "-(v.a + 1)");
        assert_eq!(inverted("-v.a + 1"), "-(-v.a + 1)");
        assert_eq!(inverted("v.a * -1"), "-(v.a * -1)");
        assert_eq!(inverted("q.t > 1 ? 4 : 5"), "-(q.t > 1 ? 4 : 5)");
        // Operators inside brackets do not count.
        assert_eq!(
            inverted("math.max(v.a, 1 - v.b)"),
            "-math.max(v.a, 1 - v.b)"
        );
    }

    #[test]
    fn a_script_changes_the_value_it_returns() {
        assert_eq!(inverted("v.x = 1; v.x * 2"), "v.x = 1;-v.x * 2");
        assert_eq!(
            inverted("v.x = 1; return v.x + 1;"),
            "v.x = 1; return -(v.x + 1);"
        );
        assert_eq!(
            inverted("q.t > 1 ? { return 2; } : { return 3; }"),
            "q.t > 1 ? { return -2; } : { return -3; }"
        );
        // A script that ends in an assignment has no value to invert.
        assert_eq!(inverted("v.x = 1"), "v.x = 1");
        assert_eq!(inverted("v.y = 2; v.x = v.y"), "v.y = 2; v.x = v.y");
        assert_eq!(inverted("q.a == 1"), "-(q.a == 1)");
    }

    #[test]
    fn a_script_is_split_outside_brackets_only() {
        assert_eq!(
            inverted("v.x = 1; loop(2, { v.x = v.x * 2; })"),
            "v.x = 1;-loop(2, { v.x = v.x * 2; })"
        );
        assert_eq!(
            inverted("loop(2, { v.x = v.x + 1; }); return v.x * 2;"),
            "loop(2, { v.x = v.x + 1; }); return -v.x * 2;"
        );
        // A statement in braces is not an assignment of the script, and a return ends at the
        // bracket that closes its block.
        assert_eq!(
            inverted("v.x = 1; q.t > 1 ? { v.y = 2; }"),
            "v.x = 1;-(q.t > 1 ? { v.y = 2; })"
        );
        assert_eq!(
            inverted("q.t > 1 ? { return 2 } : { return 3 }"),
            "q.t > 1 ? { return -2 } : { return -3 }"
        );
    }

    #[test]
    fn a_kept_animator_gets_the_old_signs_on_its_axes() {
        let mut animator = serde_json::json!({"type": "null_object", "keyframes": [
            {"channel": "rotation", "interpolation": "bezier",
             "data_points": [{"x": 1, "y": "2.5", "z": "3"}, {"x": 0, "y": "", "z": 4.5}],
             "bezier_left_value": [1, 2, 3], "bezier_right_value": [0.5, -2, 3]},
            {"channel": "position", "interpolation": "linear",
             "data_points": [{"x": "v.a + 1", "y": 2, "z": 3}], "bezier_left_value": [1, 2, 3]},
            {"channel": "scale", "data_points": [{"x": 2, "y": 2, "z": 2}]},
            {"channel": "sound", "data_points": [{"effect": "x"}]},
        ]});
        to_old_animator(&mut animator);
        let expected = serde_json::json!({"type": "null_object", "keyframes": [
            {"channel": "rotation", "interpolation": "bezier",
             "data_points": [{"x": -1, "y": "-2.5", "z": "3"}, {"x": 0, "y": "", "z": 4.5}],
             "bezier_left_value": [-1, -2, 3], "bezier_right_value": [-0.5, 2, 3]},
            {"channel": "position", "interpolation": "linear",
             "data_points": [{"x": "-(v.a + 1)", "y": 2, "z": 3}],
             "bezier_left_value": [1, 2, 3]},
            {"channel": "scale", "data_points": [{"x": 2, "y": 2, "z": 2}]},
            {"channel": "sound", "data_points": [{"effect": "x"}]},
        ]});
        assert_eq!(animator, expected);
    }

    #[test]
    fn inverting_twice_gives_the_same_value() {
        for text in ["math.cos(q.t * 3)", "-q.t * 2", "1.25", "-4"] {
            assert_eq!(inverted(&inverted(text)), text);
        }
        // A wrapped sum comes back in its brackets; a number in its shortest form, which is why a
        // value read from a project keeps the text the project had.
        assert_eq!(inverted(&inverted("v.a + v.b")), "(v.a + v.b)");
        assert_eq!(inverted(&inverted("0.50")), "0.5");
    }

    #[test]
    fn text_read_from_an_old_project_keeps_its_source() {
        let value = read_text("0.50");
        assert_eq!(value.number(), Some(-0.5));
        assert_eq!(write_old(&value), Value::String("0.50".to_owned()));
        // A value the conversion leaves alone has no source to keep.
        let zero = read_text("0");
        assert_eq!(zero, Data::from_text("0"));
        // A value of the model written as it is: inverted.
        assert_eq!(write_old(&Data::Num(2.5)), serde_json::json!(-2.5));
        assert_eq!(
            write_old(&Data::from_text("3")),
            Value::String("-3".to_owned())
        );
    }
}
