#![warn(clippy::nursery, clippy::pedantic)]
#![allow(
    clippy::implicit_hasher,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::non_ascii_literal,
    clippy::too_many_lines
)]

// Workaround for https://github.com/rust-lang/rust/issues/55779
extern crate serde;

mod banner;
mod cache;
pub mod level;
mod map;
mod search;
mod tile;
mod utilities;

use anyhow::Result;
use askama::Template;
use banner::Banner;
use cache::Cache;
use filetime::{self, FileTime};
use indicatif::ProgressBar;
use level::Level;
use map::{Map, MapData, MapScan};
use rayon::prelude::*;
use search::{search_players, search_regions, Bounds};
use semver::VersionReq;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use tile::Tile;
use utilities::progress_bar;

const COMPATIBLE_VERSIONS: &str = "~1.16.2";

#[derive(Template)]
#[template(path = "index.html.j2")]
struct IndexTemplate<'a> {
    generator: &'a str,
    spawn_x: i32,
    spawn_z: i32,
}

pub fn search(
    name: &str,
    world_path: &Path,
    output_path: &Path,
    quiet: bool,
    force: bool,
    bounds: Option<&Bounds>,
) -> Result<HashSet<u32>> {
    let start_time = Instant::now();
    let mut players_searched = 0;
    let mut regions_searched = 0;

    let cache_path = output_path.join(format!(".cache/{}.dat", name));
    let mut cache = if force {
        Cache::default()
    } else {
        Cache::from_path(&cache_path)?
    };
    search_players(world_path, &mut cache, quiet, &mut players_searched)?;
    search_regions(world_path, &mut cache, quiet, bounds, &mut regions_searched)?;
    cache.write_to(&cache_path)?;

    // Pending https://github.com/rust-lang/rust/issues/75294
    let ids = cache
        .map_ids_by_player
        .into_iter()
        .map(|(_, v)| v)
        .chain(cache.map_ids_by_region.into_iter().map(|(_, v)| v))
        .flatten()
        .collect();

    if !quiet {
        println!(
            "Searched {} regions and {} players in {:.2}s",
            regions_searched,
            players_searched,
            start_time.elapsed().as_secs_f32()
        );
    }

    Ok(ids)
}

pub fn render(
    generator: &str,
    world_path: &Path,
    output_path: &Path,
    quiet: bool,
    force: bool,
    level: &Level,
    ids: HashSet<u32>,
) -> Result<()> {
    struct RenderQuadrant<'a> {
        world_path: &'a Path,
        output_path: &'a Path,
        force: bool,
        bar: &'a ProgressBar,
        maps_by_tile: &'a HashMap<Tile, BTreeSet<Map>>,
        layers: &'a mut Vec<Option<Vec<(&'a Map, MapData)>>>,
    }
    impl<'a> RenderQuadrant<'a> {
        fn f(&mut self, tile: &Tile) -> Result<usize> {
            let mut count = 0;

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
                let maps = self.layers.iter().flatten().flatten();

                if let Some(map_modified) = maps.clone().map(|&(m, _)| m.modified).max() {
                    if tile.render(self.output_path, maps.rev(), map_modified, self.force)? {
                        count += 1;
                    }
                }

                self.bar.inc(1);
            } else {
                for quadrant in &tile.quadrants() {
                    count += self.f(quadrant)?;
                }
            }

            self.layers.pop();

            Ok(count)
        }
    }

    let start_time = Instant::now();
    let mut maps_rendered = 0;
    let mut tiles_rendered = 0;

    let results = MapScan::run(world_path, ids)?;
    maps_rendered += results.maps_by_tile.len();

    let length = results.root_tiles.len() * 4_usize.pow(4);
    let bar = progress_bar(quiet, "Render", length, 4_u64.pow(3), "tiles");

    tiles_rendered += results
        .root_tiles
        .par_iter()
        .map(|tile| {
            RenderQuadrant {
                world_path,
                output_path,
                force,
                bar: &bar,
                maps_by_tile: &results.maps_by_tile,
                layers: &mut Vec::with_capacity(5),
            }
            .f(tile)
        })
        .try_reduce(|| 0, |a, b| Ok(a + b))?;

    bar.finish_and_clear();

    if let Some(modified) = results.banners_modified {
        let banners_path = output_path.join("banners.json");

        if force
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
        spawn_x: level.spawn_x,
        spawn_z: level.spawn_z,
    };
    File::create(output_path.join("index.html"))?.write_all(index_template.render()?.as_bytes())?;

    if !quiet {
        if tiles_rendered == 0 {
            println!("Already up-to-date");
        } else {
            println!(
                "Rendered {} tiles from {} map items in {:.2}s",
                tiles_rendered,
                maps_rendered,
                start_time.elapsed().as_secs_f32()
            );
        }
    }

    Ok(())
}

pub fn run(
    name: &str,
    version: &str,
    world_path: &Path,
    output_path: &Path,
    quiet: bool,
    force: bool,
) -> Result<()> {
    let level = Level::from_world_path(world_path)?;
    if !VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level.version) {
        panic!("Incompatible with game version {}", level.version);
    }

    let map_ids = search(name, world_path, output_path, quiet, force, None)?;

    let generator = format!("{} {}", name, version);
    render(
        &generator,
        world_path,
        output_path,
        quiet,
        force,
        &level,
        map_ids,
    )?;

    Ok(())
}
