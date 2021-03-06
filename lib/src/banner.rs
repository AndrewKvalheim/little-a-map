use derivative::Derivative;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Derivative, Eq, PartialOrd, Ord)]
#[derivative(PartialEq)]
pub struct Banner {
    #[derivative(PartialEq = "ignore")]
    pub label: Option<String>,

    #[derivative(PartialEq = "ignore")]
    pub color: String,

    pub x: i32,
    pub z: i32,
}

impl<'de> Deserialize<'de> for Banner {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            color: String,
            #[serde(default)]
            #[serde(with = "serde_with::json::nested")]
            name: Option<Name>,
            pos: Pos,
        }

        #[derive(Deserialize)]
        struct Name {
            text: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Pos {
            x: i32,
            z: i32,
        }

        let internal = Internal::deserialize(deserializer)?;
        Ok(Self {
            color: internal.color,
            label: internal.name.map(|n| n.text),
            x: internal.pos.x,
            z: internal.pos.z,
        })
    }
}
