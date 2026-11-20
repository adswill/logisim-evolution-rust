//! Grid geometry: locations and facing directions.

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct Location {
    pub x: i32,
    pub y: i32,
}

impl Location {
    pub const fn new(x: i32, y: i32) -> Self {
        Location { x, y }
    }
    pub fn translate(self, dx: i32, dy: i32) -> Self {
        Location::new(self.x + dx, self.y + dy)
    }
    pub fn manhattan_distance(self, o: Location) -> i32 {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }
}

/// Which way a component faces. East is the unrotated orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    East,
    North,
    West,
    South,
}

impl Direction {
    /// Rotate an east-facing offset to this facing (Logisim convention).
    pub fn rotate(self, off: Location) -> Location {
        let Location { x, y } = off;
        match self {
            Direction::East => Location::new(x, y),
            Direction::West => Location::new(-x, -y),
            Direction::North => Location::new(y, -x),
            Direction::South => Location::new(-y, x),
        }
    }
    /// Place a part-local offset: optionally mirror it top-to-bottom (y -> -y), then rotate to
    /// this facing. This is the one rule for turning east-facing port offsets into world offsets.
    pub fn place(self, mirror: bool, off: Location) -> Location {
        let off = if mirror {
            Location::new(off.x, -off.y)
        } else {
            off
        };
        self.rotate(off)
    }

    pub fn reverse(self) -> Direction {
        match self {
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirror_flips_y_before_rotating() {
        let o = Location::new(-30, 10);
        for d in [
            Direction::East,
            Direction::North,
            Direction::West,
            Direction::South,
        ] {
            assert_eq!(d.place(false, o), d.rotate(o));
            assert_eq!(d.place(true, o), d.rotate(Location::new(-30, -10)));
        }
        // east + mirror just negates y
        assert_eq!(Direction::East.place(true, o), Location::new(-30, -10));
        // mirroring is its own inverse
        for d in [
            Direction::East,
            Direction::North,
            Direction::West,
            Direction::South,
        ] {
            let once = d.place(true, o);
            let back = d.place(true, Location::new(o.x, -o.y));
            assert_eq!(back, d.rotate(o));
            assert_ne!(once, back);
        }
    }

    #[test]
    fn rotation() {
        let o = Location::new(-30, 10);
        assert_eq!(Direction::East.rotate(o), o);
        assert_eq!(Direction::West.rotate(o), Location::new(30, -10));
        assert_eq!(Direction::North.rotate(o), Location::new(10, 30));
        assert_eq!(Direction::South.rotate(o), Location::new(-10, -30));
    }
}
