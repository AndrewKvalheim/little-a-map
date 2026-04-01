use crate::level::Level;
use anyhow::Result;
use glob::{glob, Paths};
use std::convert::TryFrom;
use std::path::PathBuf;

pub struct World {
    path: PathBuf,
    pub level: Level,
}

impl World {
    pub fn entity_paths(&self) -> Result<Paths> {
        let relative = if self.level.data_version >= 4786 {
            "dimensions/minecraft/overworld/entities/r.*.mca"
        } else {
            "entities/r.*.mca"
        };

        Ok(glob(self.path.join(relative).to_str().unwrap())?)
    }

    #[must_use]
    pub fn map_path(&self, id: u32) -> PathBuf {
        let relative = if self.level.data_version >= 4786 {
            format!("data/minecraft/maps/{id}.dat")
        } else {
            format!("data/map_{id}.dat")
        };

        self.path.join(relative)
    }

    pub fn player_paths(&self) -> Result<Paths> {
        let relative = if self.level.data_version >= 4786 {
            "players/data/????????-????-????-????-????????????.dat"
        } else {
            "playerdata/????????-????-????-????-????????????.dat"
        };

        Ok(glob(self.path.join(relative).to_str().unwrap())?)
    }

    pub fn region_paths(&self) -> Result<Paths> {
        let relative = if self.level.data_version >= 4786 {
            "dimensions/minecraft/overworld/region/r.*.mca"
        } else {
            "region/r.*.mca"
        };

        Ok(glob(self.path.join(relative).to_str().unwrap())?)
    }
}

impl TryFrom<PathBuf> for World {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        let level = path.join("level.dat").as_path().try_into()?;

        Ok(Self { path, level })
    }
}
