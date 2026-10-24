//! The data types of this crate: revision stamps, the stored and decoded pixel containers,
//! `Image`, file kinds and errors. The algorithms live in the other modules.
//!
//! The file keeps the name it had as the W0.2 contract: the parity ledger of that package names
//! three of its tests.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use thiserror::Error;

use crate::alpha::AlphaStats;
use crate::format;

/// The largest width or height of a texture, the limit of the original's engine.
///
/// spec: tools 5.2 (device texture size)
pub const MAX_SIDE: u32 = 16_384;

/// The largest number of texels of a texture, 8192 x 8192 or 16384 x 4096: 256 MiB as RGBA8.
/// The file decoders read the size from the header and refuse a larger one before they allocate
/// a pixel buffer, and they accept every size [`check_size`] accepts, so an image the port saved
/// can be read back.
pub const MAX_TEXELS: u64 = 1 << 26;

/// Checks a size read from a file or a record against [`MAX_SIDE`] and [`MAX_TEXELS`]. Readers
/// call it before they allocate or build an [`Image`].
pub fn check_size(w: u32, h: u32) -> Result<(), ImageError> {
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_TEXELS {
        return Err(ImageError::TooLarge { w, h });
    }
    Ok(())
}

/// A revision stamp drawn from a process-wide counter, so two stamps are never equal unless one
/// was copied from the other. Caches key on `(object, Rev)`; a restored object keeps its old
/// stamp and an edited one gets a fresh stamp.
///
/// `cuboid-voxel` has a twin of this type because both crates sit on layer L1 and share only
/// `cuboid-num`, which W0.3 owns. `cuboid-model` re-exports this one as `cuboid_model::Rev`.
///
/// spec: arch 6 (`Rev`), arch 8.2
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Rev(u64);

impl Rev {
    /// The stamp of "never"; no live object has it.
    pub const NONE: Rev = Rev(0);

    /// Draws a fresh stamp.
    pub fn next() -> Rev {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Rev(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw counter value, for `FrameKey` hashing and logs.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Pixel data exactly as read from a `.cubik` texture record: format code plus raw bytes
/// (mip chain included). Kept untouched until the image is edited, so a re-save is byte-identical.
///
/// An image that was created or edited in memory holds an empty `raw`; writers then encode
/// [`Image::rgba`] as RGBA32 (native 4.5.1 writer rule).
///
/// spec: native 4.5.1, arch 8.3
#[derive(Clone)]
pub struct StoredPixels {
    /// A code of [`mod@format`]; unknown codes are kept as read.
    pub format: i32,
    /// Level 0 first, rows bottom to top.
    pub raw: Arc<[u8]>,
}

/// Prints the format and the byte count, not the bytes: documents hold megabytes of them.
impl fmt::Debug for StoredPixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredPixels")
            .field("format", &self.format)
            .field("bytes", &self.raw.len())
            .finish()
    }
}

impl StoredPixels {
    /// The state of an image that has no retained bytes: RGBA32 and empty.
    pub fn rgba_in_memory() -> Self {
        Self {
            format: format::RGBA32,
            raw: Arc::from([]),
        }
    }
}

/// An 8-bit RGBA buffer with straight alpha. Rows run bottom to top, the original's image
/// orientation; flipping happens only at PNG and GPU edges.
///
/// spec: arch 2 (principle 5), native 4.5.1
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RgbaBuf {
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
    /// `w * h * 4` bytes, R, G, B, A.
    pub pixels: Vec<u8>,
}

impl RgbaBuf {
    /// A transparent black buffer. The size is within [`check_size`].
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            pixels: vec![0; w as usize * h as usize * 4],
        }
    }

    /// The byte offset of a texel, or `None` outside the buffer.
    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        if x >= self.w || y >= self.h {
            return None;
        }
        Some((y as usize * self.w as usize + x as usize) * 4)
    }

    /// The colour of a texel; `None` outside the buffer.
    pub fn get(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let at = self.offset(x, y)?;
        self.pixels.get(at..)?.first_chunk().copied()
    }

    /// Sets a texel without blending; returns `false` and changes nothing outside the buffer.
    pub fn set(&mut self, x: i32, y: i32, color: [u8; 4]) -> bool {
        let Some(at) = self.offset(x, y) else {
            return false;
        };
        let Some(texel) = self.pixels.get_mut(at..).and_then(|s| s.first_chunk_mut()) else {
            return false;
        };
        *texel = color;
        true
    }

    /// Copies `src` with its lower-left texel to `(x, y)`, replacing the texels below it. What
    /// lies outside this buffer is cut off.
    pub fn blit(&mut self, src: &RgbaBuf, x: u32, y: u32) {
        if self.w == 0 || src.w == 0 {
            return;
        }
        let width = src.w.min(self.w.saturating_sub(x)) as usize * 4;
        let start = x as usize * 4;
        let dst_rows = self.pixels.chunks_exact_mut(self.w as usize * 4);
        let src_rows = src.pixels.chunks_exact(src.w as usize * 4);
        for (dst, src) in dst_rows.skip(y as usize).zip(src_rows) {
            if let (Some(dst), Some(src)) = (dst.get_mut(start..start + width), src.get(..width)) {
                dst.copy_from_slice(src);
            }
        }
    }

    /// A magenta and black checkerboard of 4-texel cells, the picture shown in place of pixel
    /// data that cannot be decoded.
    fn placeholder(w: u32, h: u32) -> Self {
        let (w, h) = (w.min(MAX_SIDE), h.min(MAX_SIDE));
        let mut buf = Self::new(w, h);
        for (i, texel) in buf.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (i % w as usize, i / w as usize);
            *texel = if (x / 4 + y / 4) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
        }
        buf
    }
}

/// A decoded or stored texture image.
///
/// Level 0 is decoded to RGBA8 on first use, so loading many textures decodes only those the
/// atlas needs. [`Image::edit`] switches to an in-memory RGBA32 buffer and drops the retained
/// bytes.
///
/// `w` and `h` are as read from the record and are kept for the re-save; a reader checks them
/// with [`check_size`] first. The fields are public for reading: replace the image instead of
/// changing `w`, `h` or `stored` on a live one, because the decoded pixels and the statistics are
/// cached. The image carries no name; the texture record of the model holds it (native 4.5.1).
///
/// spec: native 4.5.1, arch 6 (cuboid-image), arch 8.3
#[derive(Clone)]
pub struct Image {
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
    /// The bytes as read, or empty after [`Image::edit`] and for in-memory images.
    pub stored: StoredPixels,
    /// Changes with every edit; caches of uploads and alpha statistics key on it.
    pub rev: Rev,
    decoded: OnceLock<RgbaBuf>,
    edited: Option<RgbaBuf>,
    alpha: OnceLock<AlphaStats>,
}

/// Prints the size, the stored format and the stamp, not the pixels.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("w", &self.w)
            .field("h", &self.h)
            .field("stored", &self.stored)
            .field("rev", &self.rev)
            .field("edited", &self.edited.is_some())
            .finish()
    }
}

impl Image {
    /// An image over bytes as read from a file; nothing is decoded yet.
    pub fn from_stored(w: u32, h: u32, stored: StoredPixels) -> Self {
        Self {
            w,
            h,
            stored,
            rev: Rev::next(),
            decoded: OnceLock::new(),
            edited: None,
            alpha: OnceLock::new(),
        }
    }

    /// An in-memory image over a decoded buffer.
    pub fn from_rgba(buf: RgbaBuf) -> Self {
        Self {
            w: buf.w,
            h: buf.h,
            stored: StoredPixels::rgba_in_memory(),
            rev: Rev::next(),
            decoded: OnceLock::new(),
            edited: Some(buf),
            alpha: OnceLock::new(),
        }
    }

    /// Level 0 as RGBA8, rows bottom to top, decoded on first use. Data that cannot be decoded
    /// (unknown format code, short data) reads as a placeholder image of the same size, and the
    /// stored bytes stay as read.
    ///
    /// spec: native 4.5.1
    pub fn rgba(&self) -> &[u8] {
        if let Some(buf) = &self.edited {
            return &buf.pixels;
        }
        &self
            .decoded
            .get_or_init(|| {
                format::decode(self.stored.format, self.w, self.h, &self.stored.raw).unwrap_or_else(
                    |err| {
                        log::warn!(
                            "texture of {} x {} texels, format {}: {err}; showing a placeholder",
                            self.w,
                            self.h,
                            self.stored.format
                        );
                        RgbaBuf::placeholder(self.w, self.h)
                    },
                )
            })
            .pixels
    }

    /// Alpha statistics of level 0, built on first use.
    pub fn alpha(&self) -> &AlphaStats {
        self.alpha
            .get_or_init(|| AlphaStats::of(self.w, self.h, self.rgba()))
    }

    /// The editable RGBA buffer. The image becomes RGBA32: retained bytes are dropped, the
    /// statistics are discarded and [`Image::rev`] changes. Edit the texels in place; an image of
    /// another size is a new [`Image::from_rgba`].
    pub fn edit(&mut self) -> &mut RgbaBuf {
        if self.edited.is_none() {
            self.rgba();
            self.edited = self.decoded.take();
        }
        self.stored = StoredPixels::rgba_in_memory();
        self.alpha = OnceLock::new();
        self.rev = Rev::next();
        self.edited.get_or_insert_default()
    }

    /// The format code and bytes a writer stores for this image: the bytes as read while the
    /// image is unedited, so a re-save is byte-identical, else level 0 as RGBA32 without a mip
    /// chain.
    ///
    /// spec: native 4.5.1 (writer rules)
    pub fn stored_bytes(&self) -> (i32, &[u8]) {
        match &self.edited {
            Some(buf) => (format::RGBA32, &buf.pixels),
            None => (self.stored.format, &self.stored.raw),
        }
    }
}

/// How a texture file is decoded, chosen by the extension (case-insensitive).
///
/// spec: tools 5.10
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ImageFileKind {
    /// `.dds`: DXT1 or DXT5 blocks behind a 128-byte header, shown bottom to top.
    Dds,
    /// `.tga`: raw or RLE true colour, 24 or 32 bits.
    Tga,
    /// Any other extension (PNG, JPG, ...): the general decoder.
    Other,
}

impl ImageFileKind {
    /// The kind for an extension without its dot.
    pub fn from_extension(ext: &str) -> Self {
        if ext.eq_ignore_ascii_case("dds") {
            Self::Dds
        } else if ext.eq_ignore_ascii_case("tga") {
            Self::Tga
        } else {
            Self::Other
        }
    }
}

/// Errors of the image codecs.
///
/// spec: arch 16
#[derive(Debug, Error)]
pub enum ImageError {
    /// The format code is not one of [`mod@format`].
    #[error("pixel format code {0} is not supported")]
    UnknownFormat(i32),
    /// The data ends before level 0 does (the engine's "not enough data provided").
    #[error("{got} bytes of pixel data where {needed} are needed")]
    ShortData {
        /// Bytes of level 0.
        needed: usize,
        /// Bytes given.
        got: usize,
    },
    /// The size is beyond [`MAX_SIDE`] or overflows.
    #[error("an image of {w} x {h} texels is too large")]
    TooLarge {
        /// Width in texels.
        w: u32,
        /// Height in texels.
        h: u32,
    },
    /// The rectangles do not fit into one atlas of at most this side.
    #[error("the rectangles do not fit into an atlas of {0} x {0} texels")]
    DoesNotFit(u32),
    /// The file bytes are not a valid image of the expected kind.
    #[error("cannot decode the image: {0}")]
    Decode(String),
    /// The image cannot be encoded.
    #[error("cannot encode the image: {0}")]
    Encode(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature;
    use cuboid_num::{EngineRng, trunc_i32};

    fn stored(format: i32, raw: &[u8]) -> StoredPixels {
        StoredPixels {
            format,
            raw: Arc::from(raw),
        }
    }

    #[test]
    fn the_decoder_is_chosen_by_extension_ignoring_case() {
        assert_eq!(ImageFileKind::from_extension("DDS"), ImageFileKind::Dds);
        assert_eq!(ImageFileKind::from_extension("tga"), ImageFileKind::Tga);
        assert_eq!(ImageFileKind::from_extension("png"), ImageFileKind::Other);
        assert_eq!(ImageFileKind::from_extension("bmp"), ImageFileKind::Other);
    }

    #[test]
    fn revision_stamps_increase_and_never_equal_none() {
        let a = Rev::next();
        let b = Rev::next();
        assert!(b > a);
        assert!(a > Rev::NONE);
    }

    #[test]
    fn the_signature_has_one_character_per_digest_byte_in_a_single_byte_code_page() {
        assert_eq!(signature(b"", 1252).chars().count(), 16);
        assert_eq!(signature(b"abc", 1258), signature(b"abc", 1258));
        assert_ne!(signature(b"abc", 1252), signature(b"abd", 1252));
    }

    #[test]
    fn an_unedited_image_decodes_lazily_and_keeps_its_bytes() {
        // 2 x 2 RGB24, bottom row first: red, green / blue, white.
        let raw = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let img = Image::from_stored(2, 2, stored(format::RGB24, &raw));
        assert!(img.decoded.get().is_none());
        assert_eq!(
            img.rgba(),
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
        assert!(img.decoded.get().is_some());
        assert_eq!(img.stored_bytes(), (format::RGB24, raw.as_slice()));
    }

    #[test]
    fn editing_drops_the_retained_bytes_and_changes_the_stamp() {
        let raw = [10, 20, 30];
        let mut img = Image::from_stored(1, 1, stored(format::RGB24, &raw));
        let before = img.rev;
        let buf = img.edit();
        assert_eq!(buf.pixels, [10, 20, 30, 255]);
        buf.pixels[0] = 99;
        assert_ne!(img.rev, before);
        assert_eq!(img.rgba(), [99, 20, 30, 255]);
        assert_eq!(
            img.stored_bytes(),
            (format::RGBA32, [99, 20, 30, 255].as_slice())
        );
        assert!(img.stored.raw.is_empty());
    }

    #[test]
    fn undecodable_data_reads_as_a_placeholder_of_the_same_size_and_keeps_the_bytes() {
        let unknown = Image::from_stored(8, 4, stored(9, &[1, 2, 3]));
        assert_eq!(unknown.rgba().len(), 8 * 4 * 4);
        assert_eq!(unknown.stored_bytes(), (9, [1, 2, 3].as_slice()));
        let short = Image::from_stored(4, 4, stored(format::RGBA32, &[0; 10]));
        assert_eq!(short.rgba().len(), 4 * 4 * 4);
        assert_eq!(short.stored_bytes().1.len(), 10);
    }

    /// Whatever the code, size and bytes of a record, the image reads as `w * h` texels.
    #[test]
    fn stored_pixels_of_any_shape_read_as_w_by_h_texels() {
        let mut rng = EngineRng::seeded(2);
        let mut below = |n: i32| trunc_i32(rng.unit_f32() * n as f32);
        for _ in 0..400 {
            let code = below(18);
            let (w, h) = (
                u32::try_from(below(21)).unwrap(),
                u32::try_from(below(21)).unwrap(),
            );
            let needed = format::level_len(code, w, h).unwrap_or(64);
            let len = usize::try_from(below(i32::try_from(needed).unwrap() * 2 + 2)).unwrap();
            let raw: Vec<u8> = (0..len)
                .map(|_| u8::try_from(below(256)).unwrap())
                .collect();
            let img = Image::from_stored(w, h, stored(code, &raw));
            assert_eq!(
                img.rgba().len(),
                w as usize * h as usize * 4,
                "code {code} {w}x{h}"
            );
            assert_eq!(img.stored_bytes(), (code, raw.as_slice()));
            // The statistics of a decoded or placeholder image are built without trouble.
            let _ = img.alpha().any_partial(0, 0, w, h);
        }
    }

    #[test]
    fn a_cloned_image_decodes_independently() {
        let img = Image::from_stored(1, 1, stored(format::ALPHA8, &[7]));
        let mut copy = img.clone();
        copy.edit().pixels[3] = 0;
        assert_eq!(img.rgba(), [255, 255, 255, 7]);
        assert_eq!(copy.rgba(), [255, 255, 255, 0]);
    }

    #[test]
    fn sizes_are_limited_by_side_and_by_area() {
        assert!(check_size(16_384, 4_096).is_ok());
        assert!(check_size(8_192, 8_192).is_ok());
        assert!(check_size(0, 0).is_ok());
        assert!(matches!(
            check_size(16_385, 1),
            Err(ImageError::TooLarge { w: 16_385, h: 1 })
        ));
        assert!(matches!(
            check_size(8_193, 8_192),
            Err(ImageError::TooLarge { .. })
        ));
        assert!(matches!(
            check_size(u32::MAX, u32::MAX),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn an_image_is_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<Image>();
        shareable::<AlphaStats>();
    }

    #[test]
    fn texels_outside_the_buffer_are_ignored() {
        let mut buf = RgbaBuf::new(2, 2);
        assert!(buf.set(1, 1, [1, 2, 3, 4]));
        assert!(!buf.set(2, 0, [9; 4]));
        assert!(!buf.set(0, -1, [9; 4]));
        assert_eq!(buf.get(1, 1), Some([1, 2, 3, 4]));
        assert_eq!(buf.get(2, 1), None);
        assert_eq!(buf.get(-1, 0), None);
    }

    #[test]
    fn blit_copies_rows_and_cuts_off_what_does_not_fit() {
        let mut src = RgbaBuf::new(2, 2);
        for (i, texel) in src.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *texel = [u8::try_from(i + 1).unwrap(); 4];
        }
        let mut dst = RgbaBuf::new(3, 3);
        dst.blit(&src, 2, 1);
        assert_eq!(dst.get(2, 1), Some([1; 4]));
        assert_eq!(dst.get(2, 2), Some([3; 4]));
        // The second column falls outside; nothing wrapped to the next row.
        assert_eq!(dst.get(0, 2), Some([0; 4]));
        assert_eq!(dst.get(0, 1), Some([0; 4]));
    }
}
