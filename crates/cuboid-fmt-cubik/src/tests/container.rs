//! Container tests: stored and deflated entries, data descriptors, ZIP64, extra entries, entry
//! name case, the three distinct errors, DotNetZip's parallel segments (native 2, 10.1).

use std::io::{Cursor, Write};

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::Limits;
use crate::container::{self, ContainerError};

struct Entry<'a> {
    name: &'a str,
    method: u16,
    data: Vec<u8>,
    crc: u32,
    size: u32,
    /// The sizes and the checksum follow the data (general purpose flag bit 3).
    descriptor: bool,
}

fn crc(data: &[u8]) -> u32 {
    let mut crc = Crc::new();
    crc.update(data);
    crc.sum()
}

fn stored<'a>(name: &'a str, payload: &[u8]) -> Entry<'a> {
    Entry {
        name,
        method: 0,
        data: payload.to_vec(),
        crc: crc(payload),
        size: u32::try_from(payload.len()).unwrap(),
        descriptor: false,
    }
}

fn deflated<'a>(name: &'a str, payload: &[u8]) -> Entry<'a> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(1));
    encoder.write_all(payload).unwrap();
    Entry {
        method: 8,
        data: encoder.finish().unwrap(),
        ..stored(name, payload)
    }
}

/// An archive with the entries as they are, written by hand.
fn zip(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let le16 = |v: u16| v.to_le_bytes();
    let le32 = |v: u32| v.to_le_bytes();
    for e in entries {
        let flags: u16 = if e.descriptor { 8 } else { 0 };
        let offset = u32::try_from(out.len()).unwrap();
        let csize = u32::try_from(e.data.len()).unwrap();
        out.extend([0x50, 0x4B, 3, 4]);
        out.extend(le16(20));
        out.extend(le16(flags));
        out.extend(le16(e.method));
        out.extend(le16(0));
        out.extend(le16(33));
        let (c, cs, us) = if e.descriptor {
            (0, 0, 0)
        } else {
            (e.crc, csize, e.size)
        };
        out.extend(le32(c));
        out.extend(le32(cs));
        out.extend(le32(us));
        out.extend(le16(u16::try_from(e.name.len()).unwrap()));
        out.extend(le16(0));
        out.extend(e.name.as_bytes());
        out.extend(&e.data);
        if e.descriptor {
            out.extend([0x50, 0x4B, 7, 8]);
            out.extend(le32(e.crc));
            out.extend(le32(csize));
            out.extend(le32(e.size));
        }
        central.extend([0x50, 0x4B, 1, 2]);
        central.extend(le16(20));
        central.extend(le16(20));
        central.extend(le16(flags));
        central.extend(le16(e.method));
        central.extend(le16(0));
        central.extend(le16(33));
        central.extend(le32(e.crc));
        central.extend(le32(csize));
        central.extend(le32(e.size));
        central.extend(le16(u16::try_from(e.name.len()).unwrap()));
        central.extend([0; 12]);
        central.extend(le32(offset));
        central.extend(e.name.as_bytes());
    }
    let cd_offset = u32::try_from(out.len()).unwrap();
    out.extend(&central);
    out.extend([0x50, 0x4B, 5, 6, 0, 0, 0, 0]);
    out.extend(le16(u16::try_from(entries.len()).unwrap()));
    out.extend(le16(u16::try_from(entries.len()).unwrap()));
    out.extend(le32(u32::try_from(central.len()).unwrap()));
    out.extend(le32(cd_offset));
    out.extend(le16(0));
    out
}

fn read(bytes: &[u8]) -> Result<container::Container, ContainerError> {
    container::read(Cursor::new(bytes), &Limits::default())
}

fn payload() -> Vec<u8> {
    (0..5000u32).flat_map(|i| (i % 251).to_le_bytes()).collect()
}

#[test]
fn the_writer_makes_one_deflated_entry_that_holds_the_payload() {
    let payload = payload();
    let bytes = container::write(&payload, None).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(archive.len(), 1);
    let entry = archive.by_index(0).unwrap();
    assert_eq!(entry.name().unwrap(), "cubik");
    assert_eq!(entry.compression(), CompressionMethod::Deflated);
    drop(entry);
    let read = read(&bytes).unwrap();
    assert_eq!(read.payload, payload);
    assert!(read.extension.is_none());
}

#[test]
fn stored_and_deflated_entries_are_read() {
    let payload = payload();
    assert_eq!(
        read(&zip(&[stored("cubik", &payload)])).unwrap().payload,
        payload
    );
    assert_eq!(
        read(&zip(&[deflated("cubik", &payload)])).unwrap().payload,
        payload
    );
}

#[test]
fn a_data_descriptor_after_the_data_is_followed() {
    let payload = payload();
    for mut entry in [stored("cubik", &payload), deflated("cubik", &payload)] {
        entry.descriptor = true;
        assert_eq!(read(&zip(&[entry])).unwrap().payload, payload);
    }
}

#[test]
fn zip64_records_are_read() {
    let payload = payload();
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    w.start_file("cubik", options).unwrap();
    w.write_all(&payload).unwrap();
    let bytes = w.finish().unwrap().into_inner();
    // The local header carries the ZIP64 extra field.
    assert!(bytes.windows(4).any(|w| w == [1, 0, 16, 0]));
    assert_eq!(read(&bytes).unwrap().payload, payload);
}

#[test]
fn other_entries_are_ignored_and_the_name_may_differ_in_case() {
    let payload = payload();
    let other = deflated("other.txt", b"not the payload");
    let bytes = zip(&[other, deflated("CUBIK", &payload)]);
    assert_eq!(read(&bytes).unwrap().payload, payload);

    // The exact name wins over a name in another case.
    let bytes = zip(&[
        deflated("Cubik", b"first"),
        deflated("cubik", &payload),
        deflated("CUBIK", b"last"),
    ]);
    assert_eq!(read(&bytes).unwrap().payload, payload);
}

#[test]
fn a_missing_entry_is_not_a_zip_error() {
    let bytes = zip(&[deflated("other", b"x")]);
    assert!(matches!(read(&bytes), Err(ContainerError::NoEntry)));
    assert!(matches!(read(&zip(&[])), Err(ContainerError::NoEntry)));
}

#[test]
fn text_and_truncated_archives_are_not_zips() {
    assert!(matches!(
        read(b"this is not a zip archive at all"),
        Err(ContainerError::NotZip(_))
    ));
    assert!(matches!(read(b""), Err(ContainerError::NotZip(_))));
    let bytes = container::write(&payload(), None).unwrap();
    for cut in [1, 10, bytes.len() / 2] {
        assert!(
            matches!(
                read(&bytes[..bytes.len() - cut]),
                Err(ContainerError::NotZip(_))
            ),
            "cut {cut}"
        );
    }
}

#[test]
fn damaged_data_is_a_corrupt_entry() {
    let payload = payload();
    // A flipped byte of stored data fails the checksum.
    let mut bytes = zip(&[stored("cubik", &payload)]);
    bytes[40] ^= 0xFF;
    assert!(matches!(read(&bytes), Err(ContainerError::Corrupt(_))));
    // A damaged deflate stream fails to inflate or to check.
    let mut bytes = zip(&[deflated("cubik", &payload)]);
    for b in &mut bytes[34..60] {
        *b = !*b;
    }
    assert!(matches!(read(&bytes), Err(ContainerError::Corrupt(_))));
    // An unknown compression method.
    let mut entry = stored("cubik", &payload);
    entry.method = 99;
    assert!(matches!(
        read(&zip(&[entry])),
        Err(ContainerError::Corrupt(_))
    ));
}

#[test]
fn dotnetzip_segments_above_512_kib_are_read() {
    // Cubik's writer cuts a large payload into 64 KiB segments, each a raw deflate stream that
    // ends with a sync flush, and closes the stream with an empty final block (native 2.1).
    let payload: Vec<u8> = (0..700_000u32)
        .map(|i| u8::try_from((i * 7 + i / 300) % 256).unwrap())
        .collect();
    let mut stream = Vec::new();
    for segment in payload.chunks(64 * 1024) {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(1));
        encoder.write_all(segment).unwrap();
        // A flush is a sync flush: it ends the segment with an empty stored block.
        encoder.flush().unwrap();
        stream.extend(encoder.get_ref());
    }
    stream.extend([0x03, 0x00]);
    let entry = Entry {
        method: 8,
        data: stream,
        ..stored("cubik", &payload)
    };
    let bytes = zip(&[entry]);
    assert_eq!(read(&bytes).unwrap().payload, payload);
}

#[test]
fn the_payload_limit_applies_to_what_is_inflated() {
    let zeros = vec![0u8; 3 << 20];
    let bytes = container::write(&zeros, None).unwrap();
    assert!(bytes.len() < 16 << 10, "the archive is small");
    let limits = Limits {
        max_payload: 1 << 20,
        ..Limits::default()
    };
    assert!(matches!(
        container::read(Cursor::new(&bytes), &limits),
        Err(ContainerError::TooLarge { .. })
    ));
    // A header that claims less than the stream holds does not get around it.
    let mut entry = deflated("cubik", &zeros);
    entry.size = 1000;
    let lying = zip(&[entry]);
    let result = container::read(Cursor::new(&lying), &limits);
    assert!(result.is_err());
}

#[test]
fn the_extension_entry_is_read_next_to_the_payload() {
    let bytes = container::write(&payload(), Some(b"{\"version\":1}")).unwrap();
    let read = read(&bytes).unwrap();
    assert_eq!(
        read.extension.as_deref(),
        Some(b"{\"version\":1}".as_slice())
    );
}
