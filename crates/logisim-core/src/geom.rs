//! Grid geometry: locations and facing directions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    fn rotation() {
        let o = Location::new(-30, 10);
        assert_eq!(Direction::East.rotate(o), o);
        assert_eq!(Direction::West.rotate(o), Location::new(30, -10));
        assert_eq!(Direction::North.rotate(o), Location::new(10, 30));
        assert_eq!(Direction::South.rotate(o), Location::new(-10, -30));
    }
}
