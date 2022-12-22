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
mod search;
mod tile;
mod utilities;

use anyhow::Result;
use askama::Template;
use banner::Banner;
use cache::Cache;
use filetime::{self, FileTime};
use forgiving_semver::VersionReq;
use indicatif::ProgressBar;
use level::Level;
use map::{Map, MapData, MapScan};
use rayon::prelude::*;
use search::{search_entities, search_level, search_players, Bounds};
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use tile::Tile;
use utilities::progress_bar;

const COMPATIBLE_VERSIONS: &str = "~1.19.0";

#[derive(Template)]
#[template(path = "index.html.j2")]
struct IndexTemplate<'a> {
    center: [i32; 2],
    generator: &'a str,
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

    let cache_path = output_path.join(format!(".cache/{name}.dat"));
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
    impl RenderQuadrant<'_> {
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
                let maps = || self.layers.iter().flatten().flatten();

                if let Some(map_modified) = maps().map(|&(m, _)| m.modified).max() {
                    if tile.render(self.output_path, maps().rev(), map_modified, self.force)? {
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
    let bar = progress_bar(quiet, "Render", length, "tiles");

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
        generator,
    };
    File::create(output_path.join("index.html"))?.write_all(index_template.render()?.as_bytes())?;

    if !quiet {
        if tiles_rendered == 0 {
            println!("Already up-to-date");
        } else {
            println!(
                "Rendered {tiles_rendered} tiles from {maps_rendered} map items in {:.2}s",
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
    assert!(
        VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level.version),
        "Incompatible with game version {}",
        level.version
    );

    let map_ids = search(name, world_path, output_path, quiet, force, None)?;

    let generator = format!("{name} {version}");
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

#[cfg(test)]
mod test {
    use super::*;
    use forgiving_semver::Version;
    use itertools::{assert_equal, Itertools};
    use once_cell::sync::Lazy;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use test_context::{test_context, TestContext};

    struct World {
        pub cache: Cache,
        pub level: Level,
        pub output: TempDir,
    }

    impl World {
        fn load(major: u64, minor: u64, patch: u64) -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../fixtures/world-{major}.{minor}"));

            let mut world = Self {
                cache: Cache::default(),
                level: Level::from_world_path(&path).unwrap(),
                output: tempfile::tempdir_in(env!("TEST_OUTPUT_PATH")).unwrap(),
            };

            assert_eq!(world.level.version, Version::new(major, minor, patch));
            search_players(&path, true, &mut world.cache).unwrap();
            search_entities(&path, true, None, &mut world.cache).unwrap();
            search_level(&path, true, None, &mut world.cache).unwrap();
            render(
                "test",
                &path,
                world.output.path(),
                true,
                true,
                &world.level,
                world
                    .cache
                    .map_ids_by_entities_region
                    .values()
                    .chain(world.cache.map_ids_by_level_region.values())
                    .chain(world.cache.map_ids_by_player.values())
                    .flatten()
                    .copied()
                    .collect(),
            )
            .unwrap();

            world
        }
    }

    struct Worlds([Lazy<World>; 1]);

    impl TestContext for Worlds {
        fn setup() -> Self {
            Self([Lazy::new(|| World::load(1, 19, 3))])
        }
    }

    #[test_context(Worlds)]
    #[test]
    fn spawn(worlds: &mut Worlds) {
        for world in &worlds.0 {
            assert_eq!((world.level.spawn_x, world.level.spawn_z), (0, 0));
        }
    }

    #[test_context(Worlds)]
    #[test]
    fn in_player(worlds: &mut Worlds) {
        let ids = [
            0, // Inventory
        ];

        for world in &worlds.0 {
            let found = world.cache.map_ids_by_player.values().flatten();
            assert_equal(found.sorted(), &ids);
        }
    }
}
