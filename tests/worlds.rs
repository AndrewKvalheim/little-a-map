use image::{GenericImageView, Pixel};
use itertools::{assert_equal, Itertools};
use little_a_map::{level::Level, palette, render, search};
use rstest::*;
use rstest_reuse::{self, *};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;
use tempfile::TempDir;

const MAP_IDS: [u32; 12] = [
    0, // Player inventory
    1, // Item frame
    // 2, // Enlarged
    3,  // Chest
    4,  // Trapped chest
    5,  // Minecart with chest
    6,  // Boat with chest
    7,  // Shulker box
    8,  // Llama
    9,  // Shulker box in chest
    10, // Shulker box in player inventory
    11, // Glow item frame (enlarged from #2)
    12, // Stack in player inventory
];

const BANNERS: [(Option<&str>, &str); 17] = [
    (None, "white"),
    (None, "light_gray"),
    (None, "gray"),
    (None, "black"),
    (None, "brown"),
    (None, "red"),
    (None, "orange"),
    (None, "yellow"),
    (None, "lime"),
    (None, "green"),
    (None, "cyan"),
    (None, "light_blue"),
    (None, "blue"),
    (None, "purple"),
    (None, "magenta"),
    (None, "pink"),
    (Some("Example Banner"), "white"),
];

struct World {
    pub ids: HashSet<u32>,
    pub level: Level,
    pub output: TempDir,
}

impl FromStr for World {
    type Err = ();

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        let input =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/world-{version}"));
        let output = tempfile::tempdir_in(env!("TEST_OUTPUT_PATH")).unwrap();

        let level = Level::from_world_path(&input).unwrap();
        assert_eq!(level.version.to_string(), version);

        let ids = search(&input, output.path(), true, true, None).unwrap();
        render(&input, output.path(), true, true, &level, &ids).unwrap();

        Ok(Self { ids, level, output })
    }
}

#[template]
#[rstest]
#[case::world_1_20_2("1.20.2")]
#[case::world_1_20_4("1.20.4")]
#[case::world_1_20_6("1.20.6")]
fn worlds(#[case] world: World) {}

#[apply(worlds)]
fn spawn(world: World) {
    assert_eq!((world.level.spawn_x, world.level.spawn_z), (0, 0));
}

#[apply(worlds)]
fn map_ids(world: World) {
    assert_equal(world.ids.iter().sorted(), &MAP_IDS);
}

#[apply(worlds)]
fn banners(world: World) {
    #[derive(Deserialize)]
    struct GeoJson {
        features: Vec<Feature>,
    }

    #[derive(serde_query::Deserialize, Eq, Ord, PartialEq, PartialOrd)]
    struct Feature {
        #[query(".geometry.coordinates.[1]")]
        pub z: i32,
        #[query(".geometry.coordinates.[0]")]
        pub x: i32,
        #[query(".properties.name")]
        pub name: Option<String>,
        #[query(".properties.color")]
        pub color: String,
    }

    let json = File::open(world.output.path().join("banners.json")).unwrap();
    let geo: GeoJson = serde_json::from_reader(json).unwrap();

    let actual = geo.features.into_iter().sorted().map(|f| (f.name, f.color));
    let expected = BANNERS.iter().map(|&(n, c)| (n.map(Into::into), c.into()));
    assert_equal(actual, expected);
}

#[apply(worlds)]
fn swatch(world: World, #[values("maps/1.png", "tiles/4/0/0.png")] path: &str) {
    let view = image::open(world.output.path().join(path)).unwrap();

    assert_eq!(view.dimensions(), (128, 128));

    for (i, rgb) in (0..).zip(palette::BASE.into_iter()).skip(1) {
        let pixel = view.get_pixel(i, 0);
        assert_eq!(pixel.to_rgb(), rgb.into());
    }
}
