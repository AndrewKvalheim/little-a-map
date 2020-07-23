use derivative::Derivative;

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
