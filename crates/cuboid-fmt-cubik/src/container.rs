//! The ZIP container: one entry `cubik` that holds the BSON payload (native 2).
//!
//! The reader accepts stored and deflated entries, data descriptors, ZIP64 records and the
//! parallel-deflate segments of DotNetZip (a valid deflate stream); it ignores every entry other
//! than `cubik` and the port's `cuboid.json`. The writer produces a correct archive with one
//! deflated entry: the payload inside is byte-identical to the original's, the container bytes
//! are not (integration backlog item 2).
//!
//! spec: native 2, arch 12.3 (Container)

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use thiserror::Error;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::Limits;

/// The name of the entry that holds the payload.
pub const ENTRY: &str = "cubik";

/// The name of the port's extension entry (native 8.4).
pub const EXTENSION_ENTRY: &str = "cuboid.json";

/// The extension entry is small; a larger one is ignored.
const MAX_EXTENSION: u64 = 16 << 20;

/// Deflate cannot expand a byte past this ratio, which bounds what a header may claim.
const MAX_DEFLATE_RATIO: u64 = 1100;

/// Why a container cannot be read. The three reasons are distinct, as native 2.4 requires.
#[derive(Debug, Error)]
pub enum ContainerError {
    /// Not a ZIP archive, or a truncated one whose central directory is gone.
    #[error("not a ZIP archive: {0}")]
    NotZip(String),
    /// A ZIP archive without an entry named `cubik`.
    #[error("the archive has no `cubik` entry")]
    NoEntry,
    /// The entry exists but cannot be read: bad data, bad checksum, encryption or an unsupported
    /// compression method.
    #[error("the `cubik` entry is corrupt: {0}")]
    Corrupt(String),
    /// The payload is larger than the limit.
    #[error("the payload is larger than the limit of {max} bytes")]
    TooLarge {
        /// The limit that applied.
        max: u64,
    },
    /// The archive could not be written.
    #[error("cannot write the archive: {0}")]
    Write(String),
}

/// What a container holds.
#[derive(Debug)]
pub struct Container {
    /// The decompressed `cubik` entry: the BSON payload.
    pub payload: Vec<u8>,
    /// The `cuboid.json` entry, when the archive has one that is not too large.
    pub extension: Option<Vec<u8>>,
}

/// Reads the `cubik` entry of an archive. The exact name is looked up first, then the same name
/// in any ASCII letter case. The decompressed size is checked against `limits` while reading,
/// not only against the size the header claims.
///
/// spec: native 2.2, 2.4
pub fn read<R: Read + Seek>(mut reader: R, limits: &Limits) -> Result<Container, ContainerError> {
    let archive_len = reader
        .seek(SeekFrom::End(0))
        .and_then(|len| reader.rewind().map(|()| len))
        .map_err(|err| ContainerError::NotZip(err.to_string()))?;
    let mut archive =
        ZipArchive::new(reader).map_err(|err| ContainerError::NotZip(err.to_string()))?;
    let index = archive
        .index_for_name(ENTRY)
        .or_else(|| {
            (0..archive.len()).find(|&i| {
                archive
                    .name_for_index(i)
                    .and_then(Result::ok)
                    .is_some_and(|name| name.eq_ignore_ascii_case(ENTRY))
            })
        })
        .ok_or(ContainerError::NoEntry)?;
    let payload = read_entry(&mut archive, index, limits.max_payload, archive_len)?;
    let extension = archive
        .index_for_name(EXTENSION_ENTRY)
        .and_then(|i| read_entry(&mut archive, i, MAX_EXTENSION, archive_len).ok());
    Ok(Container { payload, extension })
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    max: u64,
    archive_len: u64,
) -> Result<Vec<u8>, ContainerError> {
    let corrupt = |err: &dyn std::fmt::Display| ContainerError::Corrupt(err.to_string());
    let file = archive.by_index(index).map_err(|err| match err {
        ZipError::CompressionMethodNotSupported(method) => {
            ContainerError::Corrupt(format!("compression method {method} is not supported"))
        }
        other => corrupt(&other),
    })?;
    if file.size() > max {
        return Err(ContainerError::TooLarge { max });
    }
    // The header may lie about both sizes, so the reservation follows what the compressed data
    // can hold at most, and the data cannot be longer than the archive.
    let reserve = file.size().min(
        file.compressed_size()
            .min(archive_len)
            .saturating_mul(MAX_DEFLATE_RATIO)
            .saturating_add(1024),
    );
    let mut out = Vec::with_capacity(usize::try_from(reserve).unwrap_or(0));
    let read = file
        .take(max.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|err| corrupt(&err))?;
    if read as u64 > max {
        return Err(ContainerError::TooLarge { max });
    }
    Ok(out)
}

/// Writes an archive with the entry `cubik` (deflate, the fastest level) and, when given, the
/// entry `cuboid.json`.
///
/// spec: native 2.4 (writer), arch 12.3 (Container)
pub fn write(payload: &[u8], extension: Option<&[u8]>) -> Result<Vec<u8>, ContainerError> {
    let failed = |err: &dyn std::fmt::Display| ContainerError::Write(err.to_string());
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(1));
    let mut zip = ZipWriter::new(Cursor::new(Vec::with_capacity(payload.len() / 4 + 1024)));
    zip.start_file(ENTRY, options).map_err(|err| failed(&err))?;
    zip.write_all(payload).map_err(|err| failed(&err))?;
    if let Some(extension) = extension {
        zip.start_file(EXTENSION_ENTRY, options)
            .map_err(|err| failed(&err))?;
        zip.write_all(extension).map_err(|err| failed(&err))?;
    }
    let cursor = zip.finish().map_err(|err| failed(&err))?;
    Ok(cursor.into_inner())
}

impl From<ContainerError> for io::Error {
    fn from(err: ContainerError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, err)
    }
}
