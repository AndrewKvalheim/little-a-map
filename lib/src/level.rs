use crate::banner::Banner;
use crate::map::Map;
use crate::tile::Tile;
use crate::utilities::progress_bar;
use anyhow::{anyhow, Result};
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
use std::path::PathBuf;

pub type Bounds = ((i32, i32), (i32, i32));

pub type MapData = [u8; 128 * 128];

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

#[derive(serde_query::Deserialize)]
pub struct Level {
    #[query(".Data.SpawnX")]
    pub spawn_x: i32,
    #[query(".Data.SpawnZ")]
    pub spawn_z: i32,
    #[query(".Data.Version.Name")]
    pub version: Version,
}

#[derive(Default)]
pub struct MapScan {
    pub banners: BTreeSet<Banner>,
    pub banners_modified: Option<FileTime>,
    pub maps_by_tile: HashMap<Tile, BTreeSet<Map>>,
    pub root_tiles: HashSet<Tile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTBanner {
    color: String,
    #[serde(default)]
    #[serde(with = "serde_with::json::nested")]
    name: Option<NBTBannerName>,
    pos: NBTBannerPos,
}

#[derive(Deserialize)]
struct NBTBannerName {
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTBannerPos {
    x: i32,
    z: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTBlockEntity {
    items: Option<Vec<NBTItem>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTChunk {
    level: NBTChunkLevel,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTChunkLevel {
    entities: Vec<NBTEntity>,
    tile_entities: Vec<NBTBlockEntity>,
}

#[derive(PartialEq)]
enum NBTDimension {
    Nether,
    Overworld,
    End,
}
impl<'de> Deserialize<'de> for NBTDimension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NBTDimension, D::Error> {
        struct NBTDimensionVisitor;

        impl<'de> Visitor<'de> for NBTDimensionVisitor {
            type Value = NBTDimension;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("integer or string")
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                match value {
                    -1 => Ok(NBTDimension::Nether),
                    0 => Ok(NBTDimension::Overworld),
                    1 => Ok(NBTDimension::End),
                    _ => Err(E::invalid_value(Unexpected::Signed(value), &self)),
                }
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "minecraft:the_nether" => Ok(NBTDimension::Nether),
                    "minecraft:overworld" => Ok(NBTDimension::Overworld),
                    "minecraft:the_end" => Ok(NBTDimension::End),
                    _ => Err(E::invalid_value(Unexpected::Str(value), &self)),
                }
            }
        }

        deserializer.deserialize_any(NBTDimensionVisitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTEntity {
    item: Option<NBTItem>,
}

#[derive(Deserialize)]
#[serde(tag = "id")]
enum NBTItem {
    #[serde(rename = "minecraft:filled_map")]
    FilledMap { tag: NBTFilledMapTag },

    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct NBTFilledMapTag {
    map: u32,
}

#[derive(Deserialize)]
struct NBTMap<'a> {
    #[serde(borrow)]
    data: NBTMapData<'a>,
}

#[derive(Deserialize)]
struct NBTMapData<'a> {
    #[serde(borrow)]
    colors: &'a [u8],
}

#[derive(serde_query::Deserialize)]
struct NBTMapMeta {
    #[query(".data.banners")]
    banners: Vec<NBTBanner>,
    #[query(".data.dimension")]
    dimension: NBTDimension,
    #[query(".data.scale")]
    scale: i8,
    #[query(".data.unlimitedTracking")]
    unlimited_tracking: bool,
    #[query(".data.xCenter")]
    x: i32,
    #[query(".data.zCenter")]
    z: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NBTPlayer {
    inventory: Vec<NBTItem>,
    ender_items: Vec<NBTItem>,
}

fn read_gz(path: &PathBuf) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(File::open(&path)?);
    let mut data = Vec::new();

    decoder.read_to_end(&mut data)?;

    Ok(data)
}

pub fn read_level(level_path: &PathBuf) -> Result<Level> {
    Ok(from_bytes(&read_gz(&level_path.join("level.dat"))?)?)
}

pub fn load_map(level_path: &PathBuf, id: u32) -> Result<MapData> {
    from_bytes::<NBTMap>(&read_gz(&level_path.join(format!("data/map_{}.dat", id)))?)?
        .data
        .colors
        .try_into()
        .map_err(|_| anyhow!("unexpected data in map #{}", id))
}

pub fn scan_maps(level_path: &PathBuf, ids: HashSet<u32>) -> Result<MapScan> {
    let data_path = level_path.join("data");

    Ok(ids
        .into_par_iter()
        .map(move |id| -> Result<MapScan> {
            let path = data_path.join(format!("map_{}.dat", id));
            let modified = FileTime::from_last_modification_time(&fs::metadata(&path)?);
            let meta: NBTMapMeta = from_bytes(&read_gz(&path)?)?;
            let mut map_scan = MapScan::default();

            if !meta.unlimited_tracking && meta.dimension == NBTDimension::Overworld {
                let tile = Tile::from_position(meta.scale, meta.x, meta.z);

                map_scan.root_tiles.insert(tile.root());

                if !meta.banners.is_empty() {
                    map_scan.banners_modified.replace(modified);
                }

                map_scan
                    .banners
                    .extend(meta.banners.into_iter().map(|b| Banner {
                        color: b.color,
                        label: b.name.map(|n| n.text),
                        x: b.pos.x,
                        z: b.pos.z,
                    }));

                map_scan
                    .maps_by_tile
                    .entry(tile.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(Map { id, modified, tile });
            }

            Ok(map_scan)
        })
        .try_reduce(MapScan::default, |mut map_scan, other| {
            if let Some(b) = other.banners_modified {
                if map_scan.banners_modified.map_or(true, |a| a < b) {
                    map_scan.banners_modified.replace(b);
                }
            }
            map_scan.root_tiles.extend(other.root_tiles);
            map_scan.maps_by_tile.extend(other.maps_by_tile);
            map_scan.banners.extend(other.banners);

            Ok(map_scan)
        })?)
}

pub fn search_players(
    level_path: &PathBuf,
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

    *count_players += length;

    players
        .into_par_iter()
        .progress_with(progress_bar(
            hidden,
            "Search for map items",
            length,
            "players",
        ))
        .map(|(uuid, path)| {
            let player: NBTPlayer = from_bytes(&read_gz(&path)?)?;

            let map_ids = player
                .inventory
                .into_iter()
                .chain(player.ender_items.into_iter())
                .filter_map(|item| match item {
                    NBTItem::FilledMap { tag } => Some(tag.map),
                    _ => None,
                })
                .collect::<HashSet<_>>();

            Ok((uuid, map_ids))
        })
        .collect()
}

pub fn search_regions(
    level_path: &PathBuf,
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
                .for_each_chunk(|_x, _z, data| {
                    let chunk: NBTChunk = from_bytes(data).unwrap();

                    map_ids.extend(chunk.level.entities.into_iter().filter_map(|entity| {
                        match entity.item {
                            Some(NBTItem::FilledMap { tag }) => Some(tag.map),
                            _ => None,
                        }
                    }));

                    map_ids.extend(chunk.level.tile_entities.into_iter().flat_map(
                        |block_entity| {
                            block_entity.items.into_iter().flat_map(|items| {
                                items.into_iter().filter_map(|item| match item {
                                    NBTItem::FilledMap { tag } => Some(tag.map),
                                    _ => None,
                                })
                            })
                        },
                    ));
                })
                .unwrap_or_default();

            Ok((position, map_ids))
        })
        .collect()
}
