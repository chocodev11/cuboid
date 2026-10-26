//! The text of a model file (interop 7.3 to 7.9, 31.2).

use cuboid_io::text::{mc_json::minify, push_json_string};
use cuboid_model::mc;
use cuboid_model::{DisplaySlot, Element, ElementFlags, Face, GuiLight, ObjRef, OverrideCase};
use cuboid_num::numtext::x9;
use glam::Vec3;

use super::{Mode, Prepared, texture_path};
use crate::Notice;
use crate::json_import::is_generated;

/// The display slot names in the order of the model.
const SLOT_KEYS: [&str; 8] = [
    "thirdperson_righthand",
    "thirdperson_lefthand",
    "firstperson_righthand",
    "firstperson_lefthand",
    "gui",
    "head",
    "ground",
    "fixed",
];

fn string(text: &str) -> String {
    let mut out = String::new();
    push_json_string(&mut out, text);
    out
}

fn vec3(v: Vec3) -> String {
    format!("[ {}, {}, {} ]", x9(v.x), x9(v.y), x9(v.z))
}

/// The text of the model file. The document must have been through [`super::prepare`].
pub(crate) fn model_text(prepared: &Prepared, mode: Mode, notices: &mut Vec<Notice>) -> String {
    let doc = &prepared.doc;
    let props = &doc.props;
    let minified = props.general.minify;
    let comments = !minified && (props.minecraft.element_names || mode == Mode::Pack);

    let mut members: Vec<String> = Vec::new();
    if !props.general.description.is_empty() {
        members.push(format!(
            "\t\"__comment\": {}",
            string(&props.general.description)
        ));
    }
    if !props.minecraft.parent.is_empty() {
        members.push(format!("\t\"parent\": {}", string(&props.minecraft.parent)));
    }
    if !props.minecraft.ambient_occlusion {
        members.push("\t\"ambientocclusion\": false".to_owned());
    }
    // The game's default is side, except under the `generated` family, which is front.
    let parent = &props.minecraft.parent;
    let parent_front = is_generated(parent.strip_prefix("minecraft:").unwrap_or(parent));
    match (props.minecraft.gui_light, parent_front) {
        (GuiLight::Front, false) => members.push("\t\"gui_light\": \"front\"".to_owned()),
        (GuiLight::Side, true) => members.push("\t\"gui_light\": \"side\"".to_owned()),
        _ => {}
    }

    let entries: Vec<String> = doc
        .textures
        .entries
        .iter()
        .map(|entry| {
            format!(
                "\t\t{}: {}",
                string(&entry.label),
                string(&texture_path(&entry.texture.source))
            )
        })
        .collect();
    members.push(if entries.is_empty() {
        "\t\"textures\": {\n\t}".to_owned()
    } else {
        format!("\t\"textures\": {{\n{}\n\t}}", entries.join(",\n"))
    });

    let hidden_out = mode == Mode::Plain && props.general.exclude_hidden;
    let elements: Vec<String> = doc
        .elements
        .ids()
        .iter()
        .zip(doc.elements.items())
        .filter(|(id, el)| {
            !(hidden_out && doc.is_hidden(ObjRef::Element(**id)))
                && el.faces.iter().any(|f| f.active)
        })
        .map(|(_, el)| element(el, prepared, comments))
        .collect();
    members.push(format!("\t\"elements\": [\n{}\n\t]", elements.join(",\n")));

    if (props.minecraft.include_display || mode == Mode::Pack)
        && let Some(display) = display(&doc.display.slots, mode == Mode::Pack)
    {
        members.push(display);
    }

    let has_overrides = !doc.overrides.is_empty();
    if prepared.tier.item_definitions {
        if has_overrides && prepared.item {
            notices.push(Notice::OverridesNotConverted {
                count: doc.overrides.len(),
            });
        }
    } else if has_overrides && (props.minecraft.keep_overrides || mode == Mode::Pack) {
        members.push(overrides(&doc.overrides));
    }

    let text = format!("{{\n{}\n}}", members.join(",\n"));
    if minified { minify(&text) } else { text }
}

fn element(el: &Element, prepared: &Prepared, comments: bool) -> String {
    let b = mc::unplace(
        el.base.p,
        el.base.s,
        el.base.o,
        el.base.e,
        el.flags.contains(ElementFlags::INVERT_NORMALS),
    );
    let mut lines: Vec<String> = Vec::new();
    if comments && !el.base.label.is_empty() {
        lines.push(format!("\t\t\t\"__comment\": {}", string(&el.base.label)));
    }
    lines.push(format!("\t\t\t\"from\": {}", vec3(b.from)));
    lines.push(format!("\t\t\t\"to\": {}", vec3(b.to)));
    if b.rotation != Vec3::ZERO {
        let rescale = if el.flags.contains(ElementFlags::RESCALE) {
            ", \"rescale\": true"
        } else {
            ""
        };
        let angles = match mc::single_axis(b.rotation) {
            Some((axis, angle)) => format!("\"axis\": \"{axis}\", \"angle\": {}", x9(angle)),
            None => format!(
                "\"x\": {}, \"y\": {}, \"z\": {}",
                x9(b.rotation.x),
                x9(b.rotation.y),
                x9(b.rotation.z)
            ),
        };
        lines.push(format!(
            "\t\t\t\"rotation\": {{ \"origin\": {}, {angles}{rescale} }}",
            vec3(b.pivot)
        ));
    }
    // TODO(R1): write `shade` from the per-element shade flag once cuboid-model has it.
    if !el.flags.contains(ElementFlags::SHADE) {
        lines.push("\t\t\t\"shade\": false".to_owned());
    }
    let faces: Vec<String> = el
        .faces
        .iter()
        .filter(|face| face.active)
        .map(|face| face_line(face, prepared))
        .collect();
    lines.push(format!(
        "\t\t\t\"faces\": {{\n{}\n\t\t\t}}",
        faces.join(",\n")
    ));
    format!("\t\t{{\n{}\n\t\t}}", lines.join(",\n"))
}

fn face_line(face: &Face, prepared: &Prepared) -> String {
    let doc = &prepared.doc;
    let mut rect = mc::uv_to_units(face.uv.tl(), face.uv.br(), mc::JSON_UV_UNITS);
    if doc.props.general.prevent_bleeding {
        // Moves every edge toward the centre by a third of a texel at most (interop 7.6).
        let size = doc
            .textures
            .entries
            .iter()
            .find(|entry| entry.label == face.texture)
            .map_or((16.0, 16.0), |entry| {
                (entry.texture.image.w as f32, entry.texture.image.h as f32)
            });
        let dx = (0.32 / size.0.max(1.0)).min(0.01);
        let dy = (0.32 / size.1.max(1.0)).min(0.01);
        let [u0, v0, u1, v1] = rect;
        let (a, b) = if u0 <= u1 { (dx, -dx) } else { (-dx, dx) };
        let (c, d) = if v0 <= v1 { (dy, -dy) } else { (-dy, dy) };
        rect = [u0 + a, v0 + c, u1 + b, v1 + d];
    }
    let mut line = format!(
        "\t\t\t\t{}: {{ \"uv\": [ {}, {}, {}, {} ], \"texture\": {}",
        string(mc::face_key(face.name)),
        x9(rect[0]),
        x9(rect[1]),
        x9(rect[2]),
        x9(rect[3]),
        string(&format!("#{}", face.texture)),
    );
    if let Some(side) = mc::cull_key(face.cull) {
        line.push_str(&format!(", \"cullface\": \"{side}\""));
    }
    let rotation = face.uv.rot().rem_euclid(360);
    if rotation > 0 {
        line.push_str(&format!(", \"rotation\": {rotation}"));
    }
    if face.tint >= 0 {
        line.push_str(&format!(", \"tintindex\": {}", face.tint));
    }
    line.push_str(" }");
    line
}

/// The display block (interop 7.7). `full` writes every slot with every member.
fn display(slots: &[DisplaySlot; 8], full: bool) -> Option<String> {
    let mut written: Vec<String> = Vec::new();
    for (slot, key) in slots.iter().zip(SLOT_KEYS) {
        if !full && !slot.enabled {
            continue;
        }
        let near = |a: Vec3, b: Vec3| a.distance_squared(b) < 1e-10;
        // A zero scale component reads as 0.0001.
        let scale = slot
            .scale
            .map(|c| if c.abs() < f32::EPSILON { 0.0001 } else { c });
        let mut members: Vec<String> = Vec::new();
        if full || !near(slot.rotation, Vec3::ZERO) {
            members.push(format!("\t\t\t\"rotation\": {}", vec3(slot.rotation)));
        }
        if full || !near(slot.translation, Vec3::ZERO) {
            members.push(format!("\t\t\t\"translation\": {}", vec3(slot.translation)));
        }
        if full || !near(scale, Vec3::ONE) {
            members.push(format!("\t\t\t\"scale\": {}", vec3(scale)));
        }
        if members.is_empty() {
            continue;
        }
        written.push(format!(
            "\t\t{}: {{\n{}\n\t\t}}",
            string(key),
            members.join(",\n")
        ));
    }
    (!written.is_empty()).then(|| format!("\t\"display\": {{\n{}\n\t}}", written.join(",\n")))
}

/// The overrides block (interop 7.8).
fn overrides(cases: &[OverrideCase]) -> String {
    let cases: Vec<String> = cases
        .iter()
        .map(|case| {
            let floats = [
                ("angle", case.angle),
                ("damage", case.damage),
                ("pull", case.pull),
                ("time", case.time),
            ];
            let ints = [
                ("blocking", case.blocking),
                ("broken", case.broken),
                ("cast", case.cast),
                ("damaged", case.damaged),
                ("lefthanded", case.lefthanded),
                ("pulling", case.pulling),
            ];
            // The keys are written in alphabetical order, as the reference does.
            let mut predicate: Vec<(&str, String)> = floats
                .iter()
                .filter_map(|(key, v)| v.map(|v| (*key, x9(v))))
                .chain(
                    ints.iter()
                        .filter_map(|(key, v)| v.map(|v| (*key, v.to_string()))),
                )
                .collect();
            predicate.sort_by_key(|(key, _)| *key);
            let members: Vec<String> = predicate
                .iter()
                .map(|(key, value)| format!("\t\t\t\t\"{key}\": {value}"))
                .collect();
            let body = if members.is_empty() {
                String::new()
            } else {
                format!("\n{}\n\t\t\t", members.join(",\n"))
            };
            format!(
                "\t\t{{\n\t\t\t\"predicate\": {{{body}}},\n\t\t\t\"model\": {}\n\t\t}}",
                string(&case.model)
            )
        })
        .collect();
    format!("\t\"overrides\": [\n{}\n\t]", cases.join(",\n"))
}
