use bevy::prelude::*;
use fixed::types::I48F16;
use serde::{Deserialize, Serialize};

pub type FixedNum = I48F16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixedVec2 {
    pub x: FixedNum,
    pub y: FixedNum,
}

impl FixedVec2 {
    pub const ZERO: Self = Self { x: FixedNum::ZERO, y: FixedNum::ZERO };

    pub fn new(x: FixedNum, y: FixedNum) -> Self {
        Self { x, y }
    }

    pub fn from_f32(x: f32, y: f32) -> Self {
        Self {
            x: FixedNum::from_num(x),
            y: FixedNum::from_num(y),
        }
    }

    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x.to_num(), self.y.to_num())
    }

    pub fn length(self) -> FixedNum {
        let len_sq = self.length_squared();
        if len_sq == FixedNum::ZERO {
            return FixedNum::ZERO;
        }
        len_sq.sqrt()
    }

    pub fn length_squared(self) -> FixedNum {
        self.x * self.x + self.y * self.y
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len == FixedNum::ZERO {
            Self::ZERO
        } else {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        }
    }
    
    #[allow(dead_code)]
    pub fn dot(self, other: Self) -> FixedNum {
        self.x * other.x + self.y * other.y
    }

    #[allow(dead_code)]
    pub fn cross(self, other: Self) -> FixedNum {
        self.x * other.y - self.y * other.x
    }
}

impl std::ops::Add for FixedVec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl std::ops::Sub for FixedVec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl std::ops::Mul<FixedNum> for FixedVec2 {
    type Output = Self;
    fn mul(self, rhs: FixedNum) -> Self::Output {
        Self { x: self.x * rhs, y: self.y * rhs }
    }
}

impl std::ops::Div<FixedNum> for FixedVec2 {
    type Output = Self;
    fn div(self, rhs: FixedNum) -> Self::Output {
        Self { x: self.x / rhs, y: self.y / rhs }
    }
}

impl std::ops::Neg for FixedVec2 {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self { x: -self.x, y: -self.y }
    }
}
