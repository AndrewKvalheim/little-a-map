#![allow(clippy::module_name_repetitions)]

use crate::banner::Banner;
use crate::tile::Tile;
use crate::utilities::read_gz;
use anyhow::Result;
use derivative::Derivative;
use fastnbt::de::from_bytes;
use filetime::FileTime;
use rayon::prelude::*;
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::TryInto;
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(PartialEq)]
enum Dimension {
    Nether,
    Overworld,
    End,
}
impl<'de> Deserialize<'de> for Dimension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DimensionVisitor;

        impl<'de> Visitor<'de> for DimensionVisitor {
            type Value = Dimension;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("integer or string")
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                match value {
                    -1 => Ok(Dimension::Nether),
                    0 => Ok(Dimension::Overworld),
                    1 => Ok(Dimension::End),
                    _ => Err(E::invalid_value(Unexpected::Signed(value), &self)),
                }
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "minecraft:the_nether" => Ok(Dimension::Nether),
                    "minecraft:overworld" => Ok(Dimension::Overworld),
                    "minecraft:the_end" => Ok(Dimension::End),
                    _ => Err(E::invalid_value(Unexpected::Str(value), &self)),
                }
            }
        }

        deserializer.deserialize_any(DimensionVisitor)
    }
}

#[derive(Debug, Derivative, Eq)]
#[derivative(Ord, PartialEq, PartialOrd)]
pub struct Map {
    pub modified: FileTime,

    pub id: u32,

    #[derivative(Ord = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(PartialOrd = "ignore")]
    pub tile: Tile,
}

pub struct MapData(pub [u8; 128 * 128]);
impl<'de> Deserialize<'de> for MapData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Internal<'a> {
            #[serde(borrow)]
            data: Data<'a>,
        }

        #[derive(Deserialize)]
        struct Data<'a> {
            #[serde(borrow)]
            colors: &'a [u8],
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self(internal.data.colors.try_into().map_err(|_| {
            de::Error::invalid_value(
                Unexpected::Bytes(internal.data.colors),
                &"array of 128 × 128 indexed-color pixels",
            )
        })?))
    }
}
impl MapData {
    pub fn from_world_path(world_path: &Path, id: u32) -> Result<Self> {
        Ok(from_bytes(&read_gz(
            &world_path.join(format!("data/map_{}.dat", id)),
        )?)?)
    }
}

#[derive(Default)]
pub struct MapScan {
    pub banners: BTreeSet<Banner>,
    pub banners_modified: Option<FileTime>,
    pub maps_by_tile: HashMap<Tile, BTreeSet<Map>>,
    pub root_tiles: HashSet<Tile>,
}
impl MapScan {
    pub fn run(world_path: &Path, ids: HashSet<u32>) -> Result<Self> {
        enum Meta {
            Normal { banners: Vec<Banner>, tile: Tile },
            Other,
        }
        impl<'de> Deserialize<'de> for Meta {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(serde_query::Deserialize)]
                struct Internal {
                    #[query(".data.banners")]
                    banners: Vec<Banner>,
                    #[query(".data.dimension")]
                    dimension: Dimension,
                    #[query(".data.scale")]
                    scale: u8,
                    #[query(".data.xCenter")]
                    x: i32,
                    #[query(".data.zCenter")]
                    z: i32,
                }
                let internal = Internal::deserialize(deserializer)?;
                if internal.dimension == Dimension::Overworld {
                    Ok(Self::Normal {
                        banners: internal.banners,
                        tile: Tile::from_position(internal.scale, internal.x, internal.z),
                    })
                } else {
                    Ok(Self::Other)
                }
            }
        }

        let data_path = world_path.join("data");

        ids.into_par_iter()
            .map(move |id| -> Result<Self> {
                let path = data_path.join(format!("map_{}.dat", id));
                let mut results = Self::default();

                if let Meta::Normal { banners, tile } = from_bytes(&read_gz(&path)?)? {
                    let modified = FileTime::from_last_modification_time(&fs::metadata(&path)?);

                    results.root_tiles.insert(tile.root());
                    if !banners.is_empty() {
                        results.banners_modified.replace(modified);
                    }
                    results.banners.extend(banners);
                    results
                        .maps_by_tile
                        .entry(tile.clone())
                        .or_insert_with(BTreeSet::new)
                        .insert(Map { id, modified, tile });
                }

                Ok(results)
            })
            .try_reduce(Self::default, |mut results, other| {
                if let Some(b) = other.banners_modified {
                    if results.banners_modified.map_or(true, |a| a < b) {
                        results.banners_modified.replace(b);
                    }
                }
                results.root_tiles.extend(other.root_tiles);
                for (tile, other_maps) in other.maps_by_tile {
                    results
                        .maps_by_tile
                        .entry(tile)
                        .or_insert_with(BTreeSet::new)
                        .extend(other_maps);
                }
                results.banners.extend(other.banners);

                Ok(results)
            })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn compare() {
        fn map(id: u32, s: i64, x: i32) -> Map {
            Map {
                id,
                modified: FileTime::from_unix_time(s, 0),
                tile: Tile::new(0, x, 0),
            }
        }

        // Identical
        assert_eq!(map(0, 0, 0), map(0, 0, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 0, 0)), Equal);

        // Ignore tile
        assert_eq!(map(0, 0, 0), map(0, 0, 1));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 0, 1)), Equal);

        // Differ by ID
        assert_ne!(map(0, 0, 0), map(1, 0, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(1, 0, 0)), Less);

        // Differ by modification time
        assert_ne!(map(0, 0, 0), map(0, 1, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 1, 0)), Less);

        // Sort first by modification time, then by ID
        assert_eq!(map(0, 1, 0).cmp(&map(1, 0, 0)), Greater);
        assert_eq!(map(1, 0, 0).cmp(&map(0, 1, 0)), Less);
    }
}
