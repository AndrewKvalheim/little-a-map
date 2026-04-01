use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use little_a_map::{render, search};
use std::env;
use std::hint::black_box;
use std::path::PathBuf;

pub fn bench_render(c: &mut Criterion) {
    let world = PathBuf::from(env!("BENCH_WORLD_PATH")).try_into().unwrap();
    let output_path = PathBuf::from(env!("BENCH_OUTPUT_PATH"));
    let map_ids = search(&world, &output_path, false, false, None).unwrap();
    println!("Found {} maps", map_ids.len());

    let mut group = c.benchmark_group("little-a-map");
    group.sample_size(10);
    group.bench_function("render", |b| {
        b.iter_batched(
            || map_ids.clone(),
            |ids| {
                render(
                    black_box(&world),
                    black_box(&output_path),
                    true,
                    black_box(true),
                    &ids,
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

pub fn bench_search(c: &mut Criterion) {
    let world = PathBuf::from(env!("BENCH_WORLD_PATH")).try_into().unwrap();
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
                black_box(&world),
                black_box(&output_path),
                true,
                black_box(true),
                Some(&bounds),
            )
        });
    });
    group.finish();
}

criterion_group!(benches, bench_search, bench_render);
criterion_main!(benches);
