#![allow(clippy::module_name_repetitions)]

use crate::cache::Cache;
use crate::utilities::{progress_bar, read_gz};
use anyhow::Result;
use fastnbt::de::from_bytes;
use glob::glob;
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use serde::{de::IgnoredAny, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

pub type Bounds = ((i32, i32), (i32, i32));

struct ChunkMapIds(HashSet<u32>);
impl<'de> Deserialize<'de> for ChunkMapIds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            level: Level,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Level {
            entities: Vec<OptionItems>,
            tile_entities: Vec<OptionItems>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct OptionItems {
            item: Option<ItemMapId>,
            items: Option<Vec<ItemMapId>>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .level
                .entities
                .into_iter()
                .chain(internal.level.tile_entities)
                .flat_map(|e| e.items.into_iter().flatten().chain(e.item))
                .filter_map(|i| i.0)
                .collect(),
        ))
    }
}

struct ItemMapId(Option<u32>);
impl<'de> Deserialize<'de> for ItemMapId {
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

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(match internal {
            Internal::FilledMap { tag } if tag.display.is_none() => Some(tag.map),
            _ => None,
        }))
    }
}

struct PlayerMapIds(HashSet<u32>);
impl<'de> Deserialize<'de> for PlayerMapIds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            ender_items: Vec<ItemMapId>,
            inventory: Vec<ItemMapId>,
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
    let bar = progress_bar(quiet, "Search for map items", length, 64, "players");

    cache.map_ids_by_player.extend(
        players
            .into_par_iter()
            .progress_with(bar.clone())
            .map(|(index, path)| Ok((index, from_bytes::<PlayerMapIds>(&read_gz(&path)?)?.0)))
            .collect::<Result<HashMap<_, _>>>()?,
    );

    bar.finish_and_clear();
    Ok(length)
}

pub fn search_regions(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    cache: &mut Cache,
) -> Result<usize> {
    let regions = glob(world_path.join("region/r.*.mca").to_str().unwrap())?
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
    let bar = progress_bar(quiet, "Search for map items", length, 4, "regions");

    cache.map_ids_by_region.extend(
        regions
            .into_par_iter()
            .progress_with(bar.clone())
            .map(|(position, path)| {
                let mut map_ids = HashSet::new();

                fastanvil::RegionBuffer::new(File::open(&path)?)
                    .for_each_chunk(|_, _, nbt| {
                        map_ids.extend(from_bytes::<ChunkMapIds>(nbt).unwrap().0);
                    })
                    .unwrap_or_default();

                Ok((position, map_ids))
            })
            .collect::<Result<HashMap<_, _>>>()?,
    );

    bar.finish_and_clear();
    Ok(length)
}
