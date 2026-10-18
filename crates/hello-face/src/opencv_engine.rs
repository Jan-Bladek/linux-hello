//! Optional CPU-only, local evaluation adapter. No acquisition, PAD or login verdict.
//! Only the pinned YuNet/SFace artifacts are accepted, from the bytes actually loaded.

use crate::{
    AlignedFace, Detection, Embedding, FaceEngine, FaceError, Frame, InputShape, ModelSpec,
    PixelFormat,
};
use opencv::{core, dnn, imgproc, objdetect, prelude::*};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path};
use zeroize::{Zeroize, Zeroizing};

// Owns only matrices allocated by this adapter. DNN internals and allocations
// inside OpenCV remain outside this best-effort erasure boundary.
struct SecretMat(core::Mat);
impl std::ops::Deref for SecretMat {
    type Target = core::Mat;
    fn deref(&self) -> &core::Mat {
        &self.0
    }
}
impl SecretMat {
    fn empty() -> Self {
        Self(core::Mat::default())
    }
    fn wipe(&mut self) {
        if let Ok(bytes) = self.0.data_bytes_mut() {
            bytes.zeroize();
        }
    }
}
impl Drop for SecretMat {
    fn drop(&mut self) {
        self.wipe();
    }
}

pub const DETECTOR_FILE: &str = "face_detection_yunet_2023mar.onnx";
pub const RECOGNIZER_FILE: &str = "face_recognition_sface_2021dec.onnx";
pub const DETECTOR_SHA256: &str =
    "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
pub const RECOGNIZER_SHA256: &str =
    "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79";
const INPUT: InputShape = InputShape {
    width: 112,
    height: 112,
    channels: 3,
};

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verified_bytes(path: &Path, size: u64, digest: &str) -> Result<Vec<u8>, FaceError> {
    // O_NONBLOCK prevents a supplied FIFO from hanging before the regular-file check.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| FaceError::ModelArtifact)?;
    let metadata = file.metadata().map_err(|_| FaceError::ModelArtifact)?;
    if !metadata.is_file() || metadata.len() != size {
        return Err(FaceError::ModelArtifact);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FaceError::ModelArtifact)?;
    if bytes.len() as u64 != size || hex_digest(&bytes) != digest {
        return Err(FaceError::ModelArtifact);
    }
    Ok(bytes)
}

fn cv<T>(result: opencv::Result<T>) -> Result<T, FaceError> {
    result.map_err(|_| FaceError::Inference)
}

/// YuNet landmarks: right eye, left eye, nose, right mouth, left mouth.
/// AlignedFace contains interleaved BGR integer samples in 0..255. SFace's
/// feature() builds RGB NCHW float input; normalization is internal to the model.
pub struct OpenCvEngine {
    detector: core::Ptr<objdetect::FaceDetectorYN>,
    recognizer: core::Ptr<objdetect::FaceRecognizerSF>,
    digest: [u8; 32],
}

/// Only byte-length/hash-verified artifacts; this step does not invoke OpenCV.
pub struct PinnedModels {
    detector: Vec<u8>,
    recognizer: Vec<u8>,
}

impl PinnedModels {
    pub fn read(directory: &Path) -> Result<Self, FaceError> {
        Ok(Self {
            detector: verified_bytes(&directory.join(DETECTOR_FILE), 232589, DETECTOR_SHA256)?,
            recognizer: verified_bytes(
                &directory.join(RECOGNIZER_FILE),
                38696353,
                RECOGNIZER_SHA256,
            )?,
        })
    }
}

impl OpenCvEngine {
    /// Explicit local directory only. No search paths, downloads or custom operators.
    pub fn load(directory: &Path) -> Result<Self, FaceError> {
        Self::from_pinned_models(PinnedModels::read(directory)?)
    }

    /// Native parsing starts here; the CLI calls it only inside its restricted worker.
    pub fn from_pinned_models(models: PinnedModels) -> Result<Self, FaceError> {
        if core::get_version_major() != 4 || core::get_version_minor() < 11 {
            return Err(FaceError::Unsupported);
        }
        let PinnedModels {
            detector,
            recognizer,
        } = models;
        let digest = crate::pipeline_identity::digest()?;
        // Process-wide OpenCV setting. This adapter belongs in a diagnostic worker.
        cv(core::set_num_threads(1))?;
        let detector = cv(objdetect::FaceDetectorYN::create_1(
            "onnx",
            &core::Vector::from_slice(&detector),
            &core::Vector::new(),
            core::Size::new(320, 320),
            0.75,
            0.3,
            500,
            dnn::DNN_BACKEND_OPENCV,
            dnn::DNN_TARGET_CPU,
        ))?;
        let recognizer = cv(objdetect::FaceRecognizerSF::create_1(
            "onnx",
            &core::Vector::from_slice(&recognizer),
            &core::Vector::new(),
            dnn::DNN_BACKEND_OPENCV,
            dnn::DNN_TARGET_CPU,
        ))?;
        Ok(Self {
            detector,
            recognizer,
            digest,
        })
    }
}

fn bgr(frame: &Frame<'_>) -> Result<SecretMat, FaceError> {
    frame.validate()?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(frame.width * frame.height * 3));
    for row in frame.pixels.chunks_exact(frame.stride) {
        match frame.format {
            PixelFormat::Gray8 => {
                for value in &row[..frame.width] {
                    bytes.extend_from_slice(&[*value; 3]);
                }
            }
            PixelFormat::Rgb8 => {
                for pixel in row[..frame.width * 3].as_chunks::<3>().0 {
                    bytes.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                }
            }
        }
    }
    let flat = cv(core::Mat::from_slice(&bytes))?;
    let shaped = cv(flat.reshape(3, frame.height as i32))?;
    cv(shaped.try_clone()).map(SecretMat)
}

fn monochrome(frame: &Frame<'_>) -> bool {
    match frame.format {
        PixelFormat::Gray8 => true,
        PixelFormat::Rgb8 => frame.pixels.chunks_exact(frame.stride).all(|row| {
            row[..frame.width * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2])
        }),
    }
}

impl FaceEngine for OpenCvEngine {
    fn model_spec(&self) -> Result<ModelSpec, FaceError> {
        Ok(ModelSpec {
            digest: self.digest,
            input: INPUT,
            embedding_dimensions: 128,
        })
    }

    fn detect(&mut self, frame: &Frame<'_>) -> Result<Vec<Detection>, FaceError> {
        let image = bgr(frame)?;
        cv(self
            .detector
            .set_input_size(core::Size::new(frame.width as i32, frame.height as i32)))?;
        let mut faces = SecretMat::empty();
        cv(self.detector.detect(&*image, &mut faces.0))?;
        let needs_enhancement = faces.empty()
            || (faces.rows() == 1
                && cv(faces.data_typed::<f32>())?[14] < crate::sampling::MIN_DETECTION_CONFIDENCE);
        if needs_enhancement && monochrome(frame) {
            // Howdy-style CLAHE on detector input only. SFace alignment/features
            // still use original pixels, preserving stored embedding semantics.
            let mut gray = SecretMat::empty();
            cv(imgproc::cvt_color(
                &*image,
                &mut gray.0,
                imgproc::COLOR_BGR2GRAY,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            ))?;
            let mut enhanced = SecretMat::empty();
            let mut clahe = cv(imgproc::create_clahe(2.0, core::Size::new(8, 8)))?;
            cv(clahe.apply(&*gray, &mut enhanced.0))?;
            let mut detector_image = SecretMat::empty();
            cv(imgproc::cvt_color(
                &*enhanced,
                &mut detector_image.0,
                imgproc::COLOR_GRAY2BGR,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            ))?;
            let mut enhanced_faces = SecretMat::empty();
            cv(self
                .detector
                .detect(&*detector_image, &mut enhanced_faces.0))?;
            // Do not turn a multi-face result into a single-face acceptance.
            if !enhanced_faces.empty()
                && (faces.empty()
                    || enhanced_faces.rows() > 1
                    || cv(enhanced_faces.data_typed::<f32>())?[14]
                        > cv(faces.data_typed::<f32>())?[14])
            {
                faces = enhanced_faces;
            }
        }
        if faces.empty() {
            return Ok(Vec::new());
        }
        if faces.cols() != 15 || faces.rows() > 500 || faces.typ() != core::CV_32F {
            return Err(FaceError::Inference);
        }
        let mut detections = Vec::with_capacity(faces.rows() as usize);
        for row in 0..faces.rows() {
            let values = cv(faces.row(row))?;
            let values = cv(values.data_typed::<f32>())?;
            let mut detection = Detection {
                bounds: values[..4].try_into().map_err(|_| FaceError::Inference)?,
                landmarks: std::array::from_fn(|i| [values[4 + i * 2], values[5 + i * 2]]),
                confidence: values[14],
            };
            let [x, y, w, h] = detection.bounds;
            if detection.bounds.iter().any(|v| !v.is_finite()) || w <= 0.0 || h <= 0.0 {
                return Err(FaceError::InvalidDetection);
            }
            let left = x.max(0.0);
            let top = y.max(0.0);
            detection.bounds = [
                left,
                top,
                (x + w).min(frame.width as f32) - left,
                (y + h).min(frame.height as f32) - top,
            ];
            detection.validate(frame)?;
            detections.push(detection);
        }
        Ok(detections)
    }

    fn align(
        &mut self,
        frame: &Frame<'_>,
        detection: &Detection,
    ) -> Result<AlignedFace, FaceError> {
        detection.validate(frame)?;
        let image = bgr(frame)?;
        let mut row = [0.0_f32; 15];
        row[..4].copy_from_slice(&detection.bounds);
        for (i, point) in detection.landmarks.iter().enumerate() {
            row[4 + i * 2..6 + i * 2].copy_from_slice(point);
        }
        row[14] = detection.confidence;
        let flat = cv(core::Mat::from_slice(&row))?;
        let face = cv(flat.reshape(1, 1))?;
        let mut aligned = SecretMat::empty();
        cv(self.recognizer.align_crop(&*image, &face, &mut aligned.0))?;
        if aligned.rows() != 112 || aligned.cols() != 112 || aligned.typ() != core::CV_8UC3 {
            return Err(FaceError::InvalidAlignedFace);
        }
        let values = cv(aligned.data_bytes())?
            .iter()
            .map(|v| f32::from(*v))
            .collect();
        Ok(AlignedFace {
            width: 112,
            height: 112,
            channels: 3,
            values: Zeroizing::new(values),
        })
    }

    fn embed(&mut self, aligned: &AlignedFace) -> Result<Embedding, FaceError> {
        aligned.validate(INPUT)?;
        if aligned
            .values
            .iter()
            .any(|v| !(0.0..=255.0).contains(v) || v.fract() != 0.0)
        {
            return Err(FaceError::InvalidAlignedFace);
        }
        let bytes = Zeroizing::new(aligned.values.iter().map(|v| *v as u8).collect::<Vec<_>>());
        let flat = cv(core::Mat::from_slice(&bytes))?;
        let image = cv(flat.reshape(3, 112))?;
        let mut feature = SecretMat::empty();
        cv(self.recognizer.feature(&image, &mut feature.0))?;
        if feature.typ() != core::CV_32F || feature.total() != 128 {
            return Err(FaceError::InvalidEmbedding);
        }
        Embedding::new(self.digest, cv(feature.data_typed::<f32>())?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn artifact_reader_rejects_tampering_symlinks_and_nonregular_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model");
        fs::write(&path, b"test").unwrap();
        let digest = hex_digest(b"test");
        assert_eq!(verified_bytes(&path, 4, &digest).unwrap(), b"test");
        assert!(verified_bytes(&path, 3, &digest).is_err());
        assert!(verified_bytes(&path, 4, DETECTOR_SHA256).is_err());
        let link = directory.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(verified_bytes(&link, 4, &digest).is_err());
        assert!(verified_bytes(directory.path(), 4, &digest).is_err());
        assert!(verified_bytes(&directory.path().join("missing"), 4, &digest).is_err());
    }

    #[test]
    fn controlled_native_pixel_and_feature_buffers_are_erased() {
        let pixels = vec![123u8; 16 * 16];
        let mut image = bgr(&Frame {
            width: 16,
            height: 16,
            stride: 16,
            format: PixelFormat::Gray8,
            pixels: &pixels,
        })
        .unwrap();
        assert!(image.data_bytes().unwrap().iter().any(|b| *b != 0));
        image.wipe();
        assert!(image.data_bytes().unwrap().iter().all(|b| *b == 0));
        let flat = core::Mat::from_slice(&[0.5f32; 128]).unwrap();
        let mut feature = SecretMat(flat.try_clone().unwrap());
        feature.wipe();
        assert!(
            feature
                .data_typed::<f32>()
                .unwrap()
                .iter()
                .all(|v| *v == 0.0)
        );
    }

    #[test]
    fn color_conversion_respects_padding_and_gray() {
        let pixels = [1, 2, 3, 9].repeat(16 * 16);
        let frame = Frame {
            width: 16,
            height: 16,
            stride: 64,
            format: PixelFormat::Rgb8,
            pixels: &pixels,
        };
        let image = bgr(&frame).unwrap();
        assert_eq!(&image.data_bytes().unwrap()[..6], &[3, 2, 1, 2, 1, 9]);
        let pixels = vec![42; 16 * 16];
        let image = bgr(&Frame {
            width: 16,
            height: 16,
            stride: 16,
            format: PixelFormat::Gray8,
            pixels: &pixels,
        })
        .unwrap();
        assert!(image.data_bytes().unwrap().iter().all(|v| *v == 42));
    }
}
