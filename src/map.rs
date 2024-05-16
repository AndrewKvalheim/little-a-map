#![allow(clippy::module_name_repetitions)]
#![allow(clippy::non_canonical_partial_ord_impl)] // Pending mcarton/rust-derivative#115

use crate::banner::Banner;
use crate::palette::{PALETTE, PALETTE_LEN};
use crate::tile::Tile;
use crate::utilities::read_gz;
use anyhow::{Context, Result};
use derivative::Derivative;
use fastnbt::from_bytes;
use itertools::Itertools;
use log::{debug, log_enabled, Level::Debug};
use rayon::prelude::*;
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;
use std::time::SystemTime;

#[derive(PartialEq)]
enum Dimension {
    Nether,
    Overworld,
    End,
}
impl<'de> Deserialize<'de> for Dimension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DimensionVisitor;

        impl Visitor<'_> for DimensionVisitor {
            type Value = Dimension;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    pub modified: SystemTime,

    pub id: u32,

    #[derivative(Ord = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(PartialOrd = "ignore")]
    pub tile: Tile,
}

impl Map {
    pub fn render(&self, output_path: &Path, data: &mut MapData, force: bool) -> Result<bool> {
        let dir_path = output_path.join("maps");
        let png_path = dir_path.join(self.id.to_string()).with_extension("png");

        if !force
            && fs::metadata(&png_path)
                .and_then(|m| m.modified())
                .map_or(false, |meta_modified| meta_modified >= self.modified)
        {
            return Ok(false);
        }

        let mut color_map = HashMap::with_capacity(PALETTE_LEN);
        let mut palette = Vec::with_capacity(PALETTE_LEN * 3);
        for color in &mut data.0 {
            let next = color_map.len();
            *color = *color_map.entry(*color).or_insert_with(|| {
                let (i, j) = (*color as usize * 3, u8::try_from(next).unwrap());
                palette.extend(&PALETTE[i..i + 3]);
                j
            });
        }

        fs::create_dir_all(&dir_path)?;
        let png_file = File::create(png_path)?;
        let mut encoder = png::Encoder::new(BufWriter::new(&png_file), 128, 128);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_compression(png::Compression::Best);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_filter(png::FilterType::NoFilter);
        encoder.set_palette(palette);
        encoder.write_header()?.write_image_data(&data.0)?;
        png_file.set_modified(self.modified)?;

        Ok(true)
    }
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
        let path = world_path.join(format!("data/map_{id}.dat"));

        from_bytes(&read_gz(&path)?)
            .with_context(|| format!("Failed to deserialize {}", path.display()))
    }
}

#[derive(Default)]
pub struct MapScan {
    pub banners: BTreeSet<Banner>,
    pub banners_modified: Option<SystemTime>,
    pub maps_by_tile: HashMap<Tile, BTreeSet<Map>>,
    pub map_ids_by_banner_position: HashMap<(i32, i32), BTreeSet<u32>>,
    pub root_tiles: HashSet<Tile>,
}
impl MapScan {
    pub fn run(world_path: &Path, ids: &HashSet<u32>) -> Result<Self> {
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
            .map(move |&id| -> Result<Self> {
                let path = data_path.join(format!("map_{id}.dat"));
                let mut results = Self::default();

                if let Meta::Normal { banners, tile } = from_bytes(&read_gz(&path)?)
                    .with_context(|| format!("Failed to deserialize {}", path.display()))?
                {
                    let modified = fs::metadata(&path)?.modified()?;

                    results.root_tiles.insert(tile.root());
                    if !banners.is_empty() {
                        results.banners_modified.replace(modified);

                        if log_enabled!(Debug) {
                            let list = banners
                                .iter()
                                .sorted()
                                .map(|Banner { x, z, .. }| format!("({x}, {z})",))
                                .join(", ");
                            debug!("Map {id} banners: {list}");
                        }
                    }
                    for banner in &banners {
                        results
                            .map_ids_by_banner_position
                            .entry((banner.x, banner.z))
                            .or_default()
                            .insert(id);
                    }
                    results.banners.extend(banners);
                    results
                        .maps_by_tile
                        .entry(tile.clone())
                        .or_default()
                        .insert(Map { modified, id, tile });
                } else {
                    debug!("Ignoring map {id}");
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
                        .or_default()
                        .extend(other_maps);
                }
                for (position, other_ids) in other.map_ids_by_banner_position {
                    results
                        .map_ids_by_banner_position
                        .entry(position)
                        .or_default()
                        .extend(other_ids);
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
    use std::time::Duration;

    #[test]
    fn compare() {
        fn map(id: u32, s: u64, x: i32) -> Map {
            Map {
                id,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(s),
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
