use crate::banner::Banner;
use crate::map::Map;
use crate::tile::Tile;
use crate::utilities::progress_bar;
use anyhow::Result;
use fastnbt::de::from_bytes;
use filetime::FileTime;
use flate2::read::GzDecoder;
use glob::glob;
use indicatif::ParallelProgressIterator;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use semver::Version;
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::TryInto;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Bounds = ((i32, i32), (i32, i32));

const PALETTE_BASE: [[u8; 3]; 59] = [
    [0, 0, 0],
    [127, 178, 56],
    [247, 233, 163],
    [199, 199, 199],
    [255, 0, 0],
    [160, 160, 255],
    [167, 167, 167],
    [0, 124, 0],
    [255, 255, 255],
    [164, 168, 184],
    [151, 109, 77],
    [112, 112, 112],
    [64, 64, 255],
    [143, 119, 72],
    [255, 252, 245],
    [216, 127, 51],
    [178, 76, 216],
    [102, 153, 216],
    [229, 229, 51],
    [127, 204, 25],
    [242, 127, 165],
    [76, 76, 76],
    [153, 153, 153],
    [76, 127, 153],
    [127, 63, 178],
    [51, 76, 178],
    [102, 76, 51],
    [102, 127, 51],
    [153, 51, 51],
    [25, 25, 25],
    [250, 238, 77],
    [92, 219, 213],
    [74, 128, 255],
    [0, 217, 58],
    [129, 86, 49],
    [112, 2, 0],
    [209, 177, 161],
    [159, 82, 36],
    [149, 87, 108],
    [112, 108, 138],
    [186, 133, 36],
    [103, 117, 53],
    [160, 77, 78],
    [57, 41, 35],
    [135, 107, 98],
    [87, 92, 92],
    [122, 73, 88],
    [76, 62, 92],
    [76, 50, 35],
    [76, 82, 42],
    [142, 60, 46],
    [37, 22, 16],
    [189, 48, 49],
    [148, 63, 97],
    [92, 25, 29],
    [22, 126, 134],
    [58, 142, 140],
    [86, 44, 62],
    [20, 180, 133],
];
const PALETTE_FACTORS: [u32; 4] = [180, 220, 255, 135];
pub const TRNS: [u8; PALETTE_FACTORS.len()] = [0; PALETTE_FACTORS.len()];
pub static PALETTE: Lazy<Vec<u8>> = Lazy::new(|| {
    PALETTE_BASE
        .iter()
        .flat_map(|rgb| {
            PALETTE_FACTORS
                .iter()
                .flat_map(move |&f| rgb.iter().map(move |&v| (u32::from(v) * f / 255) as u8))
        })
        .collect()
});

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

#[derive(serde_query::Deserialize)]
pub struct Level {
    #[query(".Data.SpawnX")]
    pub spawn_x: i32,
    #[query(".Data.SpawnZ")]
    pub spawn_z: i32,
    #[query(".Data.Version.Name")]
    pub version: Version,
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

enum MapMeta {
    Normal { banners: Vec<Banner>, tile: Tile },
    Other,
}
impl<'de> Deserialize<'de> for MapMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde_query::Deserialize)]
        struct Internal {
            #[query(".data.banners")]
            banners: Vec<Banner>,
            #[query(".data.dimension")]
            dimension: Dimension,
            #[query(".data.scale")]
            scale: u8,
            #[query(".data.unlimitedTracking")]
            unlimited_tracking: bool,
            #[query(".data.xCenter")]
            x: i32,
            #[query(".data.zCenter")]
            z: i32,
        }

        let internal = Internal::deserialize(deserializer)?;
        if !internal.unlimited_tracking && internal.dimension == Dimension::Overworld {
            Ok(Self::Normal {
                banners: internal.banners,
                tile: Tile::from_position(internal.scale, internal.x, internal.z),
            })
        } else {
            Ok(Self::Other)
        }
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

#[derive(Default)]
pub struct ScanResults {
    pub banners: BTreeSet<Banner>,
    pub banners_modified: Option<FileTime>,
    pub maps_by_tile: HashMap<Tile, BTreeSet<Map>>,
    pub root_tiles: HashSet<Tile>,
}

fn read_gz(path: &Path) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(File::open(&path)?);
    let mut data = Vec::new();

    decoder.read_to_end(&mut data)?;

    Ok(data)
}

pub fn load_map(path: &Path, id: u32) -> Result<MapData> {
    Ok(from_bytes(&read_gz(
        &path.join(format!("data/map_{}.dat", id)),
    )?)?)
}

pub fn read_level(path: &Path) -> Result<Level> {
    Ok(from_bytes(&read_gz(&path.join("level.dat"))?)?)
}

pub fn scan_maps(level_path: &Path, ids: HashSet<u32>) -> Result<ScanResults> {
    let data_path = level_path.join("data");

    ids.into_par_iter()
        .map(move |id| -> Result<ScanResults> {
            let path = data_path.join(format!("map_{}.dat", id));
            let mut results = ScanResults::default();

            if let MapMeta::Normal { banners, tile } = from_bytes(&read_gz(&path)?)? {
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
        .try_reduce(ScanResults::default, |mut results, other| {
            if let Some(b) = other.banners_modified {
                if results.banners_modified.map_or(true, |a| a < b) {
                    results.banners_modified.replace(b);
                }
            }
            results.root_tiles.extend(other.root_tiles);
            results.maps_by_tile.extend(other.maps_by_tile);
            results.banners.extend(other.banners);

            Ok(results)
        })
}

pub fn search_players(
    level_path: &Path,
    quiet: bool,
    count_players: &mut usize,
) -> Result<HashMap<String, HashSet<u32>>> {
    let players = glob(
        level_path
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
    level_path: &Path,
    quiet: bool,
    bounds: Option<&Bounds>,
    count_regions: &mut usize,
) -> Result<HashMap<(i32, i32), HashSet<u32>>> {
    let regions = glob(level_path.join("region/r.*.mca").to_str().unwrap())?
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
