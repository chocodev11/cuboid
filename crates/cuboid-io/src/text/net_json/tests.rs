use super::*;
use crate::text::mc_json::minify;

/// A geometry in the shape of the `.js` export (interop 11.1).
fn geometry(compact: bool) -> String {
    let mut json = NetJson::new(compact);
    json.begin_object();
    json.name("texturewidth");
    json.int(64);
    json.name("textureheight");
    json.int(64);
    json.name("bones");
    json.begin_array();
    json.begin_object();
    json.name("name");
    json.string("Box");
    json.name("pivot");
    json.floats(&[8.0, 0.0, -8.0]);
    json.name("rotation");
    json.floats(&[0.0, 0.0, 0.0]);
    json.name("mirror");
    json.bool(false);
    json.name("neverRender");
    json.bool(false);
    json.name("cubes");
    json.begin_array();
    json.begin_object();
    json.name("origin");
    json.floats(&[0.1, 22.5, -8.0]);
    json.name("size");
    json.floats(&[16.0, 16.0, 16.0]);
    json.name("uv");
    json.floats(&[0.0, 0.0]);
    json.end_object();
    json.end_array();
    json.end_object();
    json.end_array();
    json.end_object();
    json.finish()
}

/// The entity document of the Stonehearth export (interop 27.1): nested objects, a string
/// array and an array of objects.
fn entity(compact: bool) -> String {
    let mut json = NetJson::new(compact);
    json.begin_object();
    json.name("type");
    json.string("entity");
    json.name("components");
    json.begin_object();
    json.name("model_variants");
    json.begin_object();
    json.name("default");
    json.begin_object();
    json.name("models");
    json.strings(&["file(m.qb)"]);
    json.end_object();
    json.end_object();
    json.name("region_collision_shape");
    json.begin_object();
    json.name("region");
    json.begin_array();
    json.begin_object();
    json.name("min");
    json.begin_object();
    json.name("x");
    json.float(0.0);
    json.end_object();
    json.name("max");
    json.begin_object();
    json.name("x");
    json.float(1.0);
    json.end_object();
    json.end_object();
    json.end_array();
    json.end_object();
    json.end_object();
    json.name("value_in_gold");
    json.int(10);
    json.name("buyable");
    json.bool(true);
    json.end_object();
    json.finish()
}

// ---- interop 3.3: layout ----------------------------------------------------------------

#[test]
fn the_indented_layout_uses_two_spaces_crlf_and_compact_primitive_arrays() {
    let expected = [
        "{",
        "  \"texturewidth\": 64,",
        "  \"textureheight\": 64,",
        "  \"bones\": [",
        "    {",
        "      \"name\": \"Box\",",
        "      \"pivot\": [8.0,0.0,-8.0],",
        "      \"rotation\": [0.0,0.0,0.0],",
        "      \"mirror\": false,",
        "      \"neverRender\": false,",
        "      \"cubes\": [",
        "        {",
        "          \"origin\": [0.1,22.5,-8.0],",
        "          \"size\": [16.0,16.0,16.0],",
        "          \"uv\": [0.0,0.0]",
        "        }",
        "      ]",
        "    }",
        "  ]",
        "}",
    ]
    .join("\r\n");
    assert_eq!(geometry(false), expected);
}

#[test]
fn the_compact_layout_has_no_white_space() {
    assert_eq!(
        geometry(true),
        "{\"texturewidth\":64,\"textureheight\":64,\"bones\":[{\"name\":\"Box\",\
         \"pivot\":[8.0,0.0,-8.0],\"rotation\":[0.0,0.0,0.0],\"mirror\":false,\
         \"neverRender\":false,\"cubes\":[{\"origin\":[0.1,22.5,-8.0],\
         \"size\":[16.0,16.0,16.0],\"uv\":[0.0,0.0]}]}]}"
    );
}

#[test]
fn nested_objects_and_arrays_of_objects_are_indented() {
    let expected = [
        "{",
        "  \"type\": \"entity\",",
        "  \"components\": {",
        "    \"model_variants\": {",
        "      \"default\": {",
        "        \"models\": [\"file(m.qb)\"]",
        "      }",
        "    },",
        "    \"region_collision_shape\": {",
        "      \"region\": [",
        "        {",
        "          \"min\": {",
        "            \"x\": 0.0",
        "          },",
        "          \"max\": {",
        "            \"x\": 1.0",
        "          }",
        "        }",
        "      ]",
        "    }",
        "  },",
        "  \"value_in_gold\": 10,",
        "  \"buyable\": true",
        "}",
    ]
    .join("\r\n");
    assert_eq!(entity(false), expected);
}

#[test]
fn empty_containers_stay_on_one_line() {
    for (compact, expected) in [
        (
            false,
            "{\r\n  \"a\": [],\r\n  \"b\": {},\r\n  \"c\": [\r\n    []\r\n  ]\r\n}",
        ),
        (true, "{\"a\":[],\"b\":{},\"c\":[[]]}"),
    ] {
        let mut json = NetJson::new(compact);
        json.begin_object();
        json.name("a");
        json.begin_array();
        json.end_array();
        json.name("b");
        json.begin_object();
        json.end_object();
        json.name("c");
        json.begin_array();
        json.begin_array();
        json.end_array();
        json.end_array();
        json.end_object();
        assert_eq!(json.finish(), expected);
    }
}

#[test]
fn the_zoxel_document_is_compact_with_nested_integer_arrays() {
    // Interop 25.2, with an identity string of its own.
    let mut json = NetJson::new(true);
    json.begin_object();
    json.name("creator");
    json.string("cuboid test");
    for (name, value) in [
        ("height", 16),
        ("width", 16),
        ("depth", 16),
        ("version", 1),
        ("frames", 1),
    ] {
        json.name(name);
        json.int(value);
    }
    json.name("frame1");
    json.begin_array();
    json.ints(&[0, 0, 0, -16_777_089]);
    json.ints(&[1, 2, 3, 4]);
    json.end_array();
    json.end_object();
    assert_eq!(
        json.finish(),
        "{\"creator\":\"cuboid test\",\"height\":16,\"width\":16,\"depth\":16,\"version\":1,\
         \"frames\":1,\"frame1\":[[0,0,0,-16777089],[1,2,3,4]]}"
    );
}

#[test]
fn a_primitive_array_may_stand_at_the_root_and_as_an_item() {
    let mut json = NetJson::new(false);
    json.begin_array();
    json.floats(&[1.0, 2.5]);
    json.ints(&[]);
    json.end_array();
    assert_eq!(json.finish(), "[\r\n  [1.0,2.5],\r\n  []\r\n]");
}

// ---- interop 3.1, 3.4: values -----------------------------------------------------------

#[test]
fn floats_use_the_nj_text_and_strings_are_escaped() {
    let mut json = NetJson::new(true);
    json.begin_object();
    json.name("a\"b");
    json.string("l1\nl2\t\"q\" \\ é");
    json.name("f");
    json.floats(&[0.1, 22.5, 1e-5, -0.0]);
    json.name("one");
    json.float(1.0);
    json.name("big");
    json.int(i64::from(u32::MAX));
    json.end_object();
    assert_eq!(
        json.finish(),
        "{\"a\\\"b\":\"l1\\nl2\\t\\\"q\\\" \\\\ é\",\"f\":[0.1,22.5,1E-05,0.0],\"one\":1.0,\"big\":4294967295}"
    );
}

// ---- the output is real JSON (architecture 12.6) ---------------------------------------

#[test]
fn both_layouts_parse_as_the_same_json() {
    for build in [geometry, entity] {
        let indented: serde_json::Value = serde_json::from_str(&build(false)).unwrap();
        let compact: serde_json::Value = serde_json::from_str(&build(true)).unwrap();
        assert_eq!(indented, compact);
    }
    let parsed: serde_json::Value = serde_json::from_str(&geometry(true)).unwrap();
    assert_eq!(parsed["bones"][0]["cubes"][0]["origin"][1], 22.5);
}

#[test]
fn the_compact_layout_is_the_indented_one_minified() {
    for build in [geometry, entity] {
        assert_eq!(minify(&build(false)), build(true));
    }
}
