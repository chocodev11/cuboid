//! The logo as the app embeds it: square PNG rasters that `cargo xtask brand` renders from the SVG
//! masters in `brand/` (docs/design/logo.md 3). This crate keeps the bytes and the size table; the
//! app decodes them.

/// One raster of the logo: straight RGBA, `size` pixels wide and high.
#[derive(Copy, Clone, Debug)]
pub struct Raster {
    /// Width and height in pixels.
    pub size: u32,
    /// The PNG file.
    pub png: &'static [u8],
}

/// Every raster, smallest first. A small size is drawn from a master of its own, so the rasters
/// are not scaled copies of each other.
pub const LOGO: [Raster; 10] = [
    Raster {
        size: 16,
        png: include_bytes!("../brand/png/logo-16.png"),
    },
    Raster {
        size: 20,
        png: include_bytes!("../brand/png/logo-20.png"),
    },
    Raster {
        size: 24,
        png: include_bytes!("../brand/png/logo-24.png"),
    },
    Raster {
        size: 32,
        png: include_bytes!("../brand/png/logo-32.png"),
    },
    Raster {
        size: 40,
        png: include_bytes!("../brand/png/logo-40.png"),
    },
    Raster {
        size: 48,
        png: include_bytes!("../brand/png/logo-48.png"),
    },
    Raster {
        size: 64,
        png: include_bytes!("../brand/png/logo-64.png"),
    },
    Raster {
        size: 96,
        png: include_bytes!("../brand/png/logo-96.png"),
    },
    Raster {
        size: 128,
        png: include_bytes!("../brand/png/logo-128.png"),
    },
    Raster {
        size: 256,
        png: include_bytes!("../brand/png/logo-256.png"),
    },
];

/// The largest raster the interface draws; the 256 pixel one is for the icon files.
pub const UI_LARGEST: u32 = 128;

/// The raster of `size` pixels.
pub fn raster(size: u32) -> Option<&'static Raster> {
    LOGO.iter().find(|raster| raster.size == size)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    /// The size, bit depth and colour type in the IHDR chunk of a PNG file; nothing is decoded.
    fn header(png: &[u8]) -> Option<(u32, u32, u8, u8)> {
        let rest = png.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
        let (_length, rest) = rest.split_first_chunk::<4>()?;
        let rest = rest.strip_prefix(b"IHDR")?;
        let (width, rest) = rest.split_first_chunk::<4>()?;
        let (height, rest) = rest.split_first_chunk::<4>()?;
        let [depth, colour, ..] = *rest else {
            return None;
        };
        Some((
            u32::from_be_bytes(*width),
            u32::from_be_bytes(*height),
            depth,
            colour,
        ))
    }

    /// PNG colour type 6: red, green, blue and alpha.
    const RGBA: u8 = 6;

    #[test]
    fn every_embedded_raster_is_a_square_rgba_png_of_its_size() {
        for raster in LOGO {
            let size = raster.size;
            assert_eq!(
                header(raster.png),
                Some((size, size, 8, RGBA)),
                "logo-{size}.png"
            );
        }
    }

    #[test]
    fn the_png_files_in_the_folder_have_the_size_their_name_says() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("brand/png");
        let mut sizes = Vec::new();
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            let size: u32 = name
                .strip_prefix("logo-")
                .and_then(|rest| rest.strip_suffix(".png"))
                .and_then(|digits| digits.parse().ok())
                .unwrap_or_else(|| panic!("{name} is not named logo-<size>.png"));
            let png = fs::read(&path).unwrap();
            assert_eq!(header(&png), Some((size, size, 8, RGBA)), "{name}");
            sizes.push(size);
        }
        sizes.sort_unstable();
        let table: Vec<u32> = LOGO.iter().map(|raster| raster.size).collect();
        assert_eq!(sizes, table, "the table and the folder list the same sizes");
    }

    #[test]
    fn the_table_runs_from_small_to_large_and_the_interface_set_ends_at_128() {
        assert!(LOGO.windows(2).all(|pair| pair[0].size < pair[1].size));
        let ui: Vec<u32> = LOGO
            .iter()
            .map(|raster| raster.size)
            .filter(|size| *size <= UI_LARGEST)
            .collect();
        assert_eq!(ui, [16, 20, 24, 32, 40, 48, 64, 96, 128]);
        assert!(raster(64).is_some() && raster(65).is_none());
    }
}
