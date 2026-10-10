//! Model-independent biometric primitives. No inference model is bundled.
//! All thresholds below are diagnostic policy, not measured security claims.

use serde::{Deserialize, Serialize, ser::SerializeStruct};
use thiserror::Error;
use zeroize::Zeroizing;

pub const MAX_EMBEDDING_DIMENSIONS: usize = 2048;
pub const MAX_TEMPLATE_BYTES: usize = 64 * 1024;
pub const MIN_ENROLLMENT_SAMPLES: usize = 3;
pub const MAX_ENROLLMENT_SAMPLES: usize = 12;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum FaceError {
    #[error("no licensed and evaluated inference adapter is configured")]
    Unsupported,
    #[error("frame dimensions, stride, or byte count are invalid")]
    InvalidFrame,
    #[error("embedding is non-finite, empty, too large, or has zero norm")]
    InvalidEmbedding,
    #[error("embeddings have incompatible dimensions or model identities")]
    IncompatibleEmbedding,
    #[error("policy threshold is invalid")]
    InvalidThreshold,
    #[error("sample quality is insufficient")]
    PoorQuality,
    #[error("exactly one face is required")]
    FaceCount,
    #[error("sample was already enrolled")]
    DuplicateSample,
    #[error("sample is inconsistent with the enrollment")]
    InconsistentSample,
    #[error("enrollment sample count is outside policy")]
    SampleCount,
    #[error("stored template is malformed or exceeds limits")]
    InvalidTemplate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Gray8,
    Rgb8,
}

/// The borrowed frame never takes ownership or saves camera bytes.
pub struct Frame<'a> {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub format: PixelFormat,
    pub pixels: &'a [u8],
}

impl Frame<'_> {
    pub fn validate(&self) -> Result<(), FaceError> {
        let channels = if self.format == PixelFormat::Gray8 {
            1
        } else {
            3
        };
        if self.width < 16 || self.height < 16 || self.width > 4096 || self.height > 4096 {
            return Err(FaceError::InvalidFrame);
        }
        let minimum_stride = self
            .width
            .checked_mul(channels)
            .ok_or(FaceError::InvalidFrame)?;
        let expected = self
            .stride
            .checked_mul(self.height)
            .ok_or(FaceError::InvalidFrame)?;
        if self.stride < minimum_stride
            || self.stride > minimum_stride + 4096
            || self.pixels.len() != expected
        {
            return Err(FaceError::InvalidFrame);
        }
        Ok(())
    }

    fn intensity(&self, x: usize, y: usize) -> f64 {
        let offset = y * self.stride;
        match self.format {
            PixelFormat::Gray8 => f64::from(self.pixels[offset + x]) / 255.0,
            PixelFormat::Rgb8 => {
                let offset = offset + x * 3;
                (0.2126 * f64::from(self.pixels[offset])
                    + 0.7152 * f64::from(self.pixels[offset + 1])
                    + 0.0722 * f64::from(self.pixels[offset + 2]))
                    / 255.0
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub mean_intensity: f32,
    pub contrast: f32,
    pub edge_energy: f32,
}

impl QualityMetrics {
    /// Bounded image statistics, independent of identity and liveness.
    pub fn measure(frame: &Frame<'_>) -> Result<Self, FaceError> {
        frame.validate()?;
        let step = (frame.width.max(frame.height) / 512).max(1);
        let mut total = 0.0_f64;
        let mut squared = 0.0_f64;
        let mut edge = 0.0_f64;
        let mut count = 0_u32;
        for y in (1..frame.height).step_by(step) {
            for x in (1..frame.width).step_by(step) {
                let value = frame.intensity(x, y);
                total += value;
                squared += value * value;
                edge += ((value - frame.intensity(x - 1, y)).abs()
                    + (value - frame.intensity(x, y - 1)).abs())
                    / 2.0;
                count += 1;
            }
        }
        let mean = total / f64::from(count);
        Ok(Self {
            mean_intensity: mean as f32,
            contrast: (squared / f64::from(count) - mean * mean).max(0.0).sqrt() as f32,
            edge_energy: (edge / f64::from(count)) as f32,
        })
    }

    pub fn acceptable(&self) -> bool {
        [self.mean_intensity, self.contrast, self.edge_energy]
            .iter()
            .all(|v| v.is_finite())
            && (0.08..=0.92).contains(&self.mean_intensity)
            && (0.03..=0.5).contains(&self.contrast)
            && (0.005..=1.0).contains(&self.edge_energy)
    }
}

/// Model identity is the SHA-256 digest of the exact, separately licensed model.
/// Embeddings are redacted from Debug and zeroized when dropped.
pub struct Embedding {
    model_digest: [u8; 32],
    values: Zeroizing<Vec<f32>>,
}

impl Embedding {
    pub fn new(model_digest: [u8; 32], values: Vec<f32>) -> Result<Self, FaceError> {
        let mut values = Zeroizing::new(values);
        if values.is_empty()
            || values.len() > MAX_EMBEDDING_DIMENSIONS
            || values.iter().any(|v| !v.is_finite())
        {
            return Err(FaceError::InvalidEmbedding);
        }
        let norm = values
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt();
        if !norm.is_finite() || norm < 1e-12 {
            return Err(FaceError::InvalidEmbedding);
        }
        for value in values.iter_mut() {
            *value = (f64::from(*value) / norm) as f32;
        }
        Ok(Self {
            model_digest,
            values,
        })
    }

    pub fn dimensions(&self) -> usize {
        self.values.len()
    }

    pub fn model_digest(&self) -> [u8; 32] {
        self.model_digest
    }

    pub fn similarity(&self, other: &Self) -> Result<f32, FaceError> {
        if self.model_digest != other.model_digest || self.dimensions() != other.dimensions() {
            return Err(FaceError::IncompatibleEmbedding);
        }
        let value = self
            .values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum::<f64>();
        Ok(value.clamp(-1.0, 1.0) as f32)
    }
}

impl Serialize for Embedding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Embedding", 2)?;
        state.serialize_field("model_digest", &self.model_digest)?;
        state.serialize_field("values", &*self.values)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Embedding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            model_digest: [u8; 32],
            values: SecretValues,
        }
        let mut wire = Wire::deserialize(deserializer)?;
        Self::new(wire.model_digest, std::mem::take(&mut *wire.values.0))
            .map_err(serde::de::Error::custom)
    }
}

/// A malicious JSON array is rejected before allocating arbitrary dimensions;
/// values already parsed are zeroized even if a later field or token is invalid.
struct SecretValues(Zeroizing<Vec<f32>>);

impl<'de> Deserialize<'de> for SecretValues {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SecretValues;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded finite embedding")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Zeroizing::new(Vec::with_capacity(MAX_EMBEDDING_DIMENSIONS));
                while let Some(value) = sequence.next_element::<f32>()? {
                    if values.len() == MAX_EMBEDDING_DIMENSIONS || !value.is_finite() {
                        return Err(serde::de::Error::custom(
                            "invalid embedding dimensions or value",
                        ));
                    }
                    values.push(value);
                }
                Ok(SecretValues(values))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Serialize)]
pub struct Template {
    version: u8,
    sample_count: usize,
    centroid: Embedding,
}

impl Template {
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn model_digest(&self) -> [u8; 32] {
        self.centroid.model_digest()
    }

    pub fn compare(&self, probe: &Embedding, threshold: f32) -> Result<MatchResult, FaceError> {
        validate_threshold(threshold)?;
        let similarity = self.centroid.similarity(probe)?;
        Ok(MatchResult {
            similarity,
            accepted: similarity >= threshold,
        })
    }

    pub fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, FaceError> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|_| FaceError::InvalidTemplate)
    }

    /// Byte cap is applied before deserialization; fields are validated afterward.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FaceError> {
        if bytes.len() > MAX_TEMPLATE_BYTES {
            return Err(FaceError::InvalidTemplate);
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u8,
            sample_count: usize,
            centroid: Embedding,
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| FaceError::InvalidTemplate)?;
        if wire.version != 1
            || !(MIN_ENROLLMENT_SAMPLES..=MAX_ENROLLMENT_SAMPLES).contains(&wire.sample_count)
        {
            return Err(FaceError::InvalidTemplate);
        }
        Ok(Self {
            version: wire.version,
            sample_count: wire.sample_count,
            centroid: wire.centroid,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MatchResult {
    pub similarity: f32,
    /// Local mathematical match only; this is not an authentication verdict.
    pub accepted: bool,
}

pub struct Enrollment {
    samples: Vec<Embedding>,
    sample_ids: Vec<[u8; 32]>,
    minimum_consistency: f32,
}

impl Enrollment {
    pub fn new(minimum_consistency: f32) -> Result<Self, FaceError> {
        validate_threshold(minimum_consistency)?;
        Ok(Self {
            samples: Vec::new(),
            sample_ids: Vec::new(),
            minimum_consistency,
        })
    }

    /// `sample_id` must be a digest of the freshly acquired frame; the trusted
    /// capture adapter, not an IPC client, supplies it and the quality metrics.
    pub fn add(
        &mut self,
        sample_id: [u8; 32],
        embedding: Embedding,
        faces: usize,
        quality: QualityMetrics,
    ) -> Result<(), FaceError> {
        if self.samples.len() >= MAX_ENROLLMENT_SAMPLES {
            return Err(FaceError::SampleCount);
        }
        if faces != 1 {
            return Err(FaceError::FaceCount);
        }
        if !quality.acceptable() {
            return Err(FaceError::PoorQuality);
        }
        if self.sample_ids.contains(&sample_id) {
            return Err(FaceError::DuplicateSample);
        }
        for prior in &self.samples {
            if prior.similarity(&embedding)? < self.minimum_consistency {
                return Err(FaceError::InconsistentSample);
            }
        }
        self.sample_ids.push(sample_id);
        self.samples.push(embedding);
        Ok(())
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn finish(self) -> Result<Template, FaceError> {
        if self.samples.len() < MIN_ENROLLMENT_SAMPLES {
            return Err(FaceError::SampleCount);
        }
        let first = &self.samples[0];
        let mut values = Zeroizing::new(vec![0_f32; first.dimensions()]);
        for sample in &self.samples {
            for (sum, value) in values.iter_mut().zip(sample.values.iter()) {
                *sum += *value;
            }
        }
        // Ownership transfer avoids leaving an unzeroized second embedding.
        let values = std::mem::take(&mut *values);
        let centroid = Embedding::new(first.model_digest, values)?;
        Ok(Template {
            version: 1,
            sample_count: self.samples.len(),
            centroid,
        })
    }
}

fn validate_threshold(value: f32) -> Result<(), FaceError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(FaceError::InvalidThreshold);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Detection {
    pub bounds: [f32; 4],
    pub landmarks: [[f32; 2]; 5],
    pub confidence: f32,
}

/// Normalized model input; adapters must validate their fixed input shape.
pub struct AlignedFace {
    pub width: u16,
    pub height: u16,
    pub channels: u8,
    pub values: Zeroizing<Vec<f32>>,
}

pub trait FaceEngine: Send {
    fn model_digest(&self) -> Result<[u8; 32], FaceError>;
    fn detect(&mut self, frame: &Frame<'_>) -> Result<Vec<Detection>, FaceError>;
    fn align(&mut self, frame: &Frame<'_>, detection: &Detection)
    -> Result<AlignedFace, FaceError>;
    fn embed(&mut self, aligned: &AlignedFace) -> Result<Embedding, FaceError>;
}

/// Explicitly unavailable integration boundary, never a synthetic inference engine.
pub struct UnconfiguredEngine;

impl FaceEngine for UnconfiguredEngine {
    fn model_digest(&self) -> Result<[u8; 32], FaceError> {
        Err(FaceError::Unsupported)
    }
    fn detect(&mut self, _: &Frame<'_>) -> Result<Vec<Detection>, FaceError> {
        Err(FaceError::Unsupported)
    }
    fn align(&mut self, _: &Frame<'_>, _: &Detection) -> Result<AlignedFace, FaceError> {
        Err(FaceError::Unsupported)
    }
    fn embed(&mut self, _: &AlignedFace) -> Result<Embedding, FaceError> {
        Err(FaceError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(values: Vec<f32>) -> Embedding {
        Embedding::new([1; 32], values).unwrap()
    }
    fn quality() -> QualityMetrics {
        QualityMetrics {
            mean_intensity: 0.5,
            contrast: 0.1,
            edge_energy: 0.1,
        }
    }
    fn template() -> Template {
        let mut enrollment = Enrollment::new(0.8).unwrap();
        for index in 0..3 {
            enrollment
                .add(
                    [index; 32],
                    sample(vec![1.0, f32::from(index) * 0.05]),
                    1,
                    quality(),
                )
                .unwrap();
        }
        enrollment.finish().unwrap()
    }

    #[test]
    fn normalization_dimension_and_nan_guards() {
        for values in [
            vec![],
            vec![0.0, 0.0],
            vec![f32::NAN],
            vec![f32::INFINITY],
            vec![1.0; MAX_EMBEDDING_DIMENSIONS + 1],
        ] {
            assert!(matches!(
                Embedding::new([1; 32], values),
                Err(FaceError::InvalidEmbedding)
            ));
        }
        assert!(
            sample(vec![1.0])
                .similarity(&sample(vec![1.0, 0.0]))
                .is_err()
        );
        assert!(
            sample(vec![1.0])
                .similarity(&Embedding::new([2; 32], vec![1.0]).unwrap())
                .is_err()
        );
        assert!(
            (sample(vec![3.0, 4.0])
                .similarity(&sample(vec![6.0, 8.0]))
                .unwrap()
                - 1.0)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn enrollment_and_roundtrip_matching_work() {
        let template = template();
        assert_eq!(template.sample_count(), 3);
        assert!(
            template
                .compare(&sample(vec![1.0, 0.05]), 0.9)
                .unwrap()
                .accepted
        );
        assert!(
            !template
                .compare(&sample(vec![-1.0, 0.0]), 0.9)
                .unwrap()
                .accepted
        );
        let saved = template.to_bytes().unwrap();
        let restored = Template::from_bytes(&saved).unwrap();
        assert!(
            restored
                .compare(&sample(vec![1.0, 0.05]), 0.9)
                .unwrap()
                .accepted
        );
        for threshold in [f32::NAN, f32::NEG_INFINITY, -0.1, 1.1] {
            assert!(
                restored
                    .compare(&sample(vec![1.0, 0.05]), threshold)
                    .is_err()
            );
        }
    }

    #[test]
    fn multi_sample_quality_and_consistency_are_enforced() {
        assert!(Enrollment::new(0.8).unwrap().finish().is_err());
        let mut enrollment = Enrollment::new(0.8).unwrap();
        enrollment
            .add([1; 32], sample(vec![1.0, 0.0]), 1, quality())
            .unwrap();
        assert_eq!(
            enrollment.add([1; 32], sample(vec![1.0, 0.0]), 1, quality()),
            Err(FaceError::DuplicateSample)
        );
        assert_eq!(
            enrollment.add([2; 32], sample(vec![-1.0, 0.0]), 1, quality()),
            Err(FaceError::InconsistentSample)
        );
        assert_eq!(
            enrollment.add([2; 32], sample(vec![1.0, 0.0]), 2, quality()),
            Err(FaceError::FaceCount)
        );
        assert_eq!(
            enrollment.add(
                [2; 32],
                sample(vec![1.0, 0.0]),
                1,
                QualityMetrics {
                    contrast: f32::NAN,
                    ..quality()
                }
            ),
            Err(FaceError::PoorQuality)
        );
    }

    #[test]
    fn frame_quality_is_statistics_not_liveness() {
        let bytes = vec![127; 32 * 32];
        let frame = Frame {
            width: 32,
            height: 32,
            stride: 32,
            format: PixelFormat::Gray8,
            pixels: &bytes,
        };
        assert!(!QualityMetrics::measure(&frame).unwrap().acceptable());
        assert!(
            Frame {
                stride: usize::MAX,
                ..frame
            }
            .validate()
            .is_err()
        );
        let mut engine = UnconfiguredEngine;
        assert!(matches!(engine.detect(&frame), Err(FaceError::Unsupported)));
    }

    #[test]
    fn bounded_template_reader_rejects_invalid_metadata() {
        let mut bytes = template().to_bytes().unwrap();
        let text = std::str::from_utf8(&bytes)
            .unwrap()
            .replace("\"version\":1", "\"version\":2");
        assert!(Template::from_bytes(text.as_bytes()).is_err());
        bytes.resize(MAX_TEMPLATE_BYTES + 1, b' ');
        assert!(Template::from_bytes(&bytes).is_err());
    }
}
