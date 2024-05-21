use anyhow::Result;
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::ErrorKind::NotFound;
use std::path::Path;
use std::time::SystemTime;
use zstd::stream::{read::Decoder as ZstdDecoder, write::Encoder as ZstdEncoder};

pub type IdsBy<K> = HashMap<K, HashSet<u32>>;

#[derive(Deserialize, Serialize)]
pub struct Cache {
    #[serde(skip)]
    pub modified: Option<SystemTime>,

    #[serde(deserialize_with = "validate_version")]
    version: String,

    pub map_ids_by_entities_region: IdsBy<(i32, i32)>,
    pub map_ids_by_block_region: IdsBy<(i32, i32)>,
    pub map_ids_by_player: IdsBy<usize>,
}

impl Cache {
    pub fn from_path(path: &Path) -> Result<Self> {
        match File::open(path) {
            Ok(f) => {
                let mut cache =
                    bincode::deserialize_from::<_, Self>(ZstdDecoder::new(f)?).unwrap_or_default();
                cache.modified = Some(fs::metadata(path)?.modified()?);

                Ok(cache)
            }
            Err(e) if e.kind() == NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn is_expired_for(&self, path: &Path) -> Result<bool> {
        let modified = fs::metadata(path)?.modified()?;
        Ok(self.modified.map_or(true, |m| m < modified))
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let z = ZstdEncoder::new(File::create(path)?, 0)?.auto_finish();
        Ok(bincode::serialize_into(z, self)?)
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            map_ids_by_entities_region: HashMap::default(),
            map_ids_by_block_region: HashMap::default(),
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

#[cfg(test)]
mod test {
    use super::*;
    use forgiving_semver::Version;
    use serde_json::json;

    fn next_version(text: impl AsRef<str>) -> String {
        let mut version = Version::parse(text.as_ref()).unwrap();
        match version {
            Version { patch, .. } if patch > 0 => version.patch -= 1,
            Version { minor, .. } if minor > 0 => version.minor -= 1,
            _ => version.major -= 1,
        }
        version.to_string()
    }

    fn previous_version(text: impl AsRef<str>) -> String {
        let mut version = Version::parse(text.as_ref()).unwrap();
        version.increment_patch();
        version.to_string()
    }

    fn with_version(version: impl AsRef<str>) -> Result<Cache> {
        Ok(serde_json::from_value::<Cache>(json!({
            "version": version.as_ref(),
            "map_ids_by_entities_region": {},
            "map_ids_by_block_region": {},
            "map_ids_by_player": {}
        }))?)
    }

    #[test]
    fn validate_version() {
        let current = env!("CARGO_PKG_VERSION");

        assert!(with_version(current).is_ok());
        assert!(with_version(next_version(current)).is_err());
        assert!(with_version(previous_version(current)).is_err());
    }
}
