use crate::map::{Map, MapData};
use anyhow::Result;
use filetime::FileTime;
use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufWriter;
use std::ops::Add;
use std::path::Path;

const PALETTE_BASE: [[u8; 3]; 62] = [
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
    [100, 100, 100],
    [216, 175, 147],
    [127, 167, 150],
];
const PALETTE_FACTORS: [u32; 4] = [180, 220, 255, 135];
const PALETTE_LEN: usize = PALETTE_BASE.len() * PALETTE_FACTORS.len();
#[allow(clippy::cast_possible_truncation)]
static PALETTE: Lazy<Vec<u8>> = Lazy::new(|| {
    PALETTE_BASE
        .iter()
        .flat_map(|rgb| {
            PALETTE_FACTORS
                .iter()
                .flat_map(move |&f| rgb.iter().map(move |&v| (u32::from(v) * f / 255) as u8))
        })
        .collect()
});

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Tile {
    pub zoom: u8,
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub fn from_position(scale: u8, x: i32, z: i32) -> Self {
        let size = 128 * 2_i32.pow(u32::from(scale));

        Self {
            zoom: 4 - scale,
            x: x.div_euclid(size),
            y: z.div_euclid(size),
        }
    }

    #[cfg(test)]
    pub fn new(zoom: u8, x: i32, y: i32) -> Self {
        Self { zoom, x, y }
    }

    pub fn position(&self) -> (i32, i32) {
        let size = 128 * 2_i32.pow(u32::from(4 - self.zoom));

        (size * self.x, size * self.y)
    }

    pub fn quadrants(&self) -> [Self; 4] {
        let zoom = self.zoom + 1;
        let x = self.x * 2;
        let y = self.y * 2;

        [
            Self { zoom, x, y },
            Self { zoom, x, y: y + 1 },
            Self { zoom, x: x + 1, y },
            Self {
                zoom,
                x: x + 1,
                y: y + 1,
            },
        ]
    }

    pub fn render<'a>(
        &self,
        output_path: &Path,
        maps: impl IntoIterator<Item = &'a (&'a Map, MapData)>,
        maps_modified: FileTime,
        force: bool,
    ) -> Result<bool> {
        let dir_path = output_path.join(format!("tiles/{}/{}", self.zoom, self.x));

        let base_path = dir_path.join(self.y.to_string());
        let meta_path = base_path.with_extension("meta.json");

        if !force
            && fs::metadata(&meta_path)
                .map(|m| FileTime::from_last_modification_time(&m))
                .map_or(false, |meta_modified| meta_modified >= maps_modified)
        {
            return Ok(false);
        }

        let mut canvas = Canvas::default();

        let ids = maps
            .into_iter()
            .map(|(map, data)| {
                canvas.draw(self, map, data);

                map.id
            })
            .collect::<Vec<_>>();

        // Metadata
        fs::create_dir_all(&dir_path)?;
        serde_json::to_writer(&File::create(&meta_path)?, &json!({ "maps": ids }))?;
        filetime::set_file_mtime(&meta_path, maps_modified)?;

        // Image
        if canvas.is_dirty {
            let palette = canvas.shrink_palette();

            let png_path = base_path.with_extension("png");
            let mut encoder = png::Encoder::new(BufWriter::new(File::create(&png_path)?), 128, 128);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_compression(png::Compression::Best);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_filter(png::FilterType::NoFilter);
            encoder.set_palette(palette);
            encoder.write_header()?.write_image_data(&canvas.pixels)?;
            filetime::set_file_mtime(&png_path, maps_modified)?;
        }

        Ok(true)
    }

    pub fn root(&self) -> Self {
        let (x, y) = self.position();

        Self {
            zoom: 0,
            x: x.div_euclid(2048),
            y: y.div_euclid(2048),
        }
    }
}

impl Add<(i32, i32)> for &Tile {
    type Output = Tile;

    fn add(self, (x, y): (i32, i32)) -> Self::Output {
        Tile {
            x: self.x + x,
            y: self.y + y,
            ..*self
        }
    }
}

struct Canvas {
    is_dirty: bool,
    pixels: [u8; 128 * 128],
}

impl Canvas {
    fn draw(&mut self, tile: &Tile, map: &Map, data: &MapData) {
        let (tx, ty) = tile.position();
        let (mx, my) = map.tile.position();
        let factor = 2_usize.pow(u32::from(tile.zoom - map.tile.zoom));
        #[allow(clippy::cast_sign_loss)]
        let a = (tx - mx) as usize / factor + (ty - my) as usize / factor * 128;
        let b = 128 - 128 / factor;

        for (i, pixel) in self.pixels.iter_mut().enumerate().filter(|(_, p)| **p < 4) {
            let (j, k) = (i / factor, i / 128);
            let map_pixel = data.0[(a + j + b * k - (k - j / 128) * 128)];

            if map_pixel >= 4 {
                self.is_dirty = true;
                *pixel = map_pixel;
            }
        }
    }

    fn shrink_palette(&mut self) -> Vec<u8> {
        let mut palette = Vec::with_capacity(PALETTE_LEN * 3);
        let mut map = HashMap::with_capacity(PALETTE_LEN);
        let mut next = 0;

        for pixel in self.pixels.iter_mut() {
            *pixel = *map.entry(*pixel).or_insert_with(|| {
                let (i, j) = (*pixel as usize * 3, next);
                palette.extend(&PALETTE[i..i + 3]);
                next += 1;
                j
            });
        }

        palette
    }
}

// Pending https://github.com/rust-lang/rust/issues/61415
impl Default for Canvas {
    fn default() -> Self {
        Self {
            is_dirty: bool::default(),
            pixels: [u8::default(); 128 * 128],
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn from_position() {
        fn expect(scale: u8, cx: i32, cz: i32, zoom: u8, x: i32, y: i32) {
            assert_eq!(Tile::from_position(scale, cx, cz), Tile::new(zoom, x, y));
        }

        expect(4, 1, 1, 0, 0, 0);
        expect(4, -1, 1, 0, -1, 0);

        expect(0, -20608, 20096, 4, -161, 157);
        expect(1, -20608, 20096, 3, -81, 78);
        expect(2, -20608, 20096, 2, -41, 39);
        expect(3, -20608, 20096, 1, -21, 19);
        expect(4, -20608, 20096, 0, -11, 9);
    }

    #[test]
    fn position() {
        fn expect(scale: u8, cx: i32, cz: i32, x: i32, y: i32) {
            assert_eq!(Tile::from_position(scale, cx, cz).position(), (x, y));
        }

        assert_eq!(Tile::new(0, 0, 0).position(), (0, 0));
        expect(0, 127, 127, 0, 0);
        expect(0, 128, 128, 128, 128);
        expect(0, -128, -128, -128, -128);
        expect(0, -129, -129, -256, -256);
        expect(4, 2047, 2047, 0, 0);
        expect(4, 2048, 2048, 2048, 2048);
        expect(4, -2048, -2048, -2048, -2048);
        expect(4, -2049, -2049, -4096, -4096);
    }

    #[test]
    fn quadrants() {
        assert_eq!(
            Tile::new(0, 0, 0).quadrants(),
            [
                Tile::new(1, 0, 0),
                Tile::new(1, 0, 1),
                Tile::new(1, 1, 0),
                Tile::new(1, 1, 1),
            ]
        );

        let steps = [
            Tile::new(0, -11, 9),
            Tile::new(1, -21, 19),
            Tile::new(2, -41, 39),
            Tile::new(3, -81, 78),
            Tile::new(4, -161, 157),
        ];
        assert_eq!(steps[0].quadrants()[3], steps[1]);
        assert_eq!(steps[1].quadrants()[3], steps[2]);
        assert_eq!(steps[2].quadrants()[2], steps[3]);
        assert_eq!(steps[3].quadrants()[3], steps[4]);
    }
}
