#![allow(clippy::module_name_repetitions)]

use crate::cache::{Cache, MapIdsByRegion};
use crate::utilities::{progress_bar, read_gz};
use anyhow::Result;
use fastnbt::de::from_bytes;
use glob::glob;
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use serde::{de::DeserializeOwned, de::IgnoredAny, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

pub type Bounds = ((i32, i32), (i32, i32));

trait ContainsMapIds {
    fn map_ids(self) -> HashSet<u32>;
}

struct MapIdsOfEntity(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfEntity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            item: Option<MapIdOfItem>,
            items: Option<Vec<MapIdOfItem>>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .items
                .into_iter()
                .flatten()
                .chain(internal.item)
                .filter_map(|i| i.0)
                .collect(),
        ))
    }
}

struct MapIdsOfEntitiesChunk(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfEntitiesChunk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            entities: Vec<MapIdsOfEntity>,
        }

        Ok(Self(
            Internal::deserialize(deserializer)?
                .entities
                .into_iter()
                .flat_map(|e| e.0)
                .collect(),
        ))
    }
}
impl ContainsMapIds for MapIdsOfEntitiesChunk {
    fn map_ids(self) -> HashSet<u32> {
        self.0
    }
}

struct MapIdOfItem(Option<u32>);
impl<'de> Deserialize<'de> for MapIdOfItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #![allow(clippy::use_self)] // Pending https://github.com/rust-lang/rust-clippy/issues/6902

        #[derive(Deserialize)]
        #[serde(tag = "id")]
        enum Internal {
            #[serde(rename = "minecraft:filled_map")]
            FilledMap { tag: FilledMapTag },

            #[serde(other)]
            Other,
        }

        #[derive(Deserialize)]
        struct FilledMapTag {
            display: Option<IgnoredAny>,
            map: u32,
        }

        Ok(Self(match Internal::deserialize(deserializer)? {
            Internal::FilledMap { tag } if tag.display.is_none() => Some(tag.map),
            _ => None,
        }))
    }
}

struct MapIdsOfLevelChunk(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfLevelChunk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            level: Level,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Level {
            entities: Option<Vec<MapIdsOfEntity>>,
            tile_entities: Vec<MapIdsOfEntity>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .level
                .entities
                .into_iter()
                .flatten()
                .chain(internal.level.tile_entities)
                .flat_map(|e| e.0)
                .collect(),
        ))
    }
}
impl ContainsMapIds for MapIdsOfLevelChunk {
    fn map_ids(self) -> HashSet<u32> {
        self.0
    }
}

struct MapIdsOfPlayer(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfPlayer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            ender_items: Vec<MapIdOfItem>,
            inventory: Vec<MapIdOfItem>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .ender_items
                .into_iter()
                .chain(internal.inventory)
                .filter_map(|i| i.0)
                .collect(),
        ))
    }
}

fn search_regions<T: ContainsMapIds + DeserializeOwned>(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    cache: &mut Cache,
    pattern: &str,
) -> Result<(usize, MapIdsByRegion)> {
    let regions = glob(world_path.join(pattern).to_str().unwrap())?
        .map(|entry| {
            let path = entry?;
            let base = path.file_stem().unwrap().to_str().unwrap();
            let mut parts = base.split('.').skip(1);
            let x = parts.next().unwrap().parse()?;
            let z = parts.next().unwrap().parse()?;

            Ok(match bounds {
                Some(&((x0, z0), (x1, z1))) if x < x0 || x > x1 || z < z0 || z > z1 => None,
                _ => cache.is_expired_for(&path)?.then(|| ((x, z), path)),
            })
        })
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>>>()?;

    let length = regions.len();
    let bar = progress_bar(quiet, "Search for map items", length, "regions");

    let map_ids_by_region = regions
        .into_par_iter()
        .progress_with(bar.clone())
        .map(|(position, path)| {
            let mut map_ids = HashSet::new();

            fastanvil::RegionBuffer::new(File::open(&path)?)
                .for_each_chunk(|_, _, nbt| {
                    map_ids.extend(from_bytes::<T>(nbt).unwrap().map_ids());
                })
                .unwrap_or_default();

            Ok((position, map_ids))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    bar.finish_and_clear();
    Ok((length, map_ids_by_region))
}

pub fn search_players(world_path: &Path, quiet: bool, cache: &mut Cache) -> Result<usize> {
    let pattern = world_path.join("playerdata/????????-????-????-????-????????????.dat");
    let mut paths = glob(pattern.to_str().unwrap())?.collect::<Result<Vec<_>, _>>()?;
    paths.sort();

    let players = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| Ok(cache.is_expired_for(&path)?.then(|| (index, path))))
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>>>()?;

    let length = players.len();
    let bar = progress_bar(quiet, "Search for map items", length, "players");
    let ids = players
        .into_par_iter()
        .progress_with(bar.clone())
        .map(|(index, path)| Ok((index, from_bytes::<MapIdsOfPlayer>(&read_gz(&path)?)?.0)))
        .collect::<Result<HashMap<_, _>>>()?;
    bar.finish_and_clear();

    cache.map_ids_by_player.extend(ids);
    Ok(length)
}

pub fn search_entities(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    cache: &mut Cache,
) -> Result<usize> {
    let pattern = "entities/r.*.mca";
    let (length, ids) =
        search_regions::<MapIdsOfEntitiesChunk>(world_path, quiet, bounds, cache, pattern)?;

    cache.map_ids_by_entities_region.extend(ids);
    Ok(length)
}

pub fn search_level(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    cache: &mut Cache,
) -> Result<usize> {
    let pattern = "region/r.*.mca";
    let (length, ids) =
        search_regions::<MapIdsOfLevelChunk>(world_path, quiet, bounds, cache, pattern)?;

    cache.map_ids_by_level_region.extend(ids);
    Ok(length)
}
