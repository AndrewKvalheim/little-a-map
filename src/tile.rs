use crate::map::{Map, MapData};
use crate::utilities::write_webp;
use anyhow::Result;
use serde_json::json;
use std::fs::{self, File};
use std::ops::Add;
use std::path::Path;
use std::time::SystemTime;

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
    pub const fn new(zoom: u8, x: i32, y: i32) -> Self {
        Self { zoom, x, y }
    }

    pub fn position(&self) -> (i32, i32) {
        let size = 128 * 2_i32.pow(u32::from(4 - self.zoom));

        (size * self.x, size * self.y)
    }

    pub const fn quadrants(&self) -> [Self; 4] {
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
        maps_modified: SystemTime,
        force: bool,
    ) -> Result<bool> {
        let dir_path = output_path.join(format!("tiles/{}/{}", self.zoom, self.x));

        let base_path = dir_path.join(self.y.to_string());
        let meta_path = base_path.with_extension("meta.json");

        if !force
            && fs::metadata(&meta_path)
                .and_then(|m| m.modified())
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
        let meta_file = File::create(&meta_path)?;
        serde_json::to_writer(&meta_file, &json!({ "maps": ids }))?;
        meta_file.set_modified(maps_modified)?;

        // Image
        if canvas.is_dirty {
            let mut webp_file = File::create(base_path.with_extension("webp"))?;
            write_webp(&mut webp_file, &canvas.pixels)?;
            webp_file.set_modified(maps_modified)?;
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
        let ((tx, ty), (mx, my)) = (tile.position(), map.tile.position());
        let factor = 2_usize.pow(u32::from(tile.zoom - map.tile.zoom));
        #[allow(clippy::cast_sign_loss)] // tile ⊆ map
        let a = (tx - mx) as usize / factor + (ty - my) as usize / factor * 128;
        let b = 128 - 128 / factor;

        for (i, pixel) in self.pixels.iter_mut().enumerate().filter(|(_, p)| **p < 4) {
            let (j, k) = (i / factor, i / 128);
            let map_pixel = data.0[a + j + b * k - (k - j / 128) * 128];

            if map_pixel >= 4 {
                self.is_dirty = true;
                *pixel = map_pixel;
            }
        }
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
