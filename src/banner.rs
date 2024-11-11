use derivative::Derivative;
use fastnbt::IntArray;
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
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Internal {
            V1204(InternalV1204),
            V1205(InternalV1205),
        }

        #[serde_as]
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct InternalV1204 {
            color: String,
            #[serde_as(as = "Option<JsonString<_>>")]
            name: Option<Name>,
            pos: Pos,
        }

        #[serde_as]
        #[derive(Deserialize)]
        struct InternalV1205 {
            #[serde(default = "default_color")]
            color: String,
            #[serde_as(as = "Option<JsonString<_>>")]
            name: Option<String>,
            pos: IntArray,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Name {
            V1203(NameV1203),
            V1204(String),
        }

        #[derive(Deserialize)]
        struct NameV1203 {
            text: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Pos {
            x: i32,
            z: i32,
        }

        fn default_color() -> String {
            "white".to_owned()
        }

        Ok(match Internal::deserialize(deserializer)? {
            Internal::V1204(i) => Self {
                color: i.color,
                label: i.name.and_then(|name| match name {
                    Name::V1203(n) => n.text,
                    Name::V1204(n) => Some(n),
                }),
                x: i.pos.x,
                z: i.pos.z,
            },
            Internal::V1205(i) => Self {
                color: i.color,
                label: i.name,
                x: i.pos[0],
                z: i.pos[2],
            },
        })
    }
}
