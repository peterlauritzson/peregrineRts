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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_vec2_zero() {
        let zero = FixedVec2::ZERO;
        assert_eq!(zero.x, FixedNum::ZERO);
        assert_eq!(zero.y, FixedNum::ZERO);
    }

    #[test]
    fn test_fixed_vec2_new() {
        let v = FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(4.0));
        assert_eq!(v.x, FixedNum::from_num(3.0));
        assert_eq!(v.y, FixedNum::from_num(4.0));
    }

    #[test]
    fn test_fixed_vec2_from_f32() {
        let v = FixedVec2::from_f32(3.5, 4.5);
        assert_eq!(v.x, FixedNum::from_num(3.5));
        assert_eq!(v.y, FixedNum::from_num(4.5));
    }

    #[test]
    fn test_fixed_vec2_to_vec2() {
        let v = FixedVec2::from_f32(3.0, 4.0);
        let v2 = v.to_vec2();
        assert_eq!(v2.x, 3.0);
        assert_eq!(v2.y, 4.0);
    }

    #[test]
    fn test_fixed_vec2_length() {
        // 3-4-5 triangle
        let v = FixedVec2::from_f32(3.0, 4.0);
        let len = v.length();
        let expected = FixedNum::from_num(5.0);
        // Allow small fixed-point error
        let diff = (len - expected).abs();
        assert!(diff < FixedNum::from_num(0.001), "Length should be ~5.0, got {}", len);
    }

    #[test]
    fn test_fixed_vec2_length_zero() {
        let v = FixedVec2::ZERO;
        assert_eq!(v.length(), FixedNum::ZERO);
    }

    #[test]
    fn test_fixed_vec2_length_squared() {
        let v = FixedVec2::from_f32(3.0, 4.0);
        assert_eq!(v.length_squared(), FixedNum::from_num(25.0));
    }

    #[test]
    fn test_fixed_vec2_normalize() {
        let v = FixedVec2::from_f32(3.0, 4.0);
        let normalized = v.normalize();
        let len = normalized.length();
        let diff = (len - FixedNum::from_num(1.0)).abs();
        assert!(diff < FixedNum::from_num(0.001), "Normalized vector should have length 1.0, got {}", len);
    }

    #[test]
    fn test_fixed_vec2_normalize_zero() {
        let v = FixedVec2::ZERO;
        let normalized = v.normalize();
        assert_eq!(normalized, FixedVec2::ZERO, "Normalizing zero vector should return zero");
    }

    #[test]
    fn test_fixed_vec2_add() {
        let a = FixedVec2::from_f32(1.0, 2.0);
        let b = FixedVec2::from_f32(3.0, 4.0);
        let c = a + b;
        assert_eq!(c.x, FixedNum::from_num(4.0));
        assert_eq!(c.y, FixedNum::from_num(6.0));
    }

    #[test]
    fn test_fixed_vec2_sub() {
        let a = FixedVec2::from_f32(5.0, 7.0);
        let b = FixedVec2::from_f32(2.0, 3.0);
        let c = a - b;
        assert_eq!(c.x, FixedNum::from_num(3.0));
        assert_eq!(c.y, FixedNum::from_num(4.0));
    }

    #[test]
    fn test_fixed_vec2_mul_scalar() {
        let v = FixedVec2::from_f32(2.0, 3.0);
        let scaled = v * FixedNum::from_num(2.0);
        assert_eq!(scaled.x, FixedNum::from_num(4.0));
        assert_eq!(scaled.y, FixedNum::from_num(6.0));
    }

    #[test]
    fn test_fixed_vec2_div_scalar() {
        let v = FixedVec2::from_f32(6.0, 8.0);
        let scaled = v / FixedNum::from_num(2.0);
        assert_eq!(scaled.x, FixedNum::from_num(3.0));
        assert_eq!(scaled.y, FixedNum::from_num(4.0));
    }

    #[test]
    fn test_fixed_vec2_neg() {
        let v = FixedVec2::from_f32(3.0, -4.0);
        let negated = -v;
        assert_eq!(negated.x, FixedNum::from_num(-3.0));
        assert_eq!(negated.y, FixedNum::from_num(4.0));
    }

    #[test]
    fn test_fixed_vec2_dot() {
        let a = FixedVec2::from_f32(2.0, 3.0);
        let b = FixedVec2::from_f32(4.0, 5.0);
        let dot = a.dot(b);
        // 2*4 + 3*5 = 8 + 15 = 23
        assert_eq!(dot, FixedNum::from_num(23.0));
    }

    #[test]
    fn test_fixed_vec2_dot_perpendicular() {
        let a = FixedVec2::from_f32(1.0, 0.0);
        let b = FixedVec2::from_f32(0.0, 1.0);
        let dot = a.dot(b);
        assert_eq!(dot, FixedNum::ZERO, "Perpendicular vectors should have dot product of 0");
    }

    #[test]
    fn test_fixed_vec2_cross() {
        let a = FixedVec2::from_f32(2.0, 3.0);
        let b = FixedVec2::from_f32(4.0, 5.0);
        let cross = a.cross(b);
        // 2*5 - 3*4 = 10 - 12 = -2
        assert_eq!(cross, FixedNum::from_num(-2.0));
    }

    #[test]
    fn test_fixed_vec2_cross_parallel() {
        let a = FixedVec2::from_f32(2.0, 4.0);
        let b = FixedVec2::from_f32(1.0, 2.0);
        let cross = a.cross(b);
        assert_eq!(cross, FixedNum::ZERO, "Parallel vectors should have cross product of 0");
    }
}
