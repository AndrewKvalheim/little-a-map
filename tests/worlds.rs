use glob::glob;
use image::{GenericImageView, Pixel};
use itertools::{assert_equal, Itertools};
use little_a_map::{palette, render, search, world::World};
use rstest::*;
use rstest_reuse::{self, *};
use semver::VersionReq;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

const MAP_IDS: [(&str, u32); 21] = [
    (">=1.0", 0), // Player offhand
    (">=1.0", 1), // Item frame
    // 2 Enlarged to #11
    (">=1.0", 3),     // Chest
    (">=1.0", 4),     // Trapped chest
    (">=1.0", 5),     // Minecart with chest
    (">=1.0.6", 6),   // Boat with chest
    (">=1.11", 7),    // Shulker box
    (">=1.11", 8),    // Llama
    (">=1.11", 9),    // Shulker box in chest
    (">=1.11", 10),   // Shulker box in player inventory
    (">=1.17", 11),   // Glow item frame
    (">=1.0", 12),    // Stack in player inventory
    (">=1.3.1", 13),  // Ender chest
    (">=1.11", 14),   // Shulker box in ender chest
    (">=1.21.2", 15), // Bundle in chest
    (">=1.21.2", 16), // Bundle in bundle in chest
    (">=1.21.2", 17), // Bundle in player inventory
    (">=1.21.5", 18), // Player inventory
    (">=1.21.9", 19), // Copper chest
    (">=1.21.9", 20), // Shelf
    (">=1.21.9", 21), // Bundle on shelf
];

const BANNERS: [(Option<&str>, &str); 19] = [
    (None, "white"),                           // Default white banner
    (None, "light_gray"),                      // Default light gray banner
    (None, "gray"),                            // Default gray banner
    (None, "black"),                           // Default black banner
    (None, "brown"),                           // Default brown banner
    (None, "red"),                             // Default red banner
    (None, "orange"),                          // Default orange banner
    (None, "yellow"),                          // Default yellow banner
    (None, "lime"),                            // Default lime banner
    (None, "green"),                           // Default green banner
    (None, "cyan"),                            // Default cyan banner
    (None, "light_blue"),                      // Default light blue banner
    (None, "blue"),                            // Default blue banner
    (None, "purple"),                          // Default purple banner
    (None, "magenta"),                         // Default magenta banner
    (None, "pink"),                            // Default pink banner
    (Some("Example Banner"), "white"),         // Renamed white banner
    (None, "white"),                           // Default ominous banner
    (Some("Example Ominous Banner"), "white"), // Renamed ominous banner
];

struct Case {
    output: TempDir,
    world: World,
}

impl Case {
    fn render(&self, ids: &HashSet<u32>) -> &Path {
        let output = self.output.path();
        render(&self.world, output, true, true, ids).unwrap();
        output
    }

    fn search(&self) -> HashSet<u32> {
        search(&self.world, self.output.path(), true, true, None).unwrap()
    }
}

impl FromStr for Case {
    type Err = ();

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        let input =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/world-{version}"));
        let case = Self {
            world: input.try_into().unwrap(),
            output: tempfile::tempdir_in(env!("TEST_OUTPUT_PATH")).unwrap(),
        };

        assert_eq!(case.world.level.version.to_string(), version);

        Ok(case)
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
#[case::world_1_21_0("1.21.0")]
#[case::world_1_21_1("1.21.1")]
#[case::world_1_21_3("1.21.3")]
#[case::world_1_21_4("1.21.4")]
#[case::world_1_21_5("1.21.5")]
#[case::world_1_21_6("1.21.6")]
#[case::world_1_21_7("1.21.7")]
#[case::world_1_21_8("1.21.8")]
#[case::world_1_21_9("1.21.9")]
#[case::world_1_21_10("1.21.10")]
#[case::world_1_21_11("1.21.11")]
fn cases(#[case] case: Case) {}

#[apply(cases)]
fn spawn(case: Case) {
    assert_eq!((case.world.level.spawn_x, case.world.level.spawn_z), (0, 0));
}

#[apply(cases)]
fn map_ids(case: Case) {
    assert_equal(
        case.search().iter().sorted(),
        MAP_IDS
            .iter()
            .filter(|(v, _)| {
                VersionReq::parse(v)
                    .unwrap()
                    .matches(&case.world.level.version)
            })
            .map(|(_, id)| id),
    );
}

#[apply(cases)]
fn banners(case: Case) {
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

    let output = case.render(&case.search());
    let json = File::open(output.join("banners.json")).unwrap();
    let geo: GeoJson = serde_json::from_reader(json).unwrap();

    let actual = geo.features.into_iter().sorted().map(|f| (f.name, f.color));
    let expected = BANNERS.iter().map(|&(n, c)| (n.map(Into::into), c.into()));
    assert_equal(actual, expected);
}

#[apply(cases)]
fn swatch(case: Case, #[values("maps/1.webp", "tiles/4/0/0.webp")] relative_path: &str) {
    let output = case.render(&case.search());
    let path = output.join(relative_path);
    let metadata = fs::metadata(&path).unwrap();
    let view = image::open(&path).unwrap();

    assert_eq!(view.dimensions(), (128, 128));

    for (i, rgb) in (0..).zip(palette::BASE.into_iter()).skip(1) {
        let pixel = view.get_pixel(i, 0);
        assert_eq!(pixel.to_rgb(), rgb.into());
    }

    let expected = 850;
    let tolerance = 100;
    let actual = metadata.len();
    assert!(
        ((expected - tolerance)..=(expected + tolerance)).contains(&actual),
        "Expected size of {}: {expected}±{tolerance} B, Actual size: {actual} B",
        &path.display(),
    );
}

#[apply(cases)]
fn rerun(case: Case) {
    let ids_1 = case.search();
    let modifications_1 = observe_modifications(case.render(&ids_1));

    thread::sleep(Duration::from_millis(100));

    let ids_2 = case.search();
    let modifications_2 = observe_modifications(case.render(&ids_2));

    assert_eq!(ids_2, ids_1);
    assert_modifications(
        &[".cache/little-a-map.dat", "index.html"],
        &modifications_1,
        &modifications_2,
    );
}
