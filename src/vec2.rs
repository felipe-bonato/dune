use std::{cmp::min, ops};

type Vec2Coord = usize;

// const SATURATION_MIN: usize = u16::MIN as usize;
const SATURATION_MAX: usize = u16::MAX as usize;

#[derive(Clone, Copy, Debug)]
pub struct Vec2(pub Vec2Coord, pub Vec2Coord);

pub const ZERO: Vec2 = Vec2(0, 0);
pub const ONE: Vec2 = Vec2(1, 1);

impl Vec2 {}

impl ops::Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Vec2) -> <Self as ops::Sub<Vec2>>::Output {
        Vec2(self.0.saturating_sub(rhs.0), self.1.saturating_sub(rhs.1))
    }
}

impl ops::Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> <Self as ops::Sub<Vec2>>::Output {
        Vec2(
            min(self.0.add(rhs.0), SATURATION_MAX),
            min(self.1.add(rhs.1), SATURATION_MAX),
        )
    }
}

impl From<Vec2> for ops::Range<Vec2Coord> {
    fn from(val: Vec2) -> Self {
        ops::Range {
            start: val.0,
            end: val.1,
        }
    }
}
