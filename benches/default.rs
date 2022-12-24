use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lib::{level::Level, render, search};
use std::env;
use std::path::PathBuf;

pub fn bench_render(c: &mut Criterion) {
    let world_path = PathBuf::from(env!("BENCH_WORLD_PATH"));
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let level_info = Level::from_world_path(&world_path).unwrap();
    let map_ids = search("benchmark", &world_path, &output_path, false, false, None).unwrap();
    println!("Found {} maps", map_ids.len());

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(20);
    group.bench_function("render", |b| {
        b.iter_batched(
            || map_ids.clone(),
            |ids| {
                render(
                    "benchmark",
                    black_box(&world_path),
                    black_box(&output_path),
                    true,
                    black_box(true),
                    black_box(&level_info),
                    &ids,
                )
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

pub fn bench_search(c: &mut Criterion) {
    let world_path = PathBuf::from(env!("BENCH_WORLD_PATH"));
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let bounds = (
        (
            env!("BENCH_SEARCH_REGION_X0").parse().unwrap(),
            env!("BENCH_SEARCH_REGION_Z0").parse().unwrap(),
        ),
        (
            env!("BENCH_SEARCH_REGION_X1").parse().unwrap(),
            env!("BENCH_SEARCH_REGION_Z1").parse().unwrap(),
        ),
    );

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(20);
    group.bench_function("search", |b| {
        b.iter(|| {
            search(
                "benchmark",
                black_box(&world_path),
                black_box(&output_path),
                true,
                black_box(true),
                Some(&bounds),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, bench_search, bench_render);
criterion_main!(benches);
