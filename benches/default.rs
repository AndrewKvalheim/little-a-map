use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::env;
use std::path::PathBuf;

pub fn bench(c: &mut Criterion) {
    let level_path = PathBuf::from(env!("BENCH_LEVEL_PATH"));
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let generator = "benchmark";

    let mut group = c.benchmark_group("default");
    group.sample_size(20);
    group.bench_function("run", |b| {
        b.iter(|| {
            lib::run(
                generator,
                black_box(&level_path),
                black_box(&output_path),
                black_box(true),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
