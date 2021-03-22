pub mod banner;
pub mod level;
pub mod map;
pub mod tile;
mod utilities;

use anyhow::Result;
use askama::Template;
use banner::Banner;
use filetime::{self, FileTime};
use indicatif::ProgressBar;
use level::{Bounds, Level, MapData};
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
    level_path: &PathBuf,
    quiet: bool,
    region_bounds: Option<&Bounds>,
) -> Result<HashSet<u32>> {
    let start_time = Instant::now();
    let mut players_searched = 0;
    let mut regions_searched = 0;

    let ids_by_player = level::search_players(&level_path, quiet, &mut players_searched)?;

    let ids_by_region =
        level::search_regions(&level_path, quiet, region_bounds, &mut regions_searched)?;

    let ids: HashSet<u32> = ids_by_region
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .chain(ids_by_player.into_iter().flat_map(|(_, ids)| ids))
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
    level_path: &PathBuf,
    output_path: &PathBuf,
    quiet: bool,
    force: bool,
    level_info: &Level,
    ids: HashSet<u32>,
) -> Result<()> {
    let start_time = Instant::now();
    let mut maps_rendered = 0;
    let mut tiles_rendered = 0;

    let map_scan = level::scan_maps(&level_path, ids)?;
    maps_rendered += map_scan.maps_by_tile.len();

    fn render_quadrant<'a>(
        tile_count: &mut usize,
        level_path: &PathBuf,
        output_path: &PathBuf,
        force: bool,
        bar: ProgressBar,
        maps_by_tile: &'a HashMap<Tile, BTreeSet<Map>>,
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

            bar.inc(1);
        } else {
            tile.quadrants().iter().try_for_each(|t| {
                render_quadrant(
                    tile_count,
                    level_path,
                    output_path,
                    force,
                    bar.clone(),
                    maps_by_tile,
                    layers,
                    &t,
                )
            })?;
        }

        layers.pop();

        Ok(())
    };

    let length = map_scan.root_tiles.len();
    let hidden = quiet || length < 3;

    let bar = progress_bar(hidden, "Render", length * 4usize.pow(4), "tiles");

    tiles_rendered += map_scan
        .root_tiles
        .par_iter()
        .map(|t| -> Result<usize> {
            let mut tile_count = 0;

            render_quadrant(
                &mut tile_count,
                &level_path,
                &output_path,
                force,
                bar.clone(),
                &map_scan.maps_by_tile,
                &mut Vec::with_capacity(5),
                t,
            )?;

            Ok(tile_count)
        })
        .try_reduce(|| 0, |a, b| Ok(a + b))?;

    bar.finish_and_clear();

    if let Some(modified) = map_scan.banners_modified {
        let banners_path = output_path.join("banners.json");

        if force
            || fs::metadata(&banners_path)
                .map(|m| FileTime::from_last_modification_time(&m))
                .map_or(true, |json_modified| json_modified < modified)
        {
            let label_counts = {
                let mut counts: HashMap<&str, usize> = HashMap::new();
                map_scan
                    .banners
                    .iter()
                    .filter_map(|b| b.label.as_ref())
                    .for_each(|label| {
                        *counts.entry(label).or_insert(0) += 1;
                    });
                counts
            };

            let is_unique = |banner: &Banner| -> bool {
                banner
                    .label
                    .as_deref()
                    .map_or(false, |l| matches!(label_counts.get(l), Some(1)))
            };

            serde_json::to_writer(
                &File::create(&banners_path)?,
                &json!({
                    "type": "FeatureCollection",
                    "features": map_scan.banners.iter().map(|banner| json!({
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
    generator: &str,
    level_path: &PathBuf,
    output_path: &PathBuf,
    quiet: bool,
    force: bool,
) -> Result<()> {
    let level_info = level::read_level(&level_path)?;
    if !VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level_info.version) {
        panic!("Incompatible with game version {}", level_info.version);
    }

    let map_ids = search(&level_path, quiet, None)?;

    render(
        &generator,
        &level_path,
        &output_path,
        quiet,
        force,
        &level_info,
        map_ids,
    )?;

    Ok(())
}
