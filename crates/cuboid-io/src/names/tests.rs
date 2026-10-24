use super::*;

fn p(text: &str) -> &Path {
    Path::new(text)
}

#[test]
fn the_extension_starts_at_the_last_dot_of_the_file_name() {
    let cases = [
        ("C:\\models\\m.cubik", ".cubik"),
        ("/models/m.cubik", ".cubik"),
        ("m.tar.gz", ".gz"),
        (".hidden", ".hidden"),
        ("m.CUBIK", ".CUBIK"),
        ("noext", ""),
        ("trailing.", ""),
        ("dir.d/noext", ""),
        ("C:\\dir.d\\noext", ""),
        ("D:file", ""),
        ("", ""),
    ];
    for (path, ext) in cases {
        assert_eq!(extension(p(path)), ext, "{path}");
    }
}

#[test]
fn the_stem_is_the_file_name_without_its_last_extension() {
    let cases = [
        ("C:\\models\\My Model.cubik", "My Model"),
        ("/models/m.tar.gz", "m.tar"),
        ("m.vox", "m"),
        (".hidden", ""),
        ("noext", "noext"),
        ("trailing.", "trailing."),
        ("dir.d/noext", "noext"),
        ("é 😀.json", "é 😀"),
    ];
    for (path, stem_text) in cases {
        assert_eq!(stem(p(path)), stem_text, "{path}");
    }
}

#[test]
fn siblings_use_the_folder_of_the_target_with_forward_slashes() {
    let cases = [
        ("C:\\out\\m.obj", "m.mtl", "C:/out/m.mtl"),
        ("C:/out/m.obj", "m_a.png", "C:/out/m_a.png"),
        ("/home/u/m.obj", "m.mtl", "/home/u/m.mtl"),
        ("C:\\m.obj", "m.mtl", "C:/m.mtl"),
        ("/m.obj", "m.mtl", "/m.mtl"),
        // A bare name resolves against the current folder.
        ("m.obj", "m.mtl", "m.mtl"),
    ];
    for (target, name, expected) in cases {
        assert_eq!(
            sibling(p(target), name),
            PathBuf::from(expected),
            "{target}"
        );
    }
}

#[test]
fn png_files_get_the_extension_unless_they_have_it_in_any_case() {
    let cases = [
        ("out\\tex", "out\\tex.png"),
        ("tex.png", "tex.png"),
        ("tex.PNG", "tex.PNG"),
        ("tex.Png", "tex.Png"),
        ("tex.jpg", "tex.jpg.png"),
        ("png", "png.png"),
        ("", ".png"),
    ];
    for (path, expected) in cases {
        assert_eq!(png_path(p(path)), PathBuf::from(expected), "{path}");
    }
}
