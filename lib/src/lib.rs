pub mod banner;
pub mod level;
pub mod map;
pub mod tile;

use anyhow::Result;
use askama::Template;
use banner::Banner;
use filetime::{self, FileTime};
use level::MapData;
use map::Map;
use rayon::prelude::*;
use semver::VersionReq;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use tile::Tile;

const COMPATIBLE_VERSIONS: &str = "~1.16.2";

type OrderedMaps = BTreeSet<Map>;

struct Stats {
    banners: usize,
    tiles: usize,
    start: Instant,
}

#[derive(Template)]
#[template(path = "index.html.j2")]
struct IndexTemplate<'a> {
    generator: &'a str,
    spawn_x: i32,
    spawn_z: i32,
}

pub fn run(
    generator: &str,
    level_path: &PathBuf,
    output_path: &PathBuf,
    force: bool,
) -> Result<()> {
    let mut stats = Stats {
        banners: 0,
        tiles: 0,
        start: Instant::now(),
    };

    let level_info = level::read_level(&level_path)?;
    if !VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level_info.version) {
        panic!("Incompatible with game version {}", level_info.version);
    }

    let mut banners: BTreeSet<Banner> = BTreeSet::new();
    let mut banners_modified: Option<FileTime> = None;
    let mut root_tiles: HashSet<Tile> = HashSet::new();
    let mut maps_by_tile: HashMap<Tile, OrderedMaps> = HashMap::new();

    level::scan(
        &level_path,
        |map| {
            root_tiles.insert(map.tile.root());

            maps_by_tile
                .entry(map.tile.clone())
                .or_insert_with(BTreeSet::new)
                .insert(map);
        },
        |modified, banner| {
            banners.insert(banner);

            if banners_modified.map_or(true, |m| m < modified) {
                banners_modified.replace(modified);
            }
        },
    )?;

    fn render<'a>(
        tile_count: &mut usize,
        level_path: &PathBuf,
        output_path: &PathBuf,
        force: bool,
        maps_by_tile: &'a HashMap<Tile, OrderedMaps>,
        layers: &mut Vec<Option<Vec<(&'a Map, MapData)>>>,
        tile: &Tile,
    ) -> Result<()> {
        layers.push(
            maps_by_tile
                .get(&tile)
                .map(|maps| {
                    maps.iter()
                        .map(|map| Ok((map, level::load_map(level_path, map.id)?)))
                        .collect::<Result<_>>()
                })
                .transpose()?,
        );

        if tile.zoom == 4 {
            if let Some(map_modified) = layers
                .iter()
                .flatten()
                .flatten()
                .map(|&(m, _)| m.modified)
                .max()
            {
                if tile.render(
                    &output_path,
                    layers.iter().flatten().flatten().rev(),
                    map_modified,
                    force,
                )? {
                    *tile_count += 1;
                }
            }
        } else {
            tile.quadrants().iter().try_for_each(|t| {
                render(
                    tile_count,
                    level_path,
                    output_path,
                    force,
                    maps_by_tile,
                    layers,
                    &t,
                )
            })?;
        }

        layers.pop();

        Ok(())
    };
    stats.tiles += root_tiles
        .par_iter()
        .map(|t| -> Result<usize> {
            let mut tile_count = 0;

            render(
                &mut tile_count,
                &level_path,
                &output_path,
                force,
                &maps_by_tile,
                &mut Vec::with_capacity(5),
                t,
            )?;

            Ok(tile_count)
        })
        .try_reduce(|| 0, |a, b| Ok(a + b))?;

    if let Some(modified) = banners_modified {
        let banners_path = output_path.join("banners.json");

        if force
            || fs::metadata(&banners_path)
                .map(|m| FileTime::from_last_modification_time(&m))
                .map_or(true, |json_modified| json_modified < modified)
        {
            stats.banners += banners.len();

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

            let is_unique = |banner: &Banner| -> bool {
                match banner.label.as_deref() {
                    None => false,
                    Some(l) => match label_counts.get(l) {
                        Some(1) => true,
                        _ => false,
                    },
                }
            };

            serde_json::to_writer(
                &File::create(&banners_path)?,
                &json!({
                    "type": "FeatureCollection",
                    "features": banners.iter().map(|banner| json!({
                        "type": "Feature",
                        "geometry": {
                            "type": "Point",
                            "coordinates": [banner.x, banner.z]
                        },
                        "properties": {
                            "color": banner.color,
                            "name": banner.label,
                            "unique": is_unique(banner),
                        }
                    })).collect::<Vec<_>>()
                }),
            )?;

            filetime::set_file_mtime(banners_path, modified)?;
        }
    }

    let index_template = IndexTemplate {
        generator,
        spawn_x: level_info.spawn_x,
        spawn_z: level_info.spawn_z,
    };
    File::create(output_path.join("index.html"))?.write_all(index_template.render()?.as_bytes())?;

    if stats.banners == 0 && stats.tiles == 0 {
        println!("Nothing to do");
    } else {
        println!(
            "Rendered {} tiles and {} banners in {:.2}s",
            stats.tiles,
            stats.banners,
            stats.start.elapsed().as_secs_f32()
        );
    }

    Ok(())
}
