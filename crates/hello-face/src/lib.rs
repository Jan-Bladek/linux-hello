//! Model-independent biometric primitives. No inference model is bundled.
//! All thresholds below are diagnostic policy, not measured security claims.

use serde::{Deserialize, Serialize, ser::SerializeStruct};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub mod geometry;
mod pipeline;
pub mod sampling;
pub mod verification;
pub use pipeline::{InputShape, ModelSpec, extract_embedding};

#[cfg(all(feature = "opencv", target_os = "linux"))]
pub mod opencv_engine;

pub const MAX_EMBEDDING_DIMENSIONS: usize = 2048;
pub const MAX_TEMPLATE_BYTES: usize = 64 * 1024;
pub const MIN_ENROLLMENT_SAMPLES: usize = 3;
pub const MAX_ENROLLMENT_SAMPLES: usize = 12;

#[derive(Debug, Clone, PartialEq, Error, Serialize, Deserialize)]
pub enum FaceError {
    #[error("model file does not match the pinned artifact or cannot be read safely")]
    ModelArtifact,
    #[error("OpenCV diagnostic inference failed")]
    Inference,
    #[error("no licensed and evaluated inference adapter is configured")]
    Unsupported,
    #[error("frame dimensions, stride, or byte count are invalid")]
    InvalidFrame,
    #[error("detector returned invalid bounds, landmarks, or confidence")]
    InvalidDetection,
    #[error("aligned input has an invalid shape, length, or non-finite value")]
    InvalidAlignedFace,
    #[error("model specification has an invalid identity, input shape, or output dimension")]
    InvalidModel,
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
    #[error("look straight while capturing the first three reference frames")]
    EnrollmentPose,
    #[error("enrollment sample count is outside policy")]
    SampleCount,
    #[error("stored template is malformed or exceeds limits")]
    InvalidTemplate,
    #[error(
        "person name must contain 1–64 characters (maximum 256 bytes), without control characters"
    )]
    InvalidDisplayName,
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

    /// Whole-frame signal gate; a dark IR background is not poor face quality.
    pub fn has_signal(&self) -> bool {
        [self.mean_intensity, self.contrast, self.edge_energy]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.contrast >= 0.003
            && self.edge_energy >= 0.0005
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
#[derive(Clone)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exemplars: Vec<Embedding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    geometry: Vec<Option<geometry::FaceGeometry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_binding: Option<[u8; 32]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
}

/// A local display label, not a verified identity or login principal.
pub fn normalize_display_name(name: &str) -> Result<String, FaceError> {
    if name.chars().any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')) {
        return Err(FaceError::InvalidDisplayName);
    }
    let name = name.trim();
    if name.is_empty() || name.len() > 256 || name.chars().count() > 64 {
        return Err(FaceError::InvalidDisplayName);
    }
    Ok(name.to_owned())
}

impl Drop for Template {
    fn drop(&mut self) {
        self.display_name.zeroize();
    }
}

impl Template {
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn set_display_name(&mut self, name: &str) -> Result<(), FaceError> {
        let name = normalize_display_name(name)?;
        self.display_name.zeroize();
        self.display_name = Some(name);
        Ok(())
    }
    pub fn dimensions(&self) -> usize {
        self.centroid.dimensions()
    }

    pub fn capture_binding(&self) -> Option<[u8; 32]> {
        self.capture_binding
    }

    pub fn bind_capture(mut self, binding: [u8; 32]) -> Result<Self, FaceError> {
        if binding == [0; 32] {
            return Err(FaceError::InvalidTemplate);
        }
        self.capture_binding = Some(binding);
        Ok(self)
    }

    pub fn similarity(&self, probe: &Embedding) -> Result<f32, FaceError> {
        self.centroid.similarity(probe)
    }

    pub fn strict_available(&self) -> bool {
        self.exemplars.len() == self.sample_count
            && self.geometry.len() == self.sample_count
            && self.geometry.iter().all(Option::is_some)
    }
    /// Conservative identity gate: centroid AND median exemplar similarity,
    /// plus a nearest stored landmark shape. Thresholds are provisional.
    pub fn strict_similarity(
        &self,
        probe: &Embedding,
        geometry: &geometry::FaceGeometry,
    ) -> Result<f32, FaceError> {
        Ok(self.verification_evidence(probe, geometry)?.score)
    }
    pub fn version(&self) -> u8 {
        self.version
    }

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
            #[serde(default)]
            exemplars: Vec<Embedding>,
            #[serde(default)]
            geometry: Vec<Option<geometry::FaceGeometry>>,
            #[serde(default)]
            capture_binding: Option<[u8; 32]>,
            #[serde(default)]
            display_name: Option<String>,
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| FaceError::InvalidTemplate)?;
        if wire.capture_binding == Some([0; 32])
            || !matches!(wire.version, 1 | 2)
            || !(MIN_ENROLLMENT_SAMPLES..=MAX_ENROLLMENT_SAMPLES).contains(&wire.sample_count)
        {
            return Err(FaceError::InvalidTemplate);
        }
        if (!wire.exemplars.is_empty() && wire.exemplars.len() != wire.sample_count)
            || (!wire.geometry.is_empty() && wire.geometry.len() != wire.sample_count)
            || wire.geometry.iter().flatten().any(|g| !g.valid())
            || wire.exemplars.iter().any(|e| {
                e.model_digest() != wire.centroid.model_digest()
                    || e.dimensions() != wire.centroid.dimensions()
            })
        {
            return Err(FaceError::InvalidTemplate);
        }
        if wire.version == 2
            && (wire.exemplars.len() != wire.sample_count
                || wire.geometry.len() != wire.sample_count
                || wire.geometry.iter().any(Option::is_none)
                || wire.centroid.model_digest() == [0; 32]
                || averaged_embedding(&wire.exemplars)?.similarity(&wire.centroid)? < 0.99999)
        {
            return Err(FaceError::InvalidTemplate);
        }
        let display_name = wire
            .display_name
            .as_deref()
            .map(normalize_display_name)
            .transpose()?;
        Ok(Self {
            version: wire.version,
            sample_count: wire.sample_count,
            centroid: wire.centroid,
            exemplars: wire.exemplars,
            geometry: wire.geometry,
            capture_binding: wire.capture_binding,
            display_name,
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
    geometry: Vec<Option<geometry::FaceGeometry>>,
    sample_ids: Vec<[u8; 32]>,
    minimum_consistency: f32,
    pose_aware: bool,
}

impl Enrollment {
    pub fn new(minimum_consistency: f32) -> Result<Self, FaceError> {
        validate_threshold(minimum_consistency)?;
        Ok(Self {
            samples: Vec::new(),
            geometry: Vec::new(),
            sample_ids: Vec::new(),
            minimum_consistency,
            pose_aware: false,
        })
    }

    /// Three frontal anchors precede pose variations; rejected samples never mutate the set.
    pub fn new_pose_aware(minimum_consistency: f32) -> Result<Self, FaceError> {
        let mut enrollment = Self::new(minimum_consistency)?;
        enrollment.pose_aware = true;
        Ok(enrollment)
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
        if self.pose_aware && self.samples.len() >= 3 {
            let anchors = &self.samples[..3];
            let support = anchors
                .iter()
                .map(|prior| prior.similarity(&embedding))
                .collect::<Result<Vec<_>, _>>()?;
            if averaged_embedding(anchors)?.similarity(&embedding)? < self.minimum_consistency
                || support
                    .iter()
                    .filter(|&&score| score >= self.minimum_consistency)
                    .count()
                    < 2
            {
                return Err(FaceError::InconsistentSample);
            }
        } else {
            for prior in &self.samples {
                if prior.similarity(&embedding)? < self.minimum_consistency {
                    return Err(FaceError::InconsistentSample);
                }
            }
        }
        self.sample_ids.push(sample_id);
        self.samples.push(embedding);
        self.geometry.push(None);
        Ok(())
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn add_with_geometry(
        &mut self,
        sample_id: [u8; 32],
        embedding: Embedding,
        faces: usize,
        quality: QualityMetrics,
        geometry: geometry::FaceGeometry,
    ) -> Result<(), FaceError> {
        if !geometry.valid() {
            return Err(FaceError::InvalidDetection);
        }
        if self.pose_aware
            && self.samples.len() < 3
            && let Some(Some(first)) = self.geometry.first()
            && (first.yaw() - geometry.yaw()).abs() > 0.05
        {
            return Err(FaceError::EnrollmentPose);
        }
        self.add(sample_id, embedding, faces, quality)?;
        *self.geometry.last_mut().expect("accepted sample") = Some(geometry);
        Ok(())
    }
    pub fn finish(self) -> Result<Template, FaceError> {
        if self.samples.len() < MIN_ENROLLMENT_SAMPLES {
            return Err(FaceError::SampleCount);
        }
        let centroid = averaged_embedding(&self.samples)?;
        let version = if self.pose_aware && self.geometry.iter().all(Option::is_some) {
            2
        } else {
            1
        };
        Ok(Template {
            version,
            sample_count: self.samples.len(),
            centroid,
            exemplars: self.samples,
            geometry: self.geometry,
            capture_binding: None,
            display_name: None,
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
    /// Top-left x/y followed by width/height, in source-frame pixels.
    pub bounds: [f32; 4],
    /// Five source-frame pixel coordinates; ordering is adapter-specific.
    pub landmarks: [[f32; 2]; 5],
    pub confidence: f32,
}

/// Aligned model input; normalization and color order are adapter-specific.
pub struct AlignedFace {
    pub width: u16,
    pub height: u16,
    pub channels: u8,
    pub values: Zeroizing<Vec<f32>>,
}

pub trait FaceEngine: Send {
    fn model_spec(&self) -> Result<ModelSpec, FaceError>;
    fn model_digest(&self) -> Result<[u8; 32], FaceError> {
        Ok(self.model_spec()?.digest)
    }
    fn detect(&mut self, frame: &Frame<'_>) -> Result<Vec<Detection>, FaceError>;
    fn align(&mut self, frame: &Frame<'_>, detection: &Detection)
    -> Result<AlignedFace, FaceError>;
    fn embed(&mut self, aligned: &AlignedFace) -> Result<Embedding, FaceError>;
}

/// Explicitly unavailable integration boundary, never a synthetic inference engine.
pub struct UnconfiguredEngine;

impl FaceEngine for UnconfiguredEngine {
    fn model_spec(&self) -> Result<ModelSpec, FaceError> {
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

fn averaged_embedding(samples: &[Embedding]) -> Result<Embedding, FaceError> {
    let first = samples.first().ok_or(FaceError::SampleCount)?;
    let mut sums = Zeroizing::new(vec![0_f64; first.dimensions()]);
    for sample in samples {
        if sample.model_digest != first.model_digest || sample.dimensions() != first.dimensions() {
            return Err(FaceError::IncompatibleEmbedding);
        }
        for (sum, value) in sums.iter_mut().zip(sample.values.iter()) {
            *sum += f64::from(*value);
        }
    }
    Embedding::new(first.model_digest, sums.iter().map(|v| *v as f32).collect())
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
    fn capture_binding_is_optional_for_legacy_templates_but_never_zero() {
        let mut enrollment = Enrollment::new(0.8).unwrap();
        for id in 1..=3 {
            enrollment
                .add(
                    [id; 32],
                    Embedding::new([1; 32], vec![1.0, 0.0]).unwrap(),
                    1,
                    QualityMetrics {
                        mean_intensity: 0.5,
                        contrast: 0.1,
                        edge_energy: 0.1,
                    },
                )
                .unwrap();
        }
        let template = enrollment.finish().unwrap();
        assert_eq!(
            Template::from_bytes(&template.to_bytes().unwrap())
                .unwrap()
                .capture_binding(),
            None
        );
        let template = template.bind_capture([8; 32]).unwrap();
        assert_eq!(
            Template::from_bytes(&template.to_bytes().unwrap())
                .unwrap()
                .capture_binding(),
            Some([8; 32])
        );
        assert!(template.bind_capture([0; 32]).is_err());
    }
    #[test]
    fn person_names_are_bounded_and_old_templates_remain_readable() {
        let mut template = template();
        let legacy = template.to_bytes().unwrap();
        assert_eq!(Template::from_bytes(&legacy).unwrap().display_name(), None);
        template.set_display_name("  Żaneta Kowalska  ").unwrap();
        assert_eq!(template.display_name(), Some("Żaneta Kowalska"));
        let restored = Template::from_bytes(&template.to_bytes().unwrap()).unwrap();
        assert_eq!(restored.display_name(), Some("Żaneta Kowalska"));
        for name in ["", " ", "Jan\n", "Jan\0", "Jan\u{202e}"] {
            assert!(template.set_display_name(name).is_err());
            assert_eq!(template.display_name(), Some("Żaneta Kowalska"));
        }
        assert!(normalize_display_name(&"a".repeat(65)).is_err());
        assert!(normalize_display_name(&"😀".repeat(64)).is_ok());
        let mut wire = serde_json::to_value(&template).unwrap();
        wire["display_name"] = "x".repeat(1000).into();
        assert!(Template::from_bytes(&serde_json::to_vec(&wire).unwrap()).is_err());
    }

    #[test]
    fn strict_template_roundtrip_keeps_exemplars_and_rejects_malformed_geometry() {
        let geometry = geometry::FaceGeometry::measure(&Detection {
            bounds: [0.0, 0.0, 200.0, 240.0],
            landmarks: [
                [50.0, 60.0],
                [150.0, 60.0],
                [100.0, 120.0],
                [65.0, 180.0],
                [135.0, 180.0],
            ],
            confidence: 0.99,
        })
        .unwrap();
        let mut enrollment = Enrollment::new(0.8).unwrap();
        for id in 0..8 {
            enrollment
                .add_with_geometry(
                    [id; 32],
                    sample(vec![1.0, 0.0]),
                    1,
                    quality(),
                    geometry.clone(),
                )
                .unwrap();
        }
        let template = enrollment.finish().unwrap();
        assert!(template.strict_available());
        let bytes = template.to_bytes().unwrap();
        let restored = Template::from_bytes(&bytes).unwrap();
        assert!(restored.strict_available());
        assert_eq!(
            restored
                .strict_similarity(&sample(vec![1.0, 0.0]), &geometry)
                .unwrap(),
            1.0
        );
        let mut json = serde_json::to_value(&restored).unwrap();
        json["geometry"][0]["ratios"][0] = (-1.0).into();
        assert!(Template::from_bytes(&serde_json::to_vec(&json).unwrap()).is_err());
        let mut json = serde_json::to_value(&restored).unwrap();
        json["exemplars"][0]["model_digest"][0] = 99.into();
        assert!(Template::from_bytes(&serde_json::to_vec(&json).unwrap()).is_err());
        let mut json = serde_json::to_value(&restored).unwrap();
        json.as_object_mut().unwrap().remove("exemplars");
        json.as_object_mut().unwrap().remove("geometry");
        assert!(
            !Template::from_bytes(&serde_json::to_vec(&json).unwrap())
                .unwrap()
                .strict_available()
        );
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
