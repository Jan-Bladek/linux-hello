//! Five-landmark geometry and an experimental motion challenge, not certified PAD.
use crate::{Detection, FaceError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize, Zeroize)]
#[serde(deny_unknown_fields)]
pub struct FaceGeometry {
    ratios: [f32; 6],
    yaw: f32,
}
impl Drop for FaceGeometry {
    fn drop(&mut self) {
        self.zeroize();
    }
}
impl FaceGeometry {
    pub fn measure(d: &Detection) -> Result<Self, FaceError> {
        let [a, b, nose, c, e] = d.landmarks;
        let length =
            |p: [f32; 2], q: [f32; 2]| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
        let eyes = length(a, b);
        if !eyes.is_finite() || eyes < 8.0 {
            return Err(FaceError::InvalidDetection);
        }
        let middle = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        // Orient left-to-right in image coordinates so direction is stable.
        let sign = if b[0] >= a[0] { 1.0 } else { -1.0 };
        let axis = [(b[0] - a[0]) * sign / eyes, (b[1] - a[1]) * sign / eyes];
        let yaw = ((nose[0] - middle[0]) * axis[0] + (nose[1] - middle[1]) * axis[1]) / eyes;
        let mouth = [(c[0] + e[0]) * 0.5, (c[1] + e[1]) * 0.5];
        let result = Self {
            ratios: [
                length(c, e) / eyes,
                length(middle, nose) / eyes,
                length(middle, mouth) / eyes,
                length(nose, mouth) / eyes,
                eyes / d.bounds[2],
                d.bounds[3] / d.bounds[2],
            ],
            yaw,
        };
        if !result.valid() {
            return Err(FaceError::InvalidDetection);
        }
        Ok(result)
    }
    pub fn valid(&self) -> bool {
        self.ratios
            .iter()
            .all(|v| v.is_finite() && *v > 0.0 && *v <= 4.0)
            && self.yaw.is_finite()
            && self.yaw.abs() <= 1.0
    }
    pub fn yaw(&self) -> f32 {
        self.yaw
    }
    pub fn compatible(&self, other: &Self) -> bool {
        let differences = self
            .ratios
            .iter()
            .zip(other.ratios.iter())
            .map(|(a, b)| (a - b).abs());
        let mut squared = 0.0;
        for d in differences {
            if d > 0.30 {
                return false;
            }
            squared += d * d;
        }
        squared / 6.0 <= 0.15 * 0.15
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detection() -> Detection {
        Detection {
            bounds: [0.0, 0.0, 200.0, 240.0],
            landmarks: [
                [50.0, 60.0],
                [150.0, 60.0],
                [100.0, 120.0],
                [65.0, 180.0],
                [135.0, 180.0],
            ],
            confidence: 0.99,
        }
    }
    #[test]
    fn geometry_is_scale_translation_and_roll_invariant() {
        let d = detection();
        let original = FaceGeometry::measure(&d).unwrap();
        let mut moved = detection();
        moved.bounds = [20.0, 30.0, 400.0, 480.0];
        for p in &mut moved.landmarks {
            p[0] = p[0] * 2.0 + 20.0;
            p[1] = p[1] * 2.0 + 30.0;
        }
        let geometry = FaceGeometry::measure(&moved).unwrap();
        assert!(original.compatible(&geometry));
        assert!((original.yaw() - geometry.yaw()).abs() < 1e-6);
        let mut rolled = detection();
        for p in &mut rolled.landmarks {
            let x = p[0] - 100.0;
            let y = p[1] - 120.0;
            p[0] = 100.0 + x * 0.98 - y * 0.199;
            p[1] = 120.0 + x * 0.199 + y * 0.98;
        }
        assert!(FaceGeometry::measure(&rolled).unwrap().yaw().abs() < 1e-5);
    }
    #[test]
    fn degenerate_and_different_shapes_are_rejected() {
        let a = FaceGeometry::measure(&detection()).unwrap();
        let mut d = detection();
        d.landmarks[3] = [15.0, 180.0];
        d.landmarks[4] = [185.0, 180.0];
        assert!(!a.compatible(&FaceGeometry::measure(&d).unwrap()));
        d.landmarks[1] = d.landmarks[0];
        assert!(FaceGeometry::measure(&d).is_err());
    }
}
