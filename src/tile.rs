use crate::level::{self, MapData};
use crate::map::Map;
use filetime::FileTime;
use std::fs::{self, File};
use std::io::BufWriter;
use std::ops::Add;
use std::path::PathBuf;

type Canvas = [u8; 128 * 128];

fn draw_behind(tile: &Tile, canvas: &mut Canvas, map: &Map, data: &MapData) {
    let (tx, ty) = tile.position();
    let (mx, my) = map.tile.position();
    let factor = 2i32.pow((tile.zoom - map.tile.zoom) as u32);

    for (i, pixel) in canvas.iter_mut().enumerate() {
        let j = i as i32 / factor;
        let k = i as i32 / 128;
        let pick = (tx - mx) / factor + (ty - my) / factor * 128 + j + (128 - (128 / factor)) * k
            - 128 * (k - j / 128);

        let map_pixel = data[pick as usize] as u8;

        if map_pixel >= 4 {
            *pixel = map_pixel;
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Tile {
    pub zoom: i8,
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub fn from_position(scale: i8, x: i32, z: i32) -> Self {
        let size = 128 * 2i32.pow(scale as u32);

        Self {
            zoom: 4 - scale,
            x: x.div_euclid(size),
            y: z.div_euclid(size),
        }
    }

    #[cfg(test)]
    pub fn new(zoom: i8, x: i32, y: i32) -> Self {
        Self { zoom, x, y }
    }

    pub fn position(&self) -> (i32, i32) {
        let size = 128 * 2i32.pow((4 - self.zoom) as u32);

        (size * self.x, size * self.y)
    }

    pub fn quadrants(&self) -> [Tile; 4] {
        let zoom = self.zoom + 1;
        let x = self.x * 2;
        let y = self.y * 2;

        [
            Tile { zoom, x, y },
            Tile { zoom, x, y: y + 1 },
            Tile { zoom, x: x + 1, y },
            Tile {
                zoom,
                x: x + 1,
                y: y + 1,
            },
        ]
    }

    pub fn render<'a>(
        &self,
        path: &PathBuf,
        maps: impl IntoIterator<Item = &'a (&'a Map, MapData)>,
        modified: FileTime,
    ) {
        let mut canvas = [0; 128 * 128];

        maps.into_iter().for_each(|&(map, data)| {
            draw_behind(self, &mut canvas, map, &data);
        });

        fs::create_dir_all(path.parent().unwrap()).unwrap();

        let mut encoder = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), 128, 128);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(level::PALETTE.clone());
        encoder.set_trns(level::TRNS.to_vec());
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&canvas)
            .unwrap();

        filetime::set_file_mtime(path, modified).unwrap();
    }

    pub fn root(&self) -> Self {
        let position = self.position();

        Self {
            zoom: 0,
            x: position.0.div_euclid(2048),
            y: position.1.div_euclid(2048),
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn from_position() {
        fn expect(scale: i8, cx: i32, cz: i32, zoom: i8, x: i32, y: i32) {
            assert_eq!(Tile::from_position(scale, cx, cz), Tile::new(zoom, x, y))
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
        assert_eq!(Tile::new(0, 0, 0).position(), (0, 0));
        assert_eq!(Tile::from_position(0, 127, 127).position(), (0, 0));
        assert_eq!(Tile::from_position(0, 128, 128).position(), (128, 128));
        assert_eq!(Tile::from_position(0, -128, -128).position(), (-128, -128));
        assert_eq!(Tile::from_position(0, -129, -129).position(), (-256, -256));
        assert_eq!(Tile::from_position(4, 2047, 2047).position(), (0, 0));
        assert_eq!(Tile::from_position(4, 2048, 2048).position(), (2048, 2048));
        assert_eq!(
            Tile::from_position(4, -2048, -2048).position(),
            (-2048, -2048)
        );
        assert_eq!(
            Tile::from_position(4, -2049, -2049).position(),
            (-4096, -4096)
        );
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
