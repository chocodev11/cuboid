#![no_main]

use std::fs;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::OnceLock;

use cuboid_fmt_bbmodel::data_url::png_url;
use cuboid_image::{Image, RgbaBuf, encode_png};
use cuboid_mcp::tools::TOOLS;
use cuboid_mcp::{Camera, Options, Picture, RenderError, Renderer, serve};
use cuboid_model::Document;
use libfuzzer_sys::fuzz_target;

/// A model with one cube whose faces show a 16 x 16 texture, so that the tools that read a model
/// or paint its texture have pixels to work on. `@URL@` is the picture as a data URL.
const MODEL: &str = r#"{"meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
"name": "fuzz", "resolution": {"width": 16, "height": 16},
"elements": [{"name": "cube", "from": [0, 0, 0], "to": [4, 4, 4], "origin": [0, 0, 0], "uuid": "e1",
"faces": {"north": {"uv": [0, 0, 4, 4], "texture": 0}, "east": {"uv": [4, 0, 8, 4], "texture": 0},
"south": {"uv": [8, 0, 12, 4], "texture": 0}, "up": {"uv": [0, 4, 4, 8], "texture": 0}}}],
"outliner": ["e1"], "textures": [{"name": "fuzz", "id": "0", "source": "@URL@"}]}"#;

/// A renderer that draws an opaque square in a clear field, so that the code that measures a
/// picture and puts pictures on a sheet runs.
struct Square;

impl Renderer for Square {
    fn render(
        &mut self,
        doc: &Document,
        _camera: Camera,
        size: u32,
    ) -> Result<Picture, RenderError> {
        let mut picture = Picture::new(size, size, [0; 4]);
        let side = if doc.elements.is_empty() { 0 } else { size / 2 };
        for y in size / 4..size / 4 + side {
            for x in size / 4..size / 4 + side {
                picture.set(x, y, [40, 100, 200, 255]);
            }
        }
        Ok(picture)
    }
}

/// The project folder, made once. The server is read only and writes nothing into it, and every
/// process writes the same model, so one folder serves them all.
fn root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let mut buf = RgbaBuf::new(16, 16);
        for texel in buf.pixels.chunks_exact_mut(4) {
            texel.copy_from_slice(&[200, 20, 20, 255]);
        }
        let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
        let dir = std::env::temp_dir().join("cuboid-fuzz-mcp");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(
            dir.join("fuzz.bbmodel"),
            MODEL.replace("@URL@", &png_url(&png)),
        );
        dir
    })
}

// The first byte picks the input. A clear bit 0 reads the rest as the stream of requests; a set bit
// 0 makes the next byte pick a tool and the rest its arguments, in a request that is valid
// otherwise, so that the fuzzer reaches the typed arguments without first guessing the envelope.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let wrapped;
    let input = if mode & 1 == 0 {
        rest
    } else {
        let Some((&pick, arguments)) = rest.split_first() else {
            return;
        };
        let tool = TOOLS[usize::from(pick) % TOOLS.len()].name;
        wrapped = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{tool}","arguments":{},"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{}}}}}}}}"#,
            String::from_utf8_lossy(arguments)
        );
        wrapped.as_bytes()
    };
    let options = Options { read_only: true };
    let _ = serve(root(), options, &mut Square, Cursor::new(input), io::sink());
});
