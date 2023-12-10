use derivative::Derivative;
use serde::{Deserialize, Deserializer};
use serde_with::{json::JsonString, serde_as};

#[derive(Debug, Derivative, Eq, Ord, PartialOrd)]
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
        #[serde_as]
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Internal {
            color: String,
            #[serde_as(as = "Option<JsonString<_>>")]
            name: Option<Name>,
            pos: Pos,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Name {
            V1203(V1203),
            V1204(String),
        }

        #[derive(Deserialize)]
        struct V1203 {
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
            label: internal.name.map(|name| match name {
                Name::V1203(n) => n.text,
                Name::V1204(n) => n,
            }),
            x: internal.pos.x,
            z: internal.pos.z,
        })
    }
}
