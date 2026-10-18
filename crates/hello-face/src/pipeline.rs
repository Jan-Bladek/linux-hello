//! Validation shared by diagnostic inference adapters. No model or PAD is provided.

use crate::{
    AlignedFace, Detection, Embedding, FaceEngine, FaceError, Frame, MAX_EMBEDDING_DIMENSIONS,
    QualityMetrics, validate_threshold,
};

/// Fixed recognizer input, declared by a separately configured adapter.
/// These limits are allocation guards, not a model's normalization contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputShape {
    pub width: u16,
    pub height: u16,
    pub channels: u8,
}

impl InputShape {
    fn elements(self) -> Result<usize, FaceError> {
        if !(16..=1024).contains(&self.width)
            || !(16..=1024).contains(&self.height)
            || !matches!(self.channels, 1 | 3)
        {
            return Err(FaceError::InvalidModel);
        }
        Ok(usize::from(self.width) * usize::from(self.height) * usize::from(self.channels))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    /// Authenticated embedding namespace; the OpenCV adapter binds its complete
    /// versioned pipeline, artifacts and runtime. Zero is unconfigured.
    pub digest: [u8; 32],
    pub input: InputShape,
    pub embedding_dimensions: usize,
}

impl ModelSpec {
    pub fn validate(&self) -> Result<(), FaceError> {
        self.input.elements()?;
        if self.digest == [0; 32]
            || !(1..=MAX_EMBEDDING_DIMENSIONS).contains(&self.embedding_dimensions)
        {
            return Err(FaceError::InvalidModel);
        }
        Ok(())
    }
}

impl Detection {
    pub fn validate(&self, frame: &Frame<'_>) -> Result<(), FaceError> {
        frame.validate()?;
        let [x, y, width, height] = self.bounds;
        if self.bounds.iter().any(|value| !value.is_finite())
            || !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || x < 0.0
            || y < 0.0
            || width <= 0.0
            || height <= 0.0
            || x + width > frame.width as f32
            || y + height > frame.height as f32
            || self.landmarks.iter().any(|point| {
                !point[0].is_finite()
                    || !point[1].is_finite()
                    || point[0] < 0.0
                    || point[1] < 0.0
                    || point[0] >= frame.width as f32
                    || point[1] >= frame.height as f32
            })
        {
            return Err(FaceError::InvalidDetection);
        }
        Ok(())
    }
}

impl AlignedFace {
    /// Validate against the adapter's fixed shape before passing data to inference.
    pub fn validate(&self, expected: InputShape) -> Result<(), FaceError> {
        let elements = expected.elements()?;
        if self.width != expected.width
            || self.height != expected.height
            || self.channels != expected.channels
            || self.values.len() != elements
            || self.values.iter().any(|value| !value.is_finite())
        {
            return Err(FaceError::InvalidAlignedFace);
        }
        Ok(())
    }
}

/// Extract a local diagnostic embedding through a validated adapter boundary.
/// A successful result proves neither identity, trusted acquisition nor liveness.
/// Color order, landmark ordering and normalization remain the adapter's contract.
pub fn extract_embedding<E: FaceEngine + ?Sized>(
    engine: &mut E,
    frame: &Frame<'_>,
    minimum_detection_confidence: f32,
) -> Result<Embedding, FaceError> {
    validate_threshold(minimum_detection_confidence)?;
    frame.validate()?;
    let model = engine.model_spec()?;
    model.validate()?;
    if !QualityMetrics::measure(frame)?.has_signal()
        || !crate::sampling::LightHistogram::measure(frame)?.has_signal()
    {
        return Err(FaceError::PoorQuality);
    }
    let detections = engine.detect(frame)?;
    let [detection] = detections.as_slice() else {
        return Err(FaceError::FaceCount);
    };
    detection.validate(frame)?;
    if detection.confidence < minimum_detection_confidence {
        return Err(FaceError::PoorQuality);
    }
    let aligned = engine.align(frame, detection)?;
    aligned.validate(model.input)?;
    let embedding = engine.embed(&aligned)?;
    if embedding.model_digest() != model.digest
        || embedding.dimensions() != model.embedding_dimensions
    {
        return Err(FaceError::IncompatibleEmbedding);
    }
    Ok(embedding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PixelFormat, UnconfiguredEngine};
    use zeroize::Zeroizing;

    struct TestEngine {
        model: ModelSpec,
        detections: Vec<Detection>,
        aligned: Option<AlignedFace>,
        embedding_digest: [u8; 32],
        embedding_values: Vec<f32>,
        calls: Vec<&'static str>,
    }

    impl TestEngine {
        fn valid() -> Self {
            Self {
                model: ModelSpec {
                    digest: [1; 32],
                    input: InputShape {
                        width: 16,
                        height: 16,
                        channels: 3,
                    },
                    embedding_dimensions: 2,
                },
                detections: vec![Detection {
                    bounds: [0.0, 0.0, 32.0, 32.0],
                    landmarks: [
                        [10.0, 10.0],
                        [22.0, 10.0],
                        [16.0, 16.0],
                        [11.0, 22.0],
                        [21.0, 22.0],
                    ],
                    confidence: 0.9,
                }],
                aligned: Some(AlignedFace {
                    width: 16,
                    height: 16,
                    channels: 3,
                    values: Zeroizing::new(vec![0.5; 16 * 16 * 3]),
                }),
                embedding_digest: [1; 32],
                embedding_values: vec![1.0, 0.2],
                calls: vec![],
            }
        }
    }

    impl FaceEngine for TestEngine {
        fn model_spec(&self) -> Result<ModelSpec, FaceError> {
            Ok(self.model)
        }
        fn detect(&mut self, _: &Frame<'_>) -> Result<Vec<Detection>, FaceError> {
            self.calls.push("detect");
            Ok(std::mem::take(&mut self.detections))
        }
        fn align(&mut self, _: &Frame<'_>, _: &Detection) -> Result<AlignedFace, FaceError> {
            self.calls.push("align");
            Ok(self.aligned.take().unwrap())
        }
        fn embed(&mut self, _: &AlignedFace) -> Result<Embedding, FaceError> {
            self.calls.push("embed");
            Embedding::new(
                self.embedding_digest,
                std::mem::take(&mut self.embedding_values),
            )
        }
    }

    fn pixels() -> Vec<u8> {
        (0..32 * 32)
            .map(|index| {
                if (index / 32 + index % 32) % 2 == 0 {
                    64
                } else {
                    192
                }
            })
            .collect()
    }

    fn frame(pixels: &[u8]) -> Frame<'_> {
        Frame {
            width: 32,
            height: 32,
            stride: 32,
            format: PixelFormat::Gray8,
            pixels,
        }
    }

    fn rejects(mutate: impl FnOnce(&mut TestEngine), error: FaceError, calls: &[&str]) {
        let pixels = pixels();
        let mut engine = TestEngine::valid();
        mutate(&mut engine);
        assert_eq!(
            extract_embedding(&mut engine, &frame(&pixels), 0.8).err(),
            Some(error)
        );
        assert_eq!(engine.calls, calls);
    }

    #[test]
    fn validated_extraction_is_bound_to_the_declared_model_and_dimension() {
        let pixels = pixels();
        let mut engine = TestEngine::valid();
        let embedding = extract_embedding(&mut engine, &frame(&pixels), 0.8).unwrap();
        assert_eq!(embedding.model_digest(), engine.model.digest);
        assert_eq!(embedding.dimensions(), engine.model.embedding_dimensions);
        assert_eq!(engine.calls, ["detect", "align", "embed"]);
        assert_eq!(engine.model_digest().unwrap(), engine.model.digest);
        assert!(matches!(
            extract_embedding(&mut UnconfiguredEngine, &frame(&pixels), 0.8),
            Err(FaceError::Unsupported)
        ));
    }

    #[test]
    fn invalid_model_and_policy_are_rejected_before_detection() {
        rejects(
            |engine| engine.model.digest = [0; 32],
            FaceError::InvalidModel,
            &[],
        );
        rejects(
            |engine| engine.model.input.width = 0,
            FaceError::InvalidModel,
            &[],
        );
        rejects(
            |engine| engine.model.input.height = 1025,
            FaceError::InvalidModel,
            &[],
        );
        rejects(
            |engine| engine.model.input.channels = 4,
            FaceError::InvalidModel,
            &[],
        );
        for dimensions in [0, MAX_EMBEDDING_DIMENSIONS + 1] {
            rejects(
                |engine| engine.model.embedding_dimensions = dimensions,
                FaceError::InvalidModel,
                &[],
            );
        }
        let pixels = pixels();
        for threshold in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut engine = TestEngine::valid();
            assert!(matches!(
                extract_embedding(&mut engine, &frame(&pixels), threshold),
                Err(FaceError::InvalidThreshold)
            ));
            assert!(engine.calls.is_empty());
        }
    }

    #[test]
    fn bad_frames_and_quality_never_reach_detection() {
        for pixels in [vec![127; 32 * 32], vec![0; 32 * 32 - 1]] {
            let mut engine = TestEngine::valid();
            assert!(extract_embedding(&mut engine, &frame(&pixels), 0.8).is_err());
            assert!(engine.calls.is_empty());
        }
    }

    #[test]
    fn invalid_detections_never_reach_alignment() {
        rejects(
            |engine| engine.detections.clear(),
            FaceError::FaceCount,
            &["detect"],
        );
        rejects(
            |engine| engine.detections.push(engine.detections[0]),
            FaceError::FaceCount,
            &["detect"],
        );
        for invalid in [f32::NAN, f32::INFINITY, -1.0, 33.0] {
            rejects(
                |engine| engine.detections[0].bounds[0] = invalid,
                FaceError::InvalidDetection,
                &["detect"],
            );
            rejects(
                |engine| engine.detections[0].landmarks[0][0] = invalid,
                FaceError::InvalidDetection,
                &["detect"],
            );
            rejects(
                |engine| engine.detections[0].confidence = invalid,
                FaceError::InvalidDetection,
                &["detect"],
            );
        }
        rejects(
            |engine| engine.detections[0].bounds[2] = 0.0,
            FaceError::InvalidDetection,
            &["detect"],
        );
        rejects(
            |engine| engine.detections[0].confidence = 0.7,
            FaceError::PoorQuality,
            &["detect"],
        );
    }

    #[test]
    fn malformed_aligned_input_never_reaches_inference() {
        rejects(
            |engine| engine.aligned.as_mut().unwrap().width = 17,
            FaceError::InvalidAlignedFace,
            &["detect", "align"],
        );
        rejects(
            |engine| {
                engine.aligned.as_mut().unwrap().values.pop();
            },
            FaceError::InvalidAlignedFace,
            &["detect", "align"],
        );
        for invalid in [f32::NAN, f32::INFINITY] {
            rejects(
                |engine| engine.aligned.as_mut().unwrap().values[0] = invalid,
                FaceError::InvalidAlignedFace,
                &["detect", "align"],
            );
        }
    }

    #[test]
    fn wrong_model_or_output_dimension_cannot_produce_an_embedding() {
        rejects(
            |engine| engine.embedding_digest = [2; 32],
            FaceError::IncompatibleEmbedding,
            &["detect", "align", "embed"],
        );
        rejects(
            |engine| engine.embedding_values.push(0.1),
            FaceError::IncompatibleEmbedding,
            &["detect", "align", "embed"],
        );
    }
}
