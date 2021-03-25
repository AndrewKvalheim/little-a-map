#![allow(clippy::module_name_repetitions)]

use crate::utilities::{progress_bar, read_gz};
use anyhow::Result;
use fastnbt::de::from_bytes;
use glob::glob;
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

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
            map: u32,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(match internal {
            Internal::FilledMap { tag } => Some(tag.map),
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

pub fn search_players(
    world_path: &Path,
    quiet: bool,
    count_players: &mut usize,
) -> Result<HashMap<String, HashSet<u32>>> {
    let players = glob(
        world_path
            .join("playerdata/????????-????-????-????-????????????.dat")
            .to_str()
            .unwrap(),
    )?
    .map(|entry| -> Result<(String, PathBuf)> {
        let path = entry?;

        let uuid = path.file_stem().unwrap().to_str().unwrap().to_string();

        Ok((uuid, path))
    })
    .collect::<Result<Vec<_>>>()?;

    let length = players.len();
    let hidden = quiet || length < 10;
    let message = "Search for map items";

    *count_players += length;

    players
        .into_par_iter()
        .progress_with(progress_bar(hidden, message, length, "players"))
        .map(|(uuid, path)| Ok((uuid, from_bytes::<PlayerMapIds>(&read_gz(&path)?)?.0)))
        .collect()
}

pub fn search_regions(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    count_regions: &mut usize,
) -> Result<HashMap<(i32, i32), HashSet<u32>>> {
    let regions = glob(world_path.join("region/r.*.mca").to_str().unwrap())?
        .map(|entry| -> Result<((i32, i32), PathBuf)> {
            let path = entry?;

            let mut parts = path
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .split('.')
                .skip(1);
            let x = parts.next().unwrap().parse::<i32>()?;
            let z = parts.next().unwrap().parse::<i32>()?;

            Ok(((x, z), path))
        })
        .filter(|region| {
            region.as_ref().map_or(true, |((x, z), _)| {
                bounds.map_or(true, |((x0, z0), (x1, z1))| {
                    x0 <= x && x <= x1 && z0 <= z && z <= z1
                })
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let length = regions.len();
    let hidden = quiet || length < 3;
    let message = "Search for map items";

    *count_regions += length;

    regions
        .into_par_iter()
        .progress_with(progress_bar(hidden, message, length, "regions"))
        .map(|(position, path)| {
            let mut map_ids: HashSet<u32> = HashSet::new();

            fastanvil::Region::new(File::open(&path)?)
                .for_each_chunk(|_x, _z, nbt| {
                    map_ids.extend(from_bytes::<ChunkMapIds>(nbt).unwrap().0);
                })
                .unwrap_or_default();

            Ok((position, map_ids))
        })
        .collect()
}
