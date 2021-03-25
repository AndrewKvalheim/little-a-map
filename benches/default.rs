use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use glob::glob;
use lib::{level::Level, render, search};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

fn all_map_ids(world_path: &Path) -> HashSet<u32> {
    glob(world_path.join("data/map_*.dat").to_str().unwrap())
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
        .collect()
}

pub fn bench_render(c: &mut Criterion) {
    let world_path = PathBuf::from(env!("BENCH_WORLD_PATH"));
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let generator = "benchmark";
    let level_info = Level::from_world_path(&world_path).unwrap();
    let map_ids = all_map_ids(&world_path);

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(20);
    group.bench_function("render", |b| {
        b.iter_batched(
            || map_ids.clone(),
            |ids| {
                render(
                    generator,
                    black_box(&world_path),
                    black_box(&output_path),
                    true,
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

pub fn bench_search(c: &mut Criterion) {
    let world_path = PathBuf::from(env!("BENCH_WORLD_PATH"));
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
    group.bench_function("search", |b| {
        b.iter(|| search(black_box(&world_path), true, Some(&bounds)))
    });
    group.finish();
}

criterion_group!(benches, bench_search, bench_render);
criterion_main!(benches);
