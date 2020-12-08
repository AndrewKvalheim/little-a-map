use crate::banner::Banner;
use crate::map::Map;
use crate::tile::Tile;
use crate::utilities::progress_bar;
use anyhow::{anyhow, Result};
use fastnbt::anvil;
use fastnbt::nbt::{self, Error, Parser, Tag, Value};
use filetime::FileTime;
use flate2::read::GzDecoder;
use glob::glob;
use indicatif::ParallelProgressIterator;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use semver::Version;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fs::{self, File};
use std::path::PathBuf;

pub type Bounds = ((i32, i32), (i32, i32));

pub type MapData = [i8; 128 * 128];

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

pub struct Level {
    pub spawn_x: i32,
    pub spawn_z: i32,
    pub version: Version,
}

#[derive(Deserialize)]
struct NBTName {
    text: String,
}

fn err(error: nbt::Error) -> anyhow::Error {
    match error {
        nbt::Error::EOF => anyhow!("nbt::Error::EOF"),
        nbt::Error::InvalidName => anyhow!("nbt::Error::InvalidName"),
        nbt::Error::InvalidTag(i) => anyhow!("nbt::Error::InvalidTag({})", i),
        nbt::Error::IO(e) => anyhow!(e),
        nbt::Error::ShortRead => anyhow!("nbt::Error::ShortRead"),
    }
}

pub fn read_level(level_path: &PathBuf) -> Result<Level> {
    let file = File::open(&level_path.join("level.dat"))?;
    let decoder = GzDecoder::new(file);
    let mut parser = Parser::new(decoder);

    let mut version: Option<String> = None;
    let mut x: Option<i32> = None;
    let mut z: Option<i32> = None;

    'file: loop {
        match parser.next().map_err(err)? {
            Value::Compound(Some(n)) if n == "" => loop {
                match parser.next().map_err(err)? {
                    Value::Compound(Some(n)) if n == "Data" => loop {
                        match parser.next().map_err(err)? {
                            Value::Int(Some(n), v) if n == "SpawnX" => x = Some(v),
                            Value::Int(Some(n), v) if n == "SpawnZ" => z = Some(v),
                            Value::Compound(Some(n)) if n == "Version" => 'version: loop {
                                match parser.next().map_err(err)? {
                                    Value::String(Some(n), v) if n == "Name" => version = Some(v),
                                    Value::CompoundEnd => break 'version,
                                    _ => {}
                                }
                            },
                            Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
                            _ => {}
                        }

                        if x.is_some() && z.is_some() && version.is_some() {
                            break 'file;
                        }
                    },
                    Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
                    _ => {}
                };
            },
            Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
            _ => {}
        }
    }

    Ok(Level {
        spawn_x: x.unwrap(),
        spawn_z: z.unwrap(),
        version: Version::parse(&version.unwrap())?,
    })
}

pub fn load_map(level_path: &PathBuf, id: u32) -> Result<MapData> {
    let map_file = File::open(&level_path.join(format!("data/map_{}.dat", id)))?;
    let decoder = GzDecoder::new(map_file);
    let mut parser = Parser::new(decoder);

    loop {
        match parser.next().map_err(err)? {
            Value::ByteArray(Some(n), v) if n == "colors" => {
                return v
                    .try_into()
                    .map_err(|_| anyhow!("unexpected data in map #{}", id));
            }
            _ => {}
        };
    }
}

pub fn scan_maps<M, B>(
    level_path: &PathBuf,
    ids: impl IntoIterator<Item = u32>,
    mut on_map: M,
    mut on_banner: B,
) -> Result<()>
where
    B: FnMut(FileTime, Banner),
    M: FnMut(Map),
{
    let data_path = level_path.join("data");

    ids.into_iter().try_for_each(|id| {
        let path = data_path.join(format!("map_{}.dat", id));

        let modified = FileTime::from_last_modification_time(&fs::metadata(&path)?);

        let mut parser = Parser::new(GzDecoder::new(File::open(&path)?));

        let mut scale: Option<i8> = None;
        let mut x: Option<i32> = None;
        let mut z: Option<i32> = None;
        let mut overworld: Option<bool> = None;
        let mut unlimited_tracking: Option<bool> = None;
        let mut added_banners = false;

        'file: loop {
            match parser.next() {
                Err(Error::EOF) => break 'file,
                Err(e) => panic!(e),
                Ok(value) => match value {
                    Value::Compound(Some(n)) if n == "" => loop {
                        match parser.next() {
                            Err(Error::EOF) => break 'file,
                            Err(e) => panic!(e),
                            Ok(value) => match value {
                                Value::Compound(Some(n)) if n == "data" => loop {
                                    match parser.next() {
                                        Err(Error::EOF) => break 'file,
                                        Err(e) => panic!(e),
                                        Ok(value) => {
                                            match value {
                                                // Short-circuit
                                                Value::Int(Some(n), v) if n == "dimension" => {
                                                    if v == 0 {
                                                        overworld = Some(true);
                                                    } else {
                                                        break 'file;
                                                    }
                                                }
                                                Value::String(Some(n), v) if n == "dimension" => {
                                                    if v == "minecraft:overworld" {
                                                        overworld = Some(true);
                                                    } else {
                                                        break 'file;
                                                    }
                                                }

                                                // Collect
                                                Value::Byte(Some(n), v) if n == "scale" => scale = Some(v),
                                                Value::Byte(Some(n), v) if n == "unlimitedTracking" => {
                                                    unlimited_tracking = Some(v == 1)
                                                }
                                                Value::Int(Some(n), v) if n == "xCenter" => x = Some(v),
                                                Value::Int(Some(n), v) if n == "zCenter" => z = Some(v),

                                                Value::List(Some(n), Tag::Compound, _) if n == "banners" => {
                                                    'banners: loop {
                                                        match parser.next().map_err(err)? {
                                                            Value::Compound(None) => {
                                                                let mut x: Option<i32> = None;
                                                                let mut z: Option<i32> = None;
                                                                let mut color: Option<String> = None;
                                                                let mut label: Option<String> = None;

                                                                'banner: loop {
                                                                    match parser.next().map_err(err)? {
                                                                        Value::String(Some(n), v) if n == "Color" => color = Some(v),
                                                                        Value::String(Some(n), v) if n == "Name" => {
                                                                            label = Some(serde_json::from_str::<NBTName>(&v)?.text)
                                                                        }
                                                                        Value::Compound(Some(n)) if n == "Pos" => {
                                                                            'position: loop {
                                                                                match parser.next().map_err(err)? {
                                                                                    // Collect
                                                                                    Value::Int(Some(n), v) if n == "X" => x = Some(v),
                                                                                    Value::Int(Some(n), v) if n == "Z" => z = Some(v),

                                                                                    // End
                                                                                    Value::CompoundEnd => break 'position,

                                                                                    // Skip
                                                                                    _ => {}
                                                                                }
                                                                            }
                                                                        }

                                                                        // End
                                                                        Value::CompoundEnd => break 'banner,

                                                                        // Skip
                                                                        _ => {}
                                                                    }
                                                                }

                                                                let color = color.unwrap();
                                                                let x = x.unwrap();
                                                                let z = z.unwrap();

                                                                on_banner(modified, Banner { color, label, x, z });
                                                            }

                                                            // End
                                                            Value::ListEnd => break 'banners,

                                                            // Skip
                                                            _ => {}
                                                        }
                                                    }

                                                    added_banners = true;
                                                }

                                                // Skip
                                                Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
                                                _ => {}
                                            };
                                            if overworld.is_some()
                                                && unlimited_tracking.is_some()
                                                && scale.is_some()
                                                && x.is_some()
                                                && z.is_some()
                                                && added_banners
                                            {
                                                break 'file;
                                            }
                                        }
                                    }
                                },
                                Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
                                _ => {}
                            }
                        }
                    },
                    Value::Compound(_) => nbt::skip_compound(&mut parser).map_err(err)?,
                    _ => {}
                }
            }
        }

        if let (Some(true), Some(false), Some(scale), Some(x), Some(z)) =
            (overworld, unlimited_tracking, scale, x, z)
        {
            let tile = Tile::from_position(scale, x, z);

            on_map(Map { id, modified, tile });
        }

        Ok(())
    })
}

pub fn scan_players(
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
            let mut map_ids: HashSet<u32> = HashSet::new();

            let mut parser = Parser::new(GzDecoder::new(File::open(&path)?));

            let mut sections_scanned = 0;

            'file: loop {
                match parser.next().unwrap() {
                    Value::Compound(Some(n)) if n == "" => loop {
                        match parser.next().unwrap() {
                            Value::List(Some(n), _, _) if n == "EnderItems" || n == "Inventory" => {
                                let mut list_depth = 1;

                                while list_depth > 0 {
                                    match parser.next().unwrap() {
                                        Value::Compound(Some(n)) if n == "tag" => {
                                            let mut cpd_depth = 1;

                                            while cpd_depth > 0 {
                                                match parser.next().unwrap() {
                                                    Value::Int(Some(n), v) if n == "map" => {
                                                        map_ids.insert(v as u32);
                                                    }
                                                    Value::CompoundEnd => cpd_depth -= 1,
                                                    Value::Compound(_) => cpd_depth += 1,
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Value::ListEnd => list_depth -= 1,
                                        Value::List(_, _, _) => list_depth += 1,
                                        _ => {}
                                    }
                                }

                                sections_scanned += 1;

                                if sections_scanned == 2 {
                                    break 'file;
                                }
                            }
                            Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                            _ => {}
                        }
                    },
                    Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                    _ => {}
                }
            }

            Ok((uuid, map_ids))
        })
        .collect()
}

pub fn scan_regions(
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

    *count_regions += length;

    regions
        .into_par_iter()
        .progress_with(progress_bar(
            hidden,
            "Search for map items",
            length,
            "regions",
        ))
        .map(|(position, path)| {
            let mut map_ids: HashSet<u32> = HashSet::new();

            let on_chunk = |_x: usize, _z: usize, data: &Vec<u8>| {
                let mut parser = nbt::Parser::new(data.as_slice());

                let mut sections_scanned = 0;

                'chunk: loop {
                    match parser.next().unwrap() {
                        Value::Compound(Some(n)) if n == "" => loop {
                            match parser.next().unwrap() {
                                Value::Compound(Some(n)) if n == "Level" => loop {
                                    match parser.next().unwrap() {
                                        Value::List(Some(n), _, _)
                                            if n == "Entities" || n == "TileEntities" =>
                                        {
                                            let mut list_depth = 1;

                                            while list_depth > 0 {
                                                match parser.next().unwrap() {
                                                    Value::Compound(Some(n)) if n == "tag" => {
                                                        let mut cpd_depth = 1;

                                                        while cpd_depth > 0 {
                                                            match parser.next().unwrap() {
                                                                Value::Int(Some(n), v)
                                                                    if n == "map" =>
                                                                {
                                                                    map_ids.insert(v as u32);
                                                                }
                                                                Value::CompoundEnd => {
                                                                    cpd_depth -= 1
                                                                }
                                                                Value::Compound(_) => {
                                                                    cpd_depth += 1
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                    }
                                                    Value::ListEnd => list_depth -= 1,
                                                    Value::List(_, _, _) => list_depth += 1,
                                                    _ => {}
                                                }
                                            }

                                            sections_scanned += 1;

                                            if sections_scanned == 2 {
                                                break 'chunk;
                                            }
                                        }
                                        Value::Compound(_) => {
                                            nbt::skip_compound(&mut parser).unwrap()
                                        }
                                        _ => {}
                                    }
                                },
                                Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                                _ => {}
                            }
                        },
                        Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                        _ => {}
                    }
                }
            };

            anvil::Region::new(File::open(&path)?)
                .for_each_chunk(on_chunk)
                .unwrap_or_default();

            Ok((position, map_ids))
        })
        .collect()
}
