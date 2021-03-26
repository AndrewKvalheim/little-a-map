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

#[derive(Deserialize, Serialize)]
pub struct Cache {
    #[serde(deserialize_with = "validate_version")]
    version: String,

    pub players: HashMap<String, Referrer>,
    pub regions: HashMap<(i32, i32), Referrer>,
}

impl Cache {
    pub fn from_path(path: &Path) -> Result<Self> {
        match File::open(path) {
            Ok(f) => bincode::deserialize_from(GzDecoder::new(f)).or_else(|_| Ok(Self::default())),
            Err(e) if e.kind() == NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
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
            players: HashMap::default(),
            regions: HashMap::default(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct Referrer {
    pub map_ids: HashSet<u32>,
    #[serde(with = "FileTimeDefinition")]
    pub modified: FileTime,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "FileTime")]
struct FileTimeDefinition {
    #[serde(getter = "FileTime::unix_seconds")]
    unix_seconds: i64,
    #[serde(getter = "FileTime::nanoseconds")]
    nanoseconds: u32,
}

impl From<FileTimeDefinition> for FileTime {
    fn from(d: FileTimeDefinition) -> Self {
        Self::from_unix_time(d.unix_seconds, d.nanoseconds)
    }
}

fn validate_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    struct VersionVisitor;

    impl<'de> Visitor<'de> for VersionVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
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
