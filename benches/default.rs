use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use glob::glob;
use std::env;
use std::path::PathBuf;

fn all_map_ids(level_path: &PathBuf) -> impl IntoIterator<Item = u32> + Clone {
    glob(level_path.join("data/map_*.dat").to_str().unwrap())
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .rsplit('_')
                .next()
                .unwrap()
                .parse::<u32>()
                .unwrap()
        })
        .collect::<Vec<u32>>()
}

pub fn bench_render(c: &mut Criterion) {
    let level_path = PathBuf::from(env!("BENCH_LEVEL_PATH"));
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let generator = "benchmark";
    let level_info = lib::level::read_level(&level_path).unwrap();
    let map_ids = all_map_ids(&level_path);

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(20);
    group.bench_function("render", |b| {
        b.iter_batched(
            || map_ids.clone(),
            |ids| {
                lib::render(
                    generator,
                    black_box(&level_path),
                    black_box(&output_path),
                    black_box(true),
                    black_box(&level_info),
                    ids,
                )
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

pub fn bench_scan(c: &mut Criterion) {
    let level_path = PathBuf::from(env!("BENCH_LEVEL_PATH"));
    let bounds = (
        (
            env!("BENCH_SCAN_REGION_X0").parse::<i32>().unwrap(),
            env!("BENCH_SCAN_REGION_Z0").parse::<i32>().unwrap(),
        ),
        (
            env!("BENCH_SCAN_REGION_X1").parse::<i32>().unwrap(),
            env!("BENCH_SCAN_REGION_Z1").parse::<i32>().unwrap(),
        ),
    );

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(20);
    group.bench_function("scan", |b| {
        b.iter(|| lib::scan(black_box(&level_path), Some(&bounds)))
    });
    group.finish();
}

criterion_group!(benches, bench_scan, bench_render);
criterion_main!(benches);
