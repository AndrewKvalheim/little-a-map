use forgiving_semver::Version;
use image::{GenericImageView, Pixel};
use itertools::{assert_equal, Itertools};
use little_a_map::{level::Level, palette, render, search};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;
use tempfile::TempDir;
use test_context::{test_context, TestContext};

const MAP_IDS: [u32; 11] = [
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

impl World {
    fn load(major: u64, minor: u64, patch: u64) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("fixtures/world-{major}.{minor}"));

        let level = Level::from_world_path(&path).unwrap();
        let output = tempfile::tempdir_in(env!("TEST_OUTPUT_PATH")).unwrap();
        let output_path = output.path();

        assert_eq!(level.version, Version::new(major, minor, patch));
        let ids = search(&path, output_path, true, true, None).unwrap();
        render(&path, output_path, true, true, &level, &ids).unwrap();

        Self { ids, level, output }
    }
}

struct Worlds([Lazy<World>; 1]);

impl TestContext for Worlds {
    fn setup() -> Self {
        Self([Lazy::new(|| World::load(1, 19, 3))])
    }
}

#[test_context(Worlds)]
#[test]
fn spawn(worlds: &mut Worlds) {
    for world in &worlds.0 {
        assert_eq!((world.level.spawn_x, world.level.spawn_z), (0, 0));
    }
}

#[test_context(Worlds)]
#[test]
fn map_ids(worlds: &mut Worlds) {
    for world in &worlds.0 {
        assert_equal(world.ids.iter().sorted(), &MAP_IDS);
    }
}

#[test_context(Worlds)]
#[test]
fn banners(worlds: &mut Worlds) {
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

    for world in &worlds.0 {
        let json = File::open(world.output.path().join("banners.json")).unwrap();
        let geo: GeoJson = serde_json::from_reader(json).unwrap();

        let actual = geo.features.into_iter().sorted().map(|f| (f.name, f.color));
        let expected = BANNERS.iter().map(|&(n, c)| (n.map(Into::into), c.into()));
        assert_equal(actual, expected);
    }
}

#[test_context(Worlds)]
#[test]
fn swatch(worlds: &mut Worlds) {
    for world in &worlds.0 {
        let view = image::open(world.output.path().join("tiles/4/0/0.png")).unwrap();

        assert_eq!(view.dimensions(), (128, 128));

        for (i, rgb) in (0..).zip(palette::BASE.into_iter()).skip(1) {
            let pixel = view.get_pixel(i, 0);
            assert_eq!(pixel.to_rgb(), rgb.into());
        }
    }
}