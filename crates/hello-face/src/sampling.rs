//! Provisional acquisition checks on the detected face, not identity or PAD evidence.
use crate::{Detection, FaceError, Frame, PixelFormat, QualityMetrics};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const REFERENCE_SAMPLES: usize = 8;
pub const MIN_DETECTION_CONFIDENCE: f32 = 0.9;

/// Bounded intensity histogram, never identity or liveness evidence.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightHistogram {
    pub dark_fraction: f32,
    pub white_fraction: f32,
}
impl LightHistogram {
    pub fn measure(frame: &Frame<'_>) -> Result<Self, FaceError> {
        frame.validate()?;
        let step = (frame.width.max(frame.height) / 512).max(1);
        let mut total = 0_u32;
        let mut dark = 0_u32;
        let mut white = 0_u32;
        for y in (0..frame.height).step_by(step) {
            for x in (0..frame.width).step_by(step) {
                let value = frame.intensity(x, y);
                total += 1;
                dark += u32::from(value < 32.0 / 255.0);
                white += u32::from(value >= 248.0 / 255.0);
            }
        }
        Ok(Self {
            dark_fraction: dark as f32 / total as f32,
            white_fraction: white as f32 / total as f32,
        })
    }
    pub fn has_signal(&self) -> bool {
        self.dark_fraction.is_finite()
            && self.white_fraction.is_finite()
            && (0.0..=1.0).contains(&self.dark_fraction)
            && (0.0..=1.0).contains(&self.white_fraction)
            && self.dark_fraction < 0.995
            && self.white_fraction < 0.995
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleQuality {
    pub face_width: u32,
    pub face_height: u32,
    pub inter_eye_pixels: f32,
    pub coverage: f32,
    pub clearance: f32,
    pub clipped_fraction: f32,
    pub overexposed_fraction: f32,
    pub face_quality: QualityMetrics,
}

impl SampleQuality {
    pub fn measure(frame: &Frame<'_>, detection: &Detection) -> Result<Self, FaceError> {
        detection.validate(frame)?;
        let [x, y, width, height] = detection.bounds;
        let (left, top) = (x.ceil() as usize, y.ceil() as usize);
        let (right, bottom) = ((x + width).floor() as usize, (y + height).floor() as usize);
        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        if width < 16 || height < 16 {
            return Err(FaceError::PoorQuality);
        }
        // Exclude the outer 10% on each side so background/hair boundaries
        // cannot dominate exposure and detail measurements.
        let (inset_x, inset_y) = (width / 10, height / 10);
        let (detail_width, detail_height) = (width - 2 * inset_x, height - 2 * inset_y);
        if detail_width < 16 || detail_height < 16 {
            return Err(FaceError::PoorQuality);
        }
        let detail_pixels = detail_width * detail_height;
        let mut pixels = Zeroizing::new(Vec::with_capacity(detail_pixels));
        let mut clipped = 0usize;
        let mut overexposed = 0usize;
        for yy in top + inset_y..bottom - inset_y {
            for xx in left + inset_x..right - inset_x {
                let intensity = frame.intensity(xx, yy);
                clipped += usize::from(!(0.02..=0.98).contains(&intensity));
                overexposed += usize::from(intensity > 0.98);
                pixels.push((intensity * 255.0).round() as u8);
            }
        }
        let face = Frame {
            width: detail_width,
            height: detail_height,
            stride: detail_width,
            format: PixelFormat::Gray8,
            pixels: &pixels,
        };
        Ok(Self {
            face_width: width as u32,
            face_height: height as u32,
            inter_eye_pixels: ((detection.landmarks[0][0] - detection.landmarks[1][0]).powi(2)
                + (detection.landmarks[0][1] - detection.landmarks[1][1]).powi(2))
            .sqrt(),
            coverage: (width * height) as f32 / (frame.width * frame.height) as f32,
            clearance: (left as f32 / frame.width as f32)
                .min(top as f32 / frame.height as f32)
                .min((frame.width - right) as f32 / frame.width as f32)
                .min((frame.height - bottom) as f32 / frame.height as f32),
            clipped_fraction: clipped as f32 / detail_pixels as f32,
            overexposed_fraction: overexposed as f32 / detail_pixels as f32,
            face_quality: QualityMetrics::measure(&face)?,
        })
    }

    pub fn valid(&self) -> bool {
        self.inter_eye_pixels.is_finite()
            && (0.0..=2203.0).contains(&self.inter_eye_pixels)
            && self.inter_eye_pixels > 0.0
            && self.overexposed_fraction <= self.clipped_fraction
            && (16..=1920).contains(&self.face_width)
            && (16..=1080).contains(&self.face_height)
            && [
                self.coverage,
                self.clearance,
                self.clipped_fraction,
                self.overexposed_fraction,
                self.face_quality.mean_intensity,
                self.face_quality.contrast,
                self.face_quality.edge_energy,
            ]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }

    pub fn rejection(&self, confidence: f32) -> Option<&'static str> {
        if !self.valid() || !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Some("invalid face quality measurements");
        }
        if confidence < MIN_DETECTION_CONFIDENCE {
            return Some("face detection is uncertain; face the camera and uncover the lens");
        }
        if self.face_width < 80
            || self.face_height < 80
            || self.coverage < 0.04
            || self.inter_eye_pixels < 24.0
        {
            return Some("move closer so the camera sees more detail in your face");
        }
        if self.coverage > 0.65 || self.clearance < 0.015 {
            return Some("move back or toward the center; keep the whole face inside the image");
        }
        if self.face_quality.mean_intensity < 0.08 {
            return Some("face exposure is too dark; adjust lighting or IR mode");
        }
        if self.face_quality.mean_intensity > 0.92 {
            return Some("face exposure is too bright; adjust lighting or IR mode");
        }
        if self.overexposed_fraction > 0.12 || self.clipped_fraction > 0.60 {
            return Some(
                "face exposure contains too many clipped pixels; adjust lighting or IR distance",
            );
        }
        // Relative detail is less sensitive to exposure than a fixed raw edge cutoff.
        if self.face_quality.contrast < 0.03
            || self.face_quality.edge_energy < 0.005
            || self.face_quality.edge_energy / self.face_quality.contrast.max(0.001) < 0.07
        {
            return Some("face detail is too soft; hold still and check the lens");
        }
        None
    }
}

/// Position instructions are user guidance, not measured pose coverage or liveness.
pub fn sample_instruction(accepted: usize) -> &'static str {
    match accepted {
        0 => "Look straight at the camera and hold still.",
        1 => "Keep looking straight; move slightly closer with your whole face visible.",
        2 => "Keep looking straight; move slightly farther away, keeping your face large enough.",
        3 => "Turn slightly toward the left edge of the preview, keeping both eyes visible.",
        4 => "Turn slightly toward the right edge of the preview, keeping both eyes visible.",
        5 => "Raise your chin slightly and hold still.",
        6 => "Lower your chin slightly and hold still.",
        7 => "Look straight at the camera again and hold still.",
        _ => "Reference complete. Open Settings to save it encrypted.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quality() -> SampleQuality {
        SampleQuality {
            face_width: 128,
            face_height: 128,
            inter_eye_pixels: 45.0,
            coverage: 0.2,
            clearance: 0.1,
            clipped_fraction: 0.01,
            overexposed_fraction: 0.01,
            face_quality: QualityMetrics {
                mean_intensity: 0.5,
                contrast: 0.1,
                edge_energy: 0.1,
            },
        }
    }
    #[test]
    fn dark_hair_and_dim_but_detailed_face_are_not_white_clipping_or_blur() {
        let mut q = quality();
        q.clipped_fraction = 0.24;
        q.overexposed_fraction = 0.01;
        q.face_quality.mean_intensity = 0.10;
        q.face_quality.contrast = 0.07;
        q.face_quality.edge_energy = 0.006;
        assert!(q.rejection(0.99).is_none());
        q.overexposed_fraction = 0.20;
        assert!(q.rejection(0.99).unwrap().contains("exposure"));
        q.overexposed_fraction = 0.01;
        q.face_quality.edge_energy = 0.004;
        assert!(q.rejection(0.99).unwrap().contains("soft"));
        q.face_quality.edge_energy = 0.006;
        q.face_quality.contrast = 0.20;
        assert!(q.rejection(0.99).unwrap().contains("soft"));
    }
    #[test]
    fn histogram_rejects_blank_images_but_allows_dark_background_with_face_signal() {
        for value in [0_u8, 255] {
            let pixels = vec![value; 32 * 32];
            let frame = Frame {
                width: 32,
                height: 32,
                stride: 32,
                format: PixelFormat::Gray8,
                pixels: &pixels,
            };
            assert!(!LightHistogram::measure(&frame).unwrap().has_signal());
        }
        let mut pixels = vec![0_u8; 32 * 32];
        for row in 12..20 {
            for x in 12..20 {
                pixels[row * 32 + x] = if x % 2 == 0 { 140 } else { 180 };
            }
        }
        let frame = Frame {
            width: 32,
            height: 32,
            stride: 32,
            format: PixelFormat::Gray8,
            pixels: &pixels,
        };
        assert!(LightHistogram::measure(&frame).unwrap().has_signal());
        let metrics = QualityMetrics::measure(&frame).unwrap();
        assert!(metrics.has_signal());
        assert!(!metrics.acceptable());
    }
    #[test]
    fn detailed_ir_face_below_old_width_cutoff_is_usable_but_undersampled_eyes_are_not() {
        let mut q = quality();
        q.face_width = 91;
        q.face_height = 110;
        q.coverage = 0.0434;
        q.face_quality = QualityMetrics {
            mean_intensity: 0.303,
            contrast: 0.062,
            edge_energy: 0.0102,
        };
        assert!(q.rejection(0.903).is_none());
        q.inter_eye_pixels = 23.0;
        assert!(q.rejection(0.903).unwrap().contains("closer"));
        q.inter_eye_pixels = f32::NAN;
        assert!(!q.valid());
    }
    #[test]
    fn enrollment_rejects_small_clipped_soft_uncertain_faces_and_nonfinite_data() {
        assert!(quality().rejection(0.99).is_none());
        assert!(quality().rejection(0.9089786).is_none());
        let mut q = quality();
        q.face_width = 79;
        assert!(q.rejection(0.99).unwrap().contains("closer"));
        let mut q = quality();
        q.clearance = 0.0;
        assert!(q.rejection(0.99).unwrap().contains("center"));
        let mut q = quality();
        q.clipped_fraction = 0.2;
        q.overexposed_fraction = 0.2;
        assert!(q.rejection(0.99).unwrap().contains("exposure"));
        let mut q = quality();
        q.face_quality.edge_energy = 0.001;
        assert!(q.rejection(0.99).unwrap().contains("soft"));
        assert!(quality().rejection(0.89).unwrap().contains("uncertain"));
        let mut q = quality();
        q.coverage = f32::NAN;
        assert!(!q.valid());
    }
    #[test]
    fn detailed_background_and_box_border_cannot_mask_an_unusable_face_region() {
        let mut pixels = vec![0; 256 * 256];
        for y in 0..256 {
            for x in 0..256 {
                pixels[y * 256 + x] = if (x + y) % 2 == 0 { 50 } else { 200 };
            }
        }
        for y in 76..180 {
            for x in 76..180 {
                pixels[y * 256 + x] = 127;
            }
        }
        let frame = Frame {
            width: 256,
            height: 256,
            stride: 256,
            format: PixelFormat::Gray8,
            pixels: &pixels,
        };
        let detection = Detection {
            bounds: [64.0, 64.0, 128.0, 128.0],
            confidence: 0.99,
            landmarks: [
                [100.0, 100.0],
                [135.0, 100.0],
                [118.0, 128.0],
                [103.0, 158.0],
                [134.0, 158.0],
            ],
        };
        assert!(QualityMetrics::measure(&frame).unwrap().acceptable());
        let quality = SampleQuality::measure(&frame, &detection).unwrap();
        assert_eq!(quality.face_width, 128);
        assert!(quality.rejection(0.99).unwrap().contains("soft"));
    }
}
