use super::COMPATIBLE_VERSIONS;
use crate::utilities::read_gz;
use anyhow::Result;
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
    pub fn from_world_path(path: &Path) -> Result<Self> {
        let level: Self = from_bytes(&read_gz(&path.join("level.dat"))?)?;

        assert!(
            VersionReq::parse(COMPATIBLE_VERSIONS)?.matches(&level.version),
            "Incompatible with game version {}",
            level.version
        );

        Ok(level)
    }
}