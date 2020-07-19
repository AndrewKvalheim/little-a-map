mod banner;
mod level;
mod map;
mod tile;

use askama::Template;
use banner::Banner;
use level::MapData;
use map::Map;
use rayon::prelude::*;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use structopt::StructOpt;
use tile::Tile;

type OrderedMaps = BTreeSet<Map>;

#[derive(StructOpt)]
struct Args {
    #[structopt(parse(from_os_str))]
    level_path: PathBuf,

    #[structopt(long = "output", default_value = "dist", parse(from_os_str))]
    output_path: PathBuf,
}

struct Stats {
    maps: usize,
    tiles: usize,
    start: Instant,
}

#[derive(Template)]
#[template(path = "index.html.j2")]
struct IndexTemplate {
    spawn_x: i32,
    spawn_z: i32,
}

#[paw::main]
fn main(args: Args) {
    let level_path = args.level_path;
    let output_path = args.output_path;

    let mut stats = Stats {
        maps: 0,
        tiles: 0,
        start: Instant::now(),
    };

    let mut banners: BTreeSet<Banner> = BTreeSet::new();
    let mut root_tiles: HashSet<Tile> = HashSet::new();
    let mut maps_by_tile: HashMap<Tile, OrderedMaps> = HashMap::new();

    level::scan(
        &level_path,
        |map| {
            root_tiles.insert(map.tile.root());

            stats.maps += 1;
            maps_by_tile
                .entry(map.tile.clone())
                .or_insert_with(BTreeSet::new)
                .insert(map);
        },
        |banner| {
            banners.insert(banner);
        },
    );

    fn render<'a>(
        tile_count: &mut usize,
        level_path: &PathBuf,
        output_path: &PathBuf,
        maps_by_tile: &'a HashMap<Tile, OrderedMaps>,
        layers: &mut Vec<Option<Vec<(&'a Map, MapData)>>>,
        tile: &Tile,
    ) {
        layers.push(maps_by_tile.get(&tile).map(|maps| {
            maps.iter()
                .map(|map| (map, level::load_map(level_path, map.id)))
                .collect()
        }));

        if tile.zoom == 4 {
            if layers.iter().any(Option::is_some) {
                *tile_count += 1;

                tile.render(output_path, layers.iter().flatten().flatten());
            }
        } else {
            tile.quadrants().iter().for_each(|t| {
                render(
                    tile_count,
                    level_path,
                    output_path,
                    maps_by_tile,
                    layers,
                    &t,
                )
            });
        }

        layers.pop();
    };
    stats.tiles += root_tiles
        .par_iter()
        .map(|t| {
            let mut tile_count = 0;

            render(
                &mut tile_count,
                &level_path,
                &output_path,
                &maps_by_tile,
                &mut Vec::with_capacity(5),
                t,
            );

            tile_count
        })
        .sum::<usize>();

    let label_counts = {
        let mut counts: HashMap<&str, usize> = HashMap::new();

        banners
            .iter()
            .filter_map(|b| b.label.as_ref())
            .for_each(|label| {
                *counts.entry(label).or_insert(0) += 1;
            });

        counts
    };

    serde_json::to_writer(
        &File::create(output_path.join("banners.json")).unwrap(),
        &json!({
            "type": "FeatureCollection",
            "features": banners.iter().map(|banner| json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [banner.x, banner.z]
                },
                "properties": {
                    "name": banner.label,
                    "unique": banner.label.as_ref().map_or(false, |l| *label_counts.get(l.as_str()).unwrap() == 1),
                }
            })).collect::<Vec<_>>()
        }),
    )
    .unwrap();

    let (spawn_x, spawn_z) = level::get_spawn(&level_path);
    let index_template = IndexTemplate { spawn_x, spawn_z };
    File::create(output_path.join("index.html"))
        .unwrap()
        .write_all(index_template.render().unwrap().as_bytes())
        .unwrap();

    println!(
        "Rendered {} map items onto {} tiles in {:.2}s.",
        stats.maps,
        stats.tiles,
        stats.start.elapsed().as_secs_f32()
    );
}
