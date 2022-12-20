use anyhow::Result;
use filetime::FileTime;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::ErrorKind::NotFound;
use std::path::Path;

pub type MapIdsByRegion = HashMap<(i32, i32), HashSet<u32>>;

#[derive(Deserialize, Serialize)]
pub struct Cache {
    #[serde(skip)]
    pub modified: Option<FileTime>,

    #[serde(deserialize_with = "validate_version")]
    version: String,

    pub map_ids_by_entities_region: MapIdsByRegion,
    pub map_ids_by_level_region: MapIdsByRegion,
    pub map_ids_by_player: HashMap<usize, HashSet<u32>>,
}

impl Cache {
    pub fn from_path(path: &Path) -> Result<Self> {
        match File::open(path) {
            Ok(f) => {
                let mut cache =
                    bincode::deserialize_from::<_, Self>(GzDecoder::new(f)).unwrap_or_default();
                cache.modified = Some(FileTime::from_last_modification_time(&fs::metadata(path)?));

                Ok(cache)
            }
            Err(e) if e.kind() == NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn is_expired_for(&self, path: &Path) -> Result<bool> {
        let modified = FileTime::from_last_modification_time(&fs::metadata(path)?);
        Ok(self.modified.map_or(true, |m| m < modified))
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let gz = GzEncoder::new(File::create(path)?, Compression::default());
        Ok(bincode::serialize_into(gz, self)?)
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            map_ids_by_entities_region: HashMap::default(),
            map_ids_by_level_region: HashMap::default(),
            map_ids_by_player: HashMap::default(),
            modified: Option::default(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

fn validate_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    struct VersionVisitor;

    impl Visitor<'_> for VersionVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(env!("CARGO_PKG_VERSION"))
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            if value == env!("CARGO_PKG_VERSION") {
                Ok(value.to_owned())
            } else {
                Err(E::invalid_value(Unexpected::Str(value), &self))
            }
        }
    }

    deserializer.deserialize_str(VersionVisitor)
}
