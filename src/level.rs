use crate::banner::Banner;
use crate::map::Map;
use crate::tile::Tile;
use fastnbt::nbt::{self, Error, Parser, Tag, Value};
use filetime::FileTime;
use flate2::read::GzDecoder;
use glob::glob;
use lazy_static::lazy_static;
use serde::Deserialize;
use std::fs::{self, File};
use std::path::PathBuf;

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
lazy_static! {
    pub static ref PALETTE: Vec<u8> = {
        PALETTE_BASE
            .iter()
            .flat_map(|rgb| {
                PALETTE_FACTORS
                    .iter()
                    .flat_map(move |&f| rgb.iter().map(move |&v| (u32::from(v) * f / 255) as u8))
            })
            .collect()
    };
}

#[derive(Deserialize)]
struct NBTName {
    text: String,
}

pub fn get_spawn(level_path: &PathBuf) -> (i32, i32) {
    let file = File::open(&level_path.join("level.dat")).unwrap();
    let decoder = GzDecoder::new(file);
    let mut parser = Parser::new(decoder);

    let mut x: Option<i32> = None;
    let mut z: Option<i32> = None;

    parser.next().unwrap();
    'file: loop {
        match parser.next() {
            Err(error) => panic!(error),
            Ok(value) => {
                match value {
                    Value::Compound(Some(ref n)) if n == "Data" => loop {
                        match parser.next() {
                            Err(error) => panic!(error),
                            Ok(value) => {
                                match value {
                                    Value::Int(Some(ref n), v) if n == "SpawnX" => x = Some(v),
                                    Value::Int(Some(ref n), v) if n == "SpawnZ" => z = Some(v),
                                    Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                                    _ => {}
                                };
                            }
                        }

                        if x.is_some() && z.is_some() {
                            break 'file;
                        }
                    },
                    Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
                    _ => {}
                };
            }
        }
    }

    (x.unwrap(), z.unwrap())
}

pub fn load_map(level_path: &PathBuf, id: u32) -> MapData {
    let map_file = File::open(&level_path.join(format!("data/map_{}.dat", id))).unwrap();
    let decoder = GzDecoder::new(map_file);
    let mut parser = Parser::new(decoder);

    let mut pixels = [0; 128 * 128];

    loop {
        match parser.next() {
            Err(error) => panic!(error),
            Ok(value) => {
                match value {
                    Value::ByteArray(Some(ref n), v) if n == "colors" => {
                        pixels.copy_from_slice(&v);

                        return pixels;
                    }
                    _ => {}
                };
            }
        }
    }
}

pub fn scan<M, B>(level_path: &PathBuf, mut on_map: M, mut on_banner: B)
where
    B: FnMut(FileTime, Banner),
    M: FnMut(Map),
{
    glob(level_path.join("data/map_*.dat").to_str().unwrap())
        .unwrap()
        .for_each(|entry| {
            let map_path = entry.unwrap();

            let modified = FileTime::from_last_modification_time(&fs::metadata(&map_path).unwrap());

            let id = map_path
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .rsplit('_')
                .next()
                .unwrap()
                .parse::<u32>()
                .unwrap();

            let map_file = File::open(&map_path).unwrap();
            let decoder = GzDecoder::new(map_file);
            let mut parser = Parser::new(decoder);

            let mut scale: Option<i8> = None;
            let mut x: Option<i32> = None;
            let mut z: Option<i32> = None;
            let mut overworld: Option<bool> = None;
            let mut unlimited_tracking: Option<bool> = None;
            let mut added_banners = false;

            'file: loop {
                match parser.next() {
                    Err(error) => match error {
                        Error::EOF => break 'file,
                        _ => panic!(error),
                    },
                    Ok(value) => {
                        match value {
                            // Short-circuit
                            Value::Int(Some(ref n), v) if n == "dimension" => {
                                if v == 0 {
                                    overworld = Some(true);
                                } else {
                                    break 'file;
                                }
                            }
                            Value::String(Some(ref n), v) if n == "dimension" => {
                                if v == "minecraft:overworld" {
                                    overworld = Some(true);
                                } else {
                                    break 'file;
                                }
                            }

                            // Collect
                            Value::Byte(Some(ref n), v) if n == "scale" => scale = Some(v),
                            Value::Byte(Some(ref n), v) if n == "unlimitedTracking" => {
                                unlimited_tracking = Some(v == 1)
                            }
                            Value::Int(Some(ref n), v) if n == "xCenter" => x = Some(v),
                            Value::Int(Some(ref n), v) if n == "zCenter" => z = Some(v),

                            Value::List(Some(ref n), Tag::Compound, _) if n == "banners" => {
                                'banners: loop {
                                    match parser.next() {
                                        Err(error) => panic!(error),
                                        Ok(value) => {
                                            match value {
                                                Value::Compound(None) => {
                                                    let mut x: Option<i32> = None;
                                                    let mut z: Option<i32> = None;
                                                    let mut label: Option<String> = None;

                                                    'banner: loop {
                                                        match parser.next() {
                                                            Err(error) => panic!(error),
                                                            Ok(value) => {
                                                                match value {
                                                                    Value::String(
                                                                        Some(ref n),
                                                                        v,
                                                                    ) if n == "Name" => {
                                                                        let name: NBTName =
                                                                            serde_json::from_str(
                                                                                &v,
                                                                            )
                                                                            .unwrap();

                                                                        label = Some(name.text)
                                                                    }
                                                                    Value::Compound(Some(
                                                                        ref n,
                                                                    )) if n == "Pos" => {
                                                                        'position: loop {
                                                                            match parser.next() {
                                                                                Err(error) => {
                                                                                    panic!(error)
                                                                                }
                                                                                Ok(value) => {
                                                                                    match value {
                                                                                    // Collect
                                                                            Value::Int(
                                                                                Some(ref n),
                                                                                v,
                                                                            ) if n == "X" => {
                                                                                x = Some(v)
                                                                            }
                                                                            Value::Int(
                                                                                Some(ref n),
                                                                                v,
                                                                            ) if n == "Z" => {
                                                                                z = Some(v)
                                                                            }

                                                                            // End
                                                                            Value::CompoundEnd => {
                                                                                break 'position
                                                                            }

                                                                            // Skip
                                                                            _ => {}
                                                                                }
                                                                                }
                                                                            }
                                                                        }
                                                                    }

                                                                    // End
                                                                    Value::CompoundEnd => {
                                                                        break 'banner
                                                                    }

                                                                    // Skip
                                                                    _ => {}
                                                                }
                                                            }
                                                        }
                                                    }

                                                    let x = x.unwrap();
                                                    let z = z.unwrap();

                                                    on_banner(modified, Banner { label, x, z });
                                                }

                                                // End
                                                Value::ListEnd => break 'banners,

                                                // Skip
                                                _ => {}
                                            }
                                        }
                                    }
                                }

                                added_banners = true;
                            }

                            // Skip
                            // TODO: Value::Compound(_) => nbt::skip_compound(&mut parser).unwrap(),
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
            }

            if overworld != Some(true)
                || unlimited_tracking == Some(true)
                || scale.is_none()
                || x.is_none()
                || z.is_none()
            {
                return;
            }

            let scale = scale.unwrap();
            let x = x.unwrap();
            let z = z.unwrap();

            on_map(Map {
                id,
                modified,
                tile: Tile::from_position(scale, x, z),
            });
        });
}
