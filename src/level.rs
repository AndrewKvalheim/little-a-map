use super::COMPATIBLE_VERSIONS;
use crate::utilities::read_gz;
use anyhow::{Context, Result};
use fastnbt::from_bytes;
use forgiving_semver::{Version, VersionReq};
use std::path::Path;

#[derive(serde_query::Deserialize)]
pub struct Level {
    #[query(".Data.SpawnX")]
    pub spawn_x: i32,
    #[query(".Data.SpawnZ")]
    pub spawn_z: i32,
    #[query(".Data.Version.Name")]
    pub version: Version,
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
