use super::COMPATIBLE_VERSIONS;
use crate::utilities::read_gz;
use anyhow::{Context, Result};
use fastnbt::from_bytes;
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
        #[derive(serde_query::Deserialize)]
        struct Internal {
            #[query(".Data.SpawnX")]
            spawn_x: i32,
            #[query(".Data.SpawnZ")]
            spawn_z: i32,
            #[query(".Data.Version.Name")]
            version: String,
        }

        let mut internal = Internal::deserialize(deserializer)?;

        // Workaround for dtolnay/semver#219
        internal.version.push_str(
            &".0".repeat(
                2 - internal
                    .version
                    .chars()
                    .filter(|&c| c == '.')
                    .take(2)
                    .count(),
            ),
        );

        Ok(Self {
            spawn_x: internal.spawn_x,
            spawn_z: internal.spawn_z,
            version: Version::parse(&internal.version).map_err(de::Error::custom)?,
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
