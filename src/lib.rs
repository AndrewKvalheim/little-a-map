#![warn(clippy::nursery, clippy::pedantic)]
#![allow(
    clippy::implicit_hasher,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::similar_names,
    clippy::too_many_lines
)]

mod banner;
mod cache;
pub mod level;
mod map;
pub mod palette;
mod search;
mod tile;
mod utilities;

use anyhow::Result;
use askama::Template;
use banner::Banner;
use cache::Cache;
use filetime::{self, FileTime};
use glob::glob;
use indicatif::ProgressBar;
use level::Level;
use log::debug;
use map::{Map, MapData, MapScan};
use rayon::prelude::*;
use search::{search_entities, search_level, search_players, Bounds};
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::ops::AddAssign;
use std::path::Path;
use std::time::Instant;
use tile::Tile;
use utilities::progress_bar;

pub const COMPATIBLE_VERSIONS: &str = "~1.19.0";

#[derive(Template)]
#[template(path = "index.html.j2")]
struct IndexTemplate<'a> {
    center: [i32; 2],
    generator: &'a str,
}

#[derive(Default)]
struct Report {
    pub rendered: usize,
    pub tiles: HashSet<(u8, i32, i32)>,
}

impl AddAssign for Report {
    fn add_assign(&mut self, other: Self) {
        self.rendered += other.rendered;
        self.tiles.extend(other.tiles);
    }
}

struct Quadrant<'a> {
    world_path: &'a Path,
    output_path: &'a Path,
    force: bool,
    bar: &'a ProgressBar,
    maps_by_tile: &'a HashMap<Tile, BTreeSet<Map>>,
    layers: &'a mut Vec<Option<Vec<(&'a Map, MapData)>>>,
}

impl Quadrant<'_> {
    fn render(&mut self, tile: &Tile) -> Result<Report> {
        let mut report = Report::default();

        self.layers.push(
            self.maps_by_tile
                .get(tile)
                .map(|maps| {
                    maps.iter()
                        .map(|m| Ok((m, MapData::from_world_path(self.world_path, m.id)?)))
                        .collect::<Result<_>>()
                })
                .transpose()?,
        );

        if tile.zoom == 4 {
            let maps = || self.layers.iter().flatten().flatten();

            if maps().next().is_some() {
                report.tiles.insert((tile.zoom, tile.x, tile.y));

                if let Some(map_modified) = maps().map(|&(m, _)| m.modified).max() {
                    if tile.render(self.output_path, maps().rev(), map_modified, self.force)? {
                        report.rendered += 1;
                    }
                }
            }

            self.bar.inc(1);
        } else {
            for quadrant in &tile.quadrants() {
                report += self.render(quadrant)?;
            }
        }

        self.layers.pop();

        Ok(report)
    }
}

pub fn search(
    world_path: &Path,
    output_path: &Path,
    quiet: bool,
    force: bool,
    bounds: Option<&Bounds>,
) -> Result<HashSet<u32>> {
    let start_time = Instant::now();

    let cache_path = output_path.join(format!(".cache/{}.dat", env!("CARGO_PKG_NAME")));
    let mut cache = if force {
        Cache::default()
    } else {
        Cache::from_path(&cache_path)?
    };
    let players_searched = search_players(world_path, quiet, &mut cache)?;
    let entity_regions_searched = search_entities(world_path, quiet, bounds, &mut cache)?;
    let level_regions_searched = search_level(world_path, quiet, bounds, &mut cache)?;
    cache.write_to(&cache_path)?;

    if !quiet {
        println!(
            "Searched {level_regions_searched} level regions, {entity_regions_searched} entity regions, and {players_searched} players in {:.2}s",
            start_time.elapsed().as_secs_f32()
        );
    }

    Ok(cache
        .map_ids_by_entities_region
        .into_values()
        .chain(cache.map_ids_by_level_region.into_values())
        .chain(cache.map_ids_by_player.into_values())
        .flatten()
        .collect())
}

pub fn render(
    world_path: &Path,
    output_path: &Path,
    quiet: bool,
    force: bool,
    level: &Level,
    ids: &HashSet<u32>,
) -> Result<()> {
    let start_time = Instant::now();

    let results = MapScan::run(world_path, ids)?;
    let maps_rendered = results.maps_by_tile.len();

    let length = results.root_tiles.len() * 4_usize.pow(4);
    let bar = progress_bar(quiet, "Render", length, "tiles");

    let report = results
        .root_tiles
        .par_iter()
        .map(|tile| {
            Quadrant {
                world_path,
                output_path,
                force,
                bar: &bar,
                maps_by_tile: &results.maps_by_tile,
                layers: &mut Vec::with_capacity(5),
            }
            .render(tile)
        })
        .try_reduce(Report::default, |mut a, b| {
            a += b;
            Ok(a)
        })?;

    bar.finish_and_clear();

    let pruned = glob(output_path.join("tiles/*/*/*.png").to_str().unwrap())?
        .map(|entry| -> Result<usize> {
            let path = entry?;
            let relative = path.strip_prefix(output_path)?;
            let mut parts = relative.to_str().unwrap().split('/').skip(1);
            let zoom: u8 = parts.next().unwrap().parse()?;
            let x: i32 = parts.next().unwrap().parse()?;
            let y: i32 = parts.next().unwrap().split('.').next().unwrap().parse()?;

            Ok(if report.tiles.contains(&(zoom, x, y)) {
                0
            } else {
                let base = output_path.join(format!("tiles/{zoom}/{x}/{y}"));
                debug!("Prune: {}", base.as_path().to_str().unwrap());
                fs::remove_file(base.with_extension("png"))?;
                fs::remove_file(base.with_extension("meta.json"))?;
                1
            })
        })
        .sum::<Result<usize>>()?;

    if let Some(modified) = results.banners_modified {
        let banners_path = output_path.join("banners.json");

        if force
            || pruned != 0
            || fs::metadata(&banners_path)
                .map(|m| FileTime::from_last_modification_time(&m))
                .map_or(true, |json_modified| json_modified < modified)
        {
            let is_unique = {
                let mut u = HashMap::<&str, bool>::new();
                results
                    .banners
                    .iter()
                    .filter_map(|b| b.label.as_ref())
                    .for_each(|l| {
                        u.entry(l).and_modify(|v| *v = false).or_insert(true);
                    });
                move |b: &Banner| b.label.as_deref().map_or(false, |l| *u.get(l).unwrap())
            };

            serde_json::to_writer(
                &File::create(&banners_path)?,
                &json!({
                    "type": "FeatureCollection",
                    "features": results.banners.iter().map(|banner| json!({
                        "type": "Feature",
                        "geometry": {
                            "type": "Point",
                            "coordinates": [banner.x, banner.z]
                        },
                        "properties": {
                            "color": banner.color,
                            "maps": results.map_ids_by_banner_position[&(banner.x, banner.z)],
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
        center: [level.spawn_z, level.spawn_x],
        generator: &format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    };
    File::create(output_path.join("index.html"))?.write_all(index_template.render()?.as_bytes())?;

    if !quiet {
        if report.rendered == 0 && pruned == 0 {
            println!("Already up-to-date");
        } else {
            println!(
                "Rendered {} and pruned {pruned} tiles from {maps_rendered} map items in {:.2}s",
                report.rendered,
                start_time.elapsed().as_secs_f32()
            );
        }
    }

    Ok(())
}