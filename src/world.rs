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
        Ok(glob(self.path.join("entities/r.*.mca").to_str().unwrap())?)
    }

    #[must_use]
    pub fn map_path(&self, id: u32) -> PathBuf {
        self.path.join(format!("data/map_{id}.dat"))
    }

    pub fn player_paths(&self) -> Result<Paths> {
        let relative = "playerdata/????????-????-????-????-????????????.dat";

        Ok(glob(self.path.join(relative).to_str().unwrap())?)
    }

    pub fn region_paths(&self) -> Result<Paths> {
        Ok(glob(self.path.join("region/r.*.mca").to_str().unwrap())?)
    }
}

impl TryFrom<PathBuf> for World {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        let level = path.join("level.dat").as_path().try_into()?;

        Ok(Self { path, level })
    }
}
