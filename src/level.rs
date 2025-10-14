use super::COMPATIBLE_VERSIONS;
use crate::utilities::read_gz;
use anyhow::{Context, Result};
use fastnbt::{from_bytes, IntArray};
use semver::{Version, VersionReq};
use serde::{de, Deserialize, Deserializer};
use std::path::Path;

pub struct Level {
    pub spawn_x: i32,
    pub spawn_z: i32,
    pub version: Version,
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn to_version(mut v: String) -> std::result::Result<Version, semver::Error> {
            // Workaround for dtolnay/semver#219
            v.push_str(&".0".repeat(2 - v.chars().filter(|&c| c == '.').take(2).count()));

            Version::parse(&v)
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Internal {
            V1218(V1218),
            V1219(V1219),
        }

        #[derive(serde_query::Deserialize)]
        struct V1218 {
            #[query(".Data.SpawnX")]
            spawn_x: i32,
            #[query(".Data.SpawnZ")]
            spawn_z: i32,
            #[query(".Data.Version.Name")]
            version: String,
        }

        #[derive(serde_query::Deserialize)]
        struct V1219 {
            #[query(".Data.spawn.pos")]
            pos: IntArray,
            #[query(".Data.Version.Name")]
            version: String,
        }

        Ok(match Internal::deserialize(deserializer)? {
            Internal::V1218(i) => Self {
                spawn_x: i.spawn_x,
                spawn_z: i.spawn_z,
                version: to_version(i.version).map_err(de::Error::custom)?,
            },
            Internal::V1219(i) => Self {
                spawn_x: i.pos[0],
                spawn_z: i.pos[2],
                version: to_version(i.version).map_err(de::Error::custom)?,
            },
        })
    }
}

impl Level {
    pub fn from_world_path(world_path: &Path) -> Result<Self> {
        let path = world_path.join("level.dat");
        let level: Self = from_bytes(&read_gz(&path)?)
            .with_context(|| format!("Failed to deserialize {}", path.display()))?;

        assert!(
            VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level.version),
            "Incompatible with game version {}",
            level.version
        );

        Ok(level)
    }
}
