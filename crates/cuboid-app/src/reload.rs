//! Reading a texture again from the file it came from, on the I/O thread.

use std::path::Path;

use cuboid_io::texture_files::{DiskFiles, TextureFiles};
use cuboid_model::image::{ImageFileKind, decode_file, signature};

use crate::jobs::{JobError, JobHandle};

/// A texture that is being read again.
pub struct Reload {
    pub label: String,
    /// The document the texture belongs to; the result for another one is dropped.
    pub doc_id: u64,
    pub job: JobHandle<Reloaded>,
}

/// A texture read again, ready for `Command::ReplaceTexture`.
pub struct Reloaded {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The signature of the file, as the texture records it.
    pub signature: String,
    pub file_size: u64,
}

/// Reads and decodes the file of the texture `label`. The file goes through the reader the
/// importers use, which refuses network and device paths, anything but a regular file and a
/// file over its size limit before it allocates; the decoders check the size of the picture.
pub fn read(label: String, path: &Path, code_page: u16) -> Result<Reloaded, JobError> {
    let bytes = DiskFiles
        .read(path)
        .map_err(|why| JobError::Failed(format!("the file is not read: {why}")))?
        .ok_or(JobError::NotFound)?;
    let extension = path.extension().map(|ext| ext.to_string_lossy());
    let kind = ImageFileKind::from_extension(extension.as_deref().unwrap_or_default());
    let image = decode_file(&bytes, kind).map_err(|why| JobError::Failed(why.to_string()))?;
    Ok(Reloaded {
        label,
        width: image.w,
        height: image.h,
        rgba: image.rgba().to_vec(),
        signature: signature(&bytes, code_page),
        file_size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use cuboid_model::image::{Image, RgbaBuf, encode_png};

    use super::*;

    #[test]
    fn a_picture_is_read_with_its_signature_and_length() {
        let dir = tempfile::tempdir().unwrap();
        let mut buf = RgbaBuf::new(3, 2);
        buf.set(1, 0, [9, 8, 7, 255]);
        let bytes = encode_png(&Image::from_rgba(buf)).unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, &bytes).unwrap();

        let read = read("a".to_owned(), &path, 1252).unwrap();
        assert_eq!((read.width, read.height), (3, 2));
        assert_eq!(read.rgba.len(), 3 * 2 * 4);
        assert_eq!(&read.rgba[4..8], &[9, 8, 7, 255]);
        assert_eq!(read.file_size, bytes.len() as u64);
        assert_eq!(read.signature, signature(&bytes, 1252));
    }

    #[test]
    fn a_missing_file_and_a_file_that_is_no_picture_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.png");
        assert_eq!(
            read("a".to_owned(), &gone, 1252).err(),
            Some(JobError::NotFound)
        );
        assert_eq!(
            read("a".to_owned(), dir.path(), 1252).err(),
            Some(JobError::NotFound),
            "a folder holds no texture"
        );
        let text = dir.path().join("text.png");
        std::fs::write(&text, b"not a picture").unwrap();
        assert!(matches!(
            read("a".to_owned(), &text, 1252),
            Err(JobError::Failed(_))
        ));
        let device = Path::new("//./pipe/anything.png");
        assert!(matches!(
            read("a".to_owned(), device, 1252),
            Err(JobError::Failed(_))
        ));
    }
}
