use crate::tile::Tile;
use derivative::Derivative;
use filetime::FileTime;

#[derive(Debug, Derivative, Eq)]
#[derivative(Ord, PartialEq, PartialOrd)]
pub struct Map {
    pub modified: FileTime,

    pub id: u32,

    #[derivative(Ord = "ignore")]
    #[derivative(PartialEq = "ignore")]
    #[derivative(PartialOrd = "ignore")]
    pub tile: Tile,
}

#[cfg(test)]
mod test {
    use super::*;
    use std::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn compare() {
        fn map(id: u32, s: i64, x: i32) -> Map {
            Map {
                id,
                modified: FileTime::from_unix_time(s, 0),
                tile: Tile::new(0, x, 0),
            }
        }

        // Identical
        assert_eq!(map(0, 0, 0), map(0, 0, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 0, 0)), Equal);

        // Ignore tile
        assert_eq!(map(0, 0, 0), map(0, 0, 1));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 0, 1)), Equal);

        // Differ by ID
        assert_ne!(map(0, 0, 0), map(1, 0, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(1, 0, 0)), Less);

        // Differ by modification time
        assert_ne!(map(0, 0, 0), map(0, 1, 0));
        assert_eq!(map(0, 0, 0).cmp(&map(0, 1, 0)), Less);

        // Sort first by modification time, then by ID
        assert_eq!(map(0, 1, 0).cmp(&map(1, 0, 0)), Greater);
        assert_eq!(map(1, 0, 0).cmp(&map(0, 1, 0)), Less);
    }
}
