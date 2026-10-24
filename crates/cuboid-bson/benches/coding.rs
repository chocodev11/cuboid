//! Coding speed of a voxel payload shaped like native 4.7 (architecture 18):
//! `cargo bench -p cuboid-bson`.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use cuboid_bson::{Limits, RawDoc};

#[path = "../tests/support/mod.rs"]
mod support;

use support::payload::{decode_voxels, voxel_payload};

const VOXELS: usize = 1_000_000;

/// Visits every element of the tree and nothing else: the cost of finding the elements.
fn walk(doc: RawDoc<'_>) -> usize {
    let mut count = 0;
    for elem in doc.iter() {
        let elem = elem.unwrap();
        count += 1;
        if let Ok(Some(child)) = elem.doc().or_else(|_| elem.array()) {
            count += walk(child);
        }
    }
    count
}

fn coding(c: &mut Criterion) {
    let payload = voxel_payload(VOXELS);
    let mut group = c.benchmark_group("voxels");
    group.throughput(Throughput::Bytes(payload.len() as u64));
    group.bench_function("decode", |b| b.iter(|| decode_voxels(black_box(&payload))));
    group.bench_function("walk", |b| {
        b.iter(|| walk(RawDoc::parse(black_box(&payload), &Limits::DEFAULT).unwrap()));
    });
    group.bench_function("encode", |b| b.iter(|| voxel_payload(black_box(VOXELS))));
    group.finish();
}

criterion_group!(benches, coding);
criterion_main!(benches);
