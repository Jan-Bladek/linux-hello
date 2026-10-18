//! Authenticated embedding namespace for the complete supported inference contract.
//! A matching ID proves compatibility, never NIR accuracy, PAD or provenance.
use crate::{
    FaceError,
    opencv_engine::{DETECTOR_SHA256, RECOGNIZER_SHA256},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

#[derive(Clone, Serialize)]
pub struct PipelineIdentity {
    pub version: u32,
    pub detector_sha256: String,
    pub recognizer_sha256: String,
    pub preprocessing: String,
    pub alignment: String,
    pub quality: String,
    pub scoring: String,
    pub acquisition: String,
    pub input: [usize; 3],
    pub dimensions: usize,
    pub runtime_version: String,
    pub runtime_build_sha256: [u8; 32],
    pub runtime_cpu_sha256: [u8; 32],
    pub rust_build_sha256: [u8; 32],
    pub implementation_sha256: [u8; 32],
    pub target: String,
}
impl PipelineIdentity {
    pub fn digest(&self) -> Result<[u8; 32], FaceError> {
        let mut hash = Sha256::new();
        hash.update(b"linux-hello-full-pipeline-v1\0");
        hash.update(serde_json::to_vec(self).map_err(|_| FaceError::InvalidModel)?);
        Ok(hash.finalize().into())
    }
}

pub fn current() -> Result<&'static PipelineIdentity, FaceError> {
    static CURRENT: OnceLock<Result<PipelineIdentity, FaceError>> = OnceLock::new();
    CURRENT.get_or_init(|| {
        let mut implementation = Sha256::new();
        // Length framing preserves component boundaries. Source binding is
        // conservative: even nonfunctional edits can require re-enrollment.
        for source in [include_str!("pipeline_identity.rs"),
                include_str!("../build.rs"),
                include_str!("../Cargo.toml"), include_str!("opencv_engine.rs"),
            include_str!("pipeline.rs"), include_str!("sampling.rs"), include_str!("geometry.rs"),
            include_str!("verification.rs"), include_str!("lib.rs")] {
            implementation.update((source.len() as u64).to_le_bytes());
            implementation.update(source.as_bytes());
        }
        Ok(PipelineIdentity {
            version: 1,
            detector_sha256: DETECTOR_SHA256.into(), recognizer_sha256: RECOGNIZER_SHA256.into(),
            preprocessing: "gray8-replicated-or-rgb8-to-bgr8;detector-only-clahe-2.0-8x8;sface-original-bgr8-to-rgb-nchw-raw-v1".into(),
            alignment: "opencv-sface-align-crop-5-landmarks-112x112-v1".into(),
            quality: "sampling-source-bound-v1".into(),
            scoring: "normalized-cosine-pose-median3-and-centroid-geometry-v2;uncalibrated".into(),
            acquisition: "camera-ir-bound;passive-front-three-fresh-frames-15s-16-captures-v1".into(),
            input: [112,112,3], dimensions:128,
            runtime_version: opencv::core::get_version_string().map_err(|_| FaceError::Inference)?,
            runtime_build_sha256: Sha256::digest(opencv::core::get_build_information().map_err(|_| FaceError::Inference)?.as_bytes()).into(),
            runtime_cpu_sha256: Sha256::digest(opencv::core::get_cpu_features_line().map_err(|_| FaceError::Inference)?.as_bytes()).into(),
            rust_build_sha256: Sha256::digest(env!("HELLO_FACE_BUILD").as_bytes()).into(),
            implementation_sha256: implementation.finalize().into(),
            target: format!("{}-{}-opencv-rust-source-bound", std::env::consts::ARCH, std::env::consts::OS),
        })
    }).as_ref().map_err(Clone::clone)
}
pub fn digest() -> Result<[u8; 32], FaceError> {
    current()?.digest()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_pipeline_component_changes_the_authenticated_namespace() {
        let base = current().unwrap();
        let expected = base.digest().unwrap();
        let bytes = serde_json::to_value(base).unwrap();
        for key in bytes.as_object().unwrap().keys() {
            let mut value = base.clone();
            match key.as_str() {
                "version" => value.version += 1,
                "detector_sha256" => value.detector_sha256.push('x'),
                "recognizer_sha256" => value.recognizer_sha256.push('x'),
                "preprocessing" => value.preprocessing.push('x'),
                "alignment" => value.alignment.push('x'),
                "quality" => value.quality.push('x'),
                "scoring" => value.scoring.push('x'),
                "acquisition" => value.acquisition.push('x'),
                "input" => value.input[0] += 1,
                "dimensions" => value.dimensions += 1,
                "runtime_version" => value.runtime_version.push('x'),
                "runtime_build_sha256" => value.runtime_build_sha256[0] ^= 1,
                "runtime_cpu_sha256" => value.runtime_cpu_sha256[0] ^= 1,
                "rust_build_sha256" => value.rust_build_sha256[0] ^= 1,
                "implementation_sha256" => value.implementation_sha256[0] ^= 1,
                "target" => value.target.push('x'),
                _ => panic!("new field needs a compatibility test"),
            }
            assert_ne!(value.digest().unwrap(), expected, "{key}");
            let original = crate::Embedding::new(expected, vec![1.0; 128]).unwrap();
            let changed = crate::Embedding::new(value.digest().unwrap(), vec![1.0; 128]).unwrap();
            assert!(original.similarity(&changed).is_err(), "{key}");
        }
        assert_eq!(expected, digest().unwrap());
        assert_ne!(expected, Sha256::digest(b"recognizer-only").as_slice());
    }
}
