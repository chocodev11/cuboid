//! The outline and the loader on a model made in the test: a group, a rotated element, faces
//! with rectangles and turns, a texture and a display slot.

use std::fmt::Write as _;
use std::fs;

use cuboid_image::{Image, RgbaBuf, encode_png};
use cuboid_library::doc::{LoadError, load};
use cuboid_library::{OUTLINE_PAGE, outline};
use cuboid_model::ObjRef;

/// A 16 x 16 picture of one colour as the data URL a bbmodel holds.
fn texture_url() -> String {
    let mut buf = RgbaBuf::new(16, 16);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&[200, 20, 20, 255]);
    }
    let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
    cuboid_fmt_bbmodel::data_url::png_url(&png)
}

/// A lamp: a base, an arm in a group turned by 22.5 degrees, and a shade, with a 16 x 16 texture.
fn lamp() -> String {
    let url = texture_url();
    format!(
        r#"{{"meta": {{"format_version": "4.10", "model_format": "java_block", "box_uv": false}},
        "name": "lamp", "resolution": {{"width": 16, "height": 16}},
        "elements": [
            {{"name": "base", "from": [4, 0, 4], "to": [12, 2, 12], "origin": [8, 0, 8],
              "uuid": "e1", "faces": {{
                "north": {{"uv": [0, 0, 8, 2], "texture": 0}},
                "up": {{"uv": [0, 0, 8, 8], "texture": 0, "rotation": 90}}}}}},
            {{"name": "arm_left", "from": [7, 2, 7], "to": [9, 10, 9], "origin": [8, 2, 8],
              "rotation": [0, 0, 22.5], "uuid": "e2",
              "faces": {{"east": {{"uv": [0, 0, 2, 8], "texture": 0}}}}}},
            {{"name": "shade", "from": [5, 10, 5], "to": [11, 11, 11], "origin": [8, 10, 8],
              "uuid": "e3", "faces": {{"down": {{"uv": [2, 2, 8, 8], "texture": 0}}}}}}],
        "outliner": ["e1", {{"name": "lamp_arm", "origin": [8, 2, 8], "uuid": "g1",
            "children": ["e2", "e3"]}}],
        "textures": [{{"name": "lamp.png", "id": "0", "source": "{url}"}}],
        "display": {{"gui": {{"rotation": [30, 225, 0], "translation": [0, 1.5, 0],
            "scale": [0.8, 0.8, 0.8]}}}}}}"#
    )
}

/// The outline of the lamp. The corners, rotation and pivot are the numbers the project gave:
/// the groups and the mirrored z of the document are undone.
const LAMP_OUTLINE: &str = concat!(
    r#"{"format":"cuboid-outline","version":1,"total":3,"offset":0,"#,
    r#""groups":[{"path":"lamp_arm","pivot":[8,2,8],"children":["arm_left","shade"]}],"#,
    r#""elements":["#,
    r#"{"number":1,"path":"base","from":[4,0,4],"to":[12,2,12],"faces":{"#,
    r#""up":["0",0,0,8,8,90],"north":["0",0,0,8,2]}},"#,
    r#"{"number":2,"path":"lamp_arm/arm_left","from":[7,2,7],"to":[9,10,9],"#,
    r#""rotation":[0,0,22.5],"pivot":[8,2,8],"faces":{"east":["0",0,0,2,8]}},"#,
    r#"{"number":3,"path":"lamp_arm/shade","from":[5,10,5],"to":[11,11,11],"#,
    r#""faces":{"down":["0",2,2,8,8]}}],"#,
    r#""textures":[{"name":"0","size":[16,16],"path":"lamp"}],"#,
    r#""display":[{"slot":"gui","rotation":[30,225,0],"translation":[0,1.5,0],"#,
    r#""scale":[0.8,0.8,0.8]}]}"#
);

#[test]
fn the_outline_of_a_model_is_in_model_space_and_rounded() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("lamp.bbmodel");
    fs::write(&path, lamp()).expect("write");
    let opened = load(&path).expect("loads");
    let text = serde_json::to_string(&outline(&opened.doc)).expect("encodes");
    assert_eq!(text, LAMP_OUTLINE);
}

#[test]
fn a_page_of_the_outline_keeps_the_tree_and_the_textures_on_the_first_page() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("lamp.bbmodel");
    fs::write(&path, lamp()).expect("write");
    let doc = load(&path).expect("loads").doc;
    let first = outline(&doc).page(0, 2);
    assert_eq!((first.elements.len(), first.next), (2, Some(2)));
    assert_eq!(
        (
            first.groups.len(),
            first.textures.len(),
            first.display.len()
        ),
        (1, 1, 1)
    );
    let last = outline(&doc).page(2, 2);
    assert_eq!((last.offset, last.next, last.total), (2, None, 3));
    assert_eq!(last.elements[0].path, "lamp_arm/shade");
    assert!(last.groups.is_empty() && last.textures.is_empty() && last.display.is_empty());
}

#[test]
fn a_large_model_is_cut_into_pages_of_24_elements() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("many.json");
    let element = r#"{"from": [0, 0, 0], "to": [1, 1, 1]}"#;
    let text = format!(r#"{{"elements": [{}]}}"#, vec![element; 50].join(","));
    fs::write(&path, text).expect("write");
    let doc = load(&path).expect("loads").doc;
    let whole = outline(&doc);
    assert_eq!((whole.total, OUTLINE_PAGE), (50, 24));
    let first = whole.clone().page(0, OUTLINE_PAGE);
    assert_eq!((first.elements.len(), first.next), (24, Some(24)));
    let last = whole.page(48, OUTLINE_PAGE);
    assert_eq!((last.elements.len(), last.next), (2, None));
    assert_eq!(last.elements[0].number, 49);
}

/// `count` elements in four groups, each with six textured faces of fractional numbers, a
/// rotation on two axes and a long name: the worst case for the size of a page.
fn busy(count: usize) -> String {
    let face = r#"{"uv": [1.5, 2.25, 4.5, 7.75], "texture": 0}"#;
    let mut elements = Vec::new();
    for n in 0..count {
        let mut text = String::new();
        let _ = write!(
            text,
            r#"{{"name": "part_{n}_upper_left", "from": [1.25, 2.5, 3.75], "to": [9.5, 10.25, 11.125],
            "origin": [8, 8, 8], "rotation": [22.5, 0, 45], "uuid": "e{n}", "faces": {{"#
        );
        let faces: Vec<String> = ["north", "east", "south", "west", "up", "down"]
            .iter()
            .map(|name| format!(r#""{name}": {face}"#))
            .collect();
        text.push_str(&faces.join(","));
        text.push_str("}}");
        elements.push(text);
    }
    let groups: Vec<String> = (0..4)
        .map(|g| {
            let children: Vec<String> = (g..count)
                .step_by(4)
                .map(|n| format!(r#""e{n}""#))
                .collect();
            format!(
                r#"{{"name": "group_{g}", "origin": [8, 8, 8], "uuid": "g{g}", "children": [{}]}}"#,
                children.join(",")
            )
        })
        .collect();
    format!(
        r#"{{"meta": {{"format_version": "4.10", "model_format": "free", "box_uv": false}},
        "name": "busy", "resolution": {{"width": 16, "height": 16}},
        "elements": [{}], "outliner": [{}],
        "textures": [{{"name": "lamp_wood.png", "id": "lamp_wood", "source": "{}"}}]}}"#,
        elements.join(","),
        groups.join(","),
        texture_url()
    )
}

#[test]
fn a_page_of_the_worst_elements_stays_under_the_result_budget() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("busy.bbmodel");
    fs::write(&path, busy(OUTLINE_PAGE * 2)).expect("write");
    let doc = load(&path).expect("loads").doc;
    let page = outline(&doc).page(0, OUTLINE_PAGE);
    assert_eq!(
        (page.elements.len(), page.next),
        (OUTLINE_PAGE, Some(OUTLINE_PAGE))
    );
    let busiest = &page.elements[OUTLINE_PAGE - 1];
    assert_eq!(busiest.faces.0.len(), 6);
    assert!(busiest.rotation.is_some() && busiest.path.contains('/'));
    // The first page also holds the groups, the texture and the display slots.
    let first = serde_json::to_string(&page).expect("encodes");
    let later =
        serde_json::to_string(&outline(&doc).page(OUTLINE_PAGE, OUTLINE_PAGE)).expect("encodes");
    // About 10.6 KB the first page and 9.3 KB a later one. 12 KB are 4,000 tokens at 3 bytes a
    // token and 6,000 at 2: under the 8,000 of the results.
    assert!(first.len() < 12_000, "{} bytes", first.len());
    assert!(later.len() < 12_000, "{} bytes", later.len());
}

#[test]
fn outline_items_name_their_nodes_in_the_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("lamp.bbmodel");
    fs::write(&path, lamp()).expect("write");
    let doc = load(&path).expect("loads").doc;
    let whole = outline(&doc);
    for element in &whole.elements {
        let ObjRef::Element(id) = element.node else {
            panic!("{element:?}");
        };
        assert_eq!(doc.elements.display_id(id), Some(element.number));
    }
    let ObjRef::Group(id) = whole.groups[0].node else {
        panic!("{:?}", whole.groups[0]);
    };
    assert_eq!(doc.groups.get(id).map(|g| &*g.name), Some("lamp_arm"));
}

#[test]
fn an_unknown_extension_is_refused_and_a_missing_file_is_named() {
    let dir = tempfile::tempdir().expect("temp dir");
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "x").expect("write");
    assert!(matches!(load(&notes), Err(LoadError::Unsupported)));
    let missing = dir.path().join("none.cubik");
    let Err(LoadError::Failed(why)) = load(&missing) else {
        panic!("a missing file is a failure");
    };
    assert!(why.contains("none.cubik"), "{why}");
}
