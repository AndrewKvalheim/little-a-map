use glob::glob;
use image::{GenericImageView, Pixel};
use itertools::{assert_equal, Itertools};
use little_a_map::{level::Level, palette, render, search};
use rstest::*;
use rstest_reuse::{self, *};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime};
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
    input: PathBuf,
    output: TempDir,
    level: Level,
}

impl World {
    fn render(&self, ids: &HashSet<u32>) -> &Path {
        let output = self.output.path();
        render(&self.input, output, true, true, &self.level, ids).unwrap();
        output
    }

    fn search(&self) -> HashSet<u32> {
        search(&self.input, self.output.path(), true, true, None).unwrap()
    }
}

impl FromStr for World {
    type Err = ();

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        let input =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/world-{version}"));
        let world = Self {
            level: Level::from_world_path(&input).unwrap(),
            output: tempfile::tempdir_in(env!("TEST_OUTPUT_PATH")).unwrap(),
            input,
        };

        assert_eq!(world.level.version.to_string(), version);

        Ok(world)
    }
}

fn assert_modifications(
    expect_modified: &[&str],
    before: &HashMap<String, SystemTime>,
    after: &HashMap<String, SystemTime>,
) {
    for path in before.keys() {
        assert!(after.contains_key(path), "{path} vanished");
    }

    for (path, modified) in after {
        if expect_modified.contains(&path.as_ref()) {
            assert!(*modified > before[path], "{path} should be modified");
        } else {
            assert_eq!(*modified, before[path], "{path} unexpectedly modified");
        }
    }
}

fn observe_modifications(base: &Path) -> HashMap<String, SystemTime> {
    glob(base.join("**/*.*").to_str().unwrap())
        .unwrap()
        .map(|entry| {
            let absolute = entry.unwrap();
            let relative = absolute.strip_prefix(base).unwrap();
            let modified = fs::metadata(&absolute).unwrap().modified().unwrap();

            (relative.to_str().unwrap().to_owned(), modified)
        })
        .collect()
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
    assert_equal(world.search().iter().sorted(), &MAP_IDS);
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

    let output = world.render(&world.search());
    let json = File::open(output.join("banners.json")).unwrap();
    let geo: GeoJson = serde_json::from_reader(json).unwrap();

    let actual = geo.features.into_iter().sorted().map(|f| (f.name, f.color));
    let expected = BANNERS.iter().map(|&(n, c)| (n.map(Into::into), c.into()));
    assert_equal(actual, expected);
}

#[apply(worlds)]
fn swatch(world: World, #[values("maps/1.png", "tiles/4/0/0.png")] path: &str) {
    let output = world.render(&world.search());
    let view = image::open(output.join(path)).unwrap();

    assert_eq!(view.dimensions(), (128, 128));

    for (i, rgb) in (0..).zip(palette::BASE.into_iter()).skip(1) {
        let pixel = view.get_pixel(i, 0);
        assert_eq!(pixel.to_rgb(), rgb.into());
    }
}

#[apply(worlds)]
fn rerun(world: World) {
    let ids_1 = world.search();
    let modifications_1 = observe_modifications(world.render(&ids_1));

    thread::sleep(Duration::from_millis(100));

    let ids_2 = world.search();
    let modifications_2 = observe_modifications(world.render(&ids_2));

    assert_eq!(ids_2, ids_1);
    assert_modifications(
        &[".cache/little-a-map.dat", "index.html"],
        &modifications_1,
        &modifications_2,
    );
}
