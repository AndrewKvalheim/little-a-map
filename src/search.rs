#![allow(clippy::module_name_repetitions)]

use crate::cache::{Cache, IdsBy};
use crate::utilities::{progress_bar, read_gz};
use anyhow::{Context, Result};
use fastnbt::from_bytes;
use glob::glob;
use indicatif::ParallelProgressIterator;
use itertools::Itertools;
use log::{debug, log_enabled, Level::Debug};
use rayon::prelude::*;
use serde::{de::DeserializeOwned, de::IgnoredAny, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::iter;
use std::path::Path;
use std::string::ToString;

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
            item: Option<MapIdsOfItem>,
            items: Option<Vec<MapIdsOfItem>>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .items
                .into_iter()
                .flatten()
                .chain(internal.item)
                .flat_map(|i| i.0)
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

struct MapIdsOfItem(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "id")]
        enum Internal {
            #[serde(rename = "minecraft:shulker_box")]
            BlockEntity { tag: BlockEntityTag },

            #[serde(rename = "minecraft:filled_map")]
            FilledMap { tag: FilledMapTag },

            #[serde(other)]
            Other,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct BlockEntityTag {
            block_entity_tag: MapIdsOfEntity,
        }

        #[derive(Deserialize)]
        struct FilledMapTag {
            display: Option<IgnoredAny>,
            map: u32,
        }

        Ok(Self(match Internal::deserialize(deserializer)? {
            Internal::BlockEntity { tag } => tag.block_entity_tag.0.into_iter().collect(),
            Internal::FilledMap { tag } if tag.display.is_none() => iter::once(tag.map).collect(),
            _ => HashSet::default(),
        }))
    }
}

struct MapIdsOfLevelChunk(HashSet<u32>);
impl<'de> Deserialize<'de> for MapIdsOfLevelChunk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Chunk {
            V117(V117),
            V118(V118),
        }

        #[derive(serde_query::Deserialize)]
        struct V117 {
            #[query(".Level.TileEntities")]
            block_entities: Vec<MapIdsOfEntity>,
        }

        #[derive(Deserialize)]
        struct V118 {
            block_entities: Vec<MapIdsOfEntity>,
        }

        let entities = match Chunk::deserialize(deserializer)? {
            Chunk::V117(c) => c.block_entities,
            Chunk::V118(c) => c.block_entities,
        };
        Ok(Self(entities.into_iter().flat_map(|e| e.0).collect()))
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
            ender_items: Vec<MapIdsOfItem>,
            inventory: Vec<MapIdsOfItem>,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(
            internal
                .ender_items
                .into_iter()
                .chain(internal.inventory)
                .flat_map(|i| i.0)
                .collect(),
        ))
    }
}

fn search_regions<T: ContainsMapIds + DeserializeOwned>(
    world_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    cache: &Cache,
    pattern: &str,
) -> Result<(usize, IdsBy<(i32, i32)>)> {
    let regions = glob(world_path.join(pattern).to_str().unwrap())?
        .map(|entry| {
            let path = entry?;
            let base = path.file_stem().unwrap().to_str().unwrap();
            let mut parts = base.split('.').skip(1);
            let x = parts.next().unwrap().parse()?;
            let z = parts.next().unwrap().parse()?;

            Ok(match bounds {
                Some(&((x0, z0), (x1, z1))) if x < x0 || x > x1 || z < z0 || z > z1 => None,
                _ => cache.is_expired_for(&path)?.then_some(((x, z), path)),
            })
        })
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>>>()?;

    let length = regions.len();
    let bar = progress_bar(quiet, "Search for map items", length, "regions");

    let map_ids_by_region = regions
        .into_par_iter()
        .progress_with(bar.clone())
        .map(|((rx, rz), path)| {
            let mut in_region = HashSet::new();

            match fastanvil::Region::from_stream(File::open(&path)?) {
                Ok(mut region) => {
                    for chunk in region.iter() {
                        let fastanvil::ChunkData { data, x, z } = chunk?;

                        let in_chunk = from_bytes::<T>(&data)
                            .with_context(|| {
                                format!("Failed to deserialize {} chunk ({x}, {z})", path.display())
                            })
                            .unwrap()
                            .map_ids();

                        if log_enabled!(Debug) && !in_chunk.is_empty() {
                            let list = in_chunk.iter().sorted().map(ToString::to_string).join(", ");
                            bar.suspend(|| {
                                debug!("Region ({rx}, {rz}) chunk ({x}, {z}) maps: {list}");
                            });
                        }

                        in_region.extend(in_chunk);
                    }
                }
                Err(fastanvil::Error::IO(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        && fs::metadata(&path)?.len() == 0 => {}
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("Failed to deserialize {}", path.display()))
                }
            }

            Ok(((rx, rz), in_region))
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
        .map(|(index, path)| Ok(cache.is_expired_for(&path)?.then_some((index, path))))
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>>>()?;

    let length = players.len();
    let bar = progress_bar(quiet, "Search for map items", length, "players");
    let ids = players
        .into_par_iter()
        .progress_with(bar.clone())
        .map(|(index, path)| {
            let ids = from_bytes::<MapIdsOfPlayer>(&read_gz(&path)?)
                .with_context(|| format!("Failed to deserialize {}", path.display()))?
                .0;

            if log_enabled!(Debug) && !ids.is_empty() {
                let list = ids.iter().sorted().map(ToString::to_string).join(", ");
                bar.suspend(|| debug!("Player {index} maps: {list}"));
            }

            Ok((index, ids))
        })
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

    cache.map_ids_by_block_region.extend(ids);
    Ok(length)
}
