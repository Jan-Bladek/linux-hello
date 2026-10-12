//! Explicit local dataset evaluation; identities and embeddings never enter reports.
use crate::face_worker::{Error, MAX_HEIGHT, MAX_WIDTH, Worker};
mod verification_evaluation;
use hello_face::Embedding;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u8,
    dataset_id: String,
    consent_record: String,
    license_record: String,
    sensor_mode: String,
    conditions: String,
    samples: Vec<Sample>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    id: String,
    identity: String,
    session: String,
    split: Split,
    path: PathBuf,
    width: u32,
    height: u32,
    #[serde(default)]
    role: Option<SampleRole>,
    #[serde(default)]
    claimed_identity: Option<String>,
    #[serde(default)]
    attempt: Option<String>,
    #[serde(default)]
    expected_faces: Option<usize>,
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SampleRole {
    Enrollment,
    Verification,
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Split {
    Calibration,
    Evaluation,
}

fn text_valid(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
impl Manifest {
    fn validate(&self) -> Result<(), Error> {
        if !matches!(self.version, 1 | 2)
            || !(8..=128).contains(&self.samples.len())
            || [
                &self.dataset_id,
                &self.consent_record,
                &self.license_record,
                &self.conditions,
            ]
            .iter()
            .any(|v| !text_valid(v))
            || !matches!(
                self.sensor_mode.as_str(),
                "rgb" | "monochrome_ir_unverified" | "verified_ir"
            )
        {
            return Err(
                "invalid evaluation manifest or missing dataset provenance/conditions".into(),
            );
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut calibration = BTreeSet::new();
        let mut evaluation = BTreeSet::new();
        let mut sessions = BTreeMap::<&str, BTreeSet<&str>>::new();
        for sample in &self.samples {
            if [&sample.id, &sample.identity, &sample.session]
                .iter()
                .any(|v| !text_valid(v))
                || !ids.insert(&sample.id)
                || !paths.insert(&sample.path)
                || sample.path.as_os_str().is_empty()
                || sample
                    .path
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                || !(16..=MAX_WIDTH).contains(&sample.width)
                || !(16..=MAX_HEIGHT).contains(&sample.height)
            {
                return Err("invalid or duplicate sample metadata/path/shape".into());
            }
            sessions
                .entry(&sample.identity)
                .or_default()
                .insert(&sample.session);
            match sample.split {
                Split::Calibration => {
                    calibration.insert(&sample.identity);
                }
                Split::Evaluation => {
                    evaluation.insert(&sample.identity);
                }
            }
        }
        if sessions.values().any(|sessions| sessions.len() < 2) {
            return Err("every identity requires at least two acquisition sessions".into());
        }
        if !calibration.is_disjoint(&evaluation) || calibration.len() < 2 || evaluation.len() < 2 {
            return Err(
                "calibration and evaluation require disjoint identities (at least two per split)"
                    .into(),
            );
        }
        if self.version == 2 {
            verification_evaluation::validate(self)?;
        } else if self
            .samples
            .iter()
            .any(|s| s.role.is_some() || s.claimed_identity.is_some() || s.attempt.is_some())
        {
            return Err("verification protocol fields require manifest version 2".into());
        }
        if self
            .samples
            .iter()
            .any(|s| s.expected_faces.is_some_and(|n| n > 20))
        {
            return Err("invalid expected face count".into());
        }
        Ok(())
    }
}

pub fn read_rgb(path: &Path, width: u32, height: u32) -> Result<Zeroizing<Vec<u8>>, Error> {
    if !(16..=MAX_WIDTH).contains(&width) || !(16..=MAX_HEIGHT).contains(&height) {
        return Err("invalid RGB shape".into());
    }
    let size = width as usize * height as usize * 3;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != size as u64 {
        return Err("RGB file must be a regular file with exact byte count".into());
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(size));
    file.take(size as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() != size {
        return Err("RGB file changed while reading".into());
    }
    Ok(bytes)
}

#[derive(Serialize)]
struct Rate {
    events: usize,
    trials: usize,
    rate: f64,
    wilson_95_descriptive: [f64; 2],
}
fn rate(events: usize, trials: usize) -> Option<Rate> {
    if trials == 0 {
        return None;
    }
    let n = trials as f64;
    let p = events as f64 / n;
    let z = 1.959963984540054_f64;
    let denominator = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denominator;
    let radius = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denominator;
    Some(Rate {
        events,
        trials,
        rate: p,
        wilson_95_descriptive: [(center - radius).max(0.0), (center + radius).min(1.0)],
    })
}
struct Scores {
    genuine: Zeroizing<Vec<f32>>,
    impostor: Zeroizing<Vec<f32>>,
    genuine_attempts: usize,
    impostor_attempts: usize,
}
fn scores(
    samples: &[Sample],
    embeddings: &[Option<Embedding>],
    split: Split,
) -> Result<Scores, Error> {
    let mut result = Scores {
        genuine: Zeroizing::new(Vec::new()),
        impostor: Zeroizing::new(Vec::new()),
        genuine_attempts: 0,
        impostor_attempts: 0,
    };
    for (i, first) in samples.iter().enumerate().filter(|(_, s)| s.split == split) {
        for (j, second) in samples
            .iter()
            .enumerate()
            .skip(i + 1)
            .filter(|(_, s)| s.split == split)
        {
            let genuine = first.identity == second.identity;
            if genuine && first.session == second.session {
                continue;
            }
            if genuine {
                result.genuine_attempts += 1;
            } else {
                result.impostor_attempts += 1;
            }
            if let (Some(a), Some(b)) = (&embeddings[i], &embeddings[j]) {
                let score = a.similarity(b)?;
                if genuine {
                    result.genuine.push(score);
                } else {
                    result.impostor.push(score);
                }
            }
        }
    }
    Ok(result)
}
fn calibrate(scores: &Scores, target_far: f64) -> Option<f32> {
    if scores.genuine.is_empty() || scores.impostor.is_empty() {
        return None;
    }
    let mut candidates = Zeroizing::new(vec![0.0]);
    candidates.extend(
        scores
            .impostor
            .iter()
            .map(|v| v.next_up())
            .filter(|v| (0.0..=1.0).contains(v)),
    );
    candidates.sort_unstable_by(f32::total_cmp);
    candidates.iter().copied().find(|threshold| {
        scores.impostor.iter().filter(|v| **v >= *threshold).count() as f64
            / scores.impostor.len() as f64
            <= target_far
    })
}
fn score_report(scores: &Scores, threshold: f32) -> serde_json::Value {
    serde_json::json!({
        "false_acceptance": rate(scores.impostor.iter().filter(|v| **v >= threshold).count(), scores.impostor.len()),
        "false_rejection": rate(scores.genuine.iter().filter(|v| **v < threshold).count(), scores.genuine.len()),
        "genuine_pair_attempts": scores.genuine_attempts, "impostor_pair_attempts": scores.impostor_attempts,
        "pairs_with_acquisition_failure": scores.genuine_attempts + scores.impostor_attempts - scores.genuine.len() - scores.impostor.len(),
    })
}
pub(crate) fn timing(values: &mut [u64]) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::Value::Null;
    }
    values.sort_unstable();
    let quantile = |fraction: f64| {
        values[((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
    };
    serde_json::json!({"samples": values.len(), "minimum_ms": values[0], "median_ms": quantile(0.5),
        "mean_ms":values.iter().map(|v| *v as f64).sum::<f64>()/values.len() as f64,
        "p95_ms": quantile(0.95), "p99_ms":quantile(0.99), "maximum_ms": values[values.len()-1]})
}

struct CheckedDataset {
    manifest: Manifest,
    root: PathBuf,
    manifest_sha256: String,
    dataset_sha256: String,
    image_digests: Vec<[u8; 32]>,
}

impl CheckedDataset {
    fn read_sample(&self, index: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
        let sample = &self.manifest.samples[index];
        let rgb = read_rgb(&self.root.join(&sample.path), sample.width, sample.height)?;
        let digest: [u8; 32] = Sha256::digest(&*rgb).into();
        if digest != self.image_digests[index] {
            return Err("dataset image changed after validation; evaluation aborted".into());
        }
        Ok(rgb)
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_dataset(path: &Path, cancel: &AtomicBool) -> Result<CheckedDataset, Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > 1024 * 1024 {
        return Err("invalid or oversized evaluation manifest".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("evaluation manifest exceeds limit".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    let manifest_digest = Sha256::digest(&bytes);
    let manifest_sha256 = hex_digest(&manifest_digest);
    let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut seen = BTreeSet::new();
    let mut image_digests = Vec::new();
    let mut dataset_digest = Sha256::new();
    dataset_digest.update(b"linux-hello-evaluation-dataset-v1\0");
    dataset_digest.update(manifest_digest);
    for sample in &manifest.samples {
        if cancel.load(Ordering::Acquire) {
            return Err("dataset check cancelled".into());
        }
        let rgb = read_rgb(&root.join(&sample.path), sample.width, sample.height)?;
        let digest: [u8; 32] = Sha256::digest(&*rgb).into();
        if !seen.insert(digest) {
            return Err("duplicate image bytes in dataset; evaluation leakage rejected".into());
        }
        dataset_digest.update(digest);
        image_digests.push(digest);
    }
    Ok(CheckedDataset {
        manifest,
        root,
        manifest_sha256,
        dataset_sha256: hex_digest(&dataset_digest.finalize()),
        image_digests,
    })
}

/// Validate all explicit local inputs without loading a model or starting capture.
pub fn dataset_check(path: &Path, cancel: Arc<AtomicBool>) -> Result<serde_json::Value, Error> {
    let checked = check_dataset(path, &cancel)?;
    let split_report = |split| {
        let samples: Vec<_> = checked
            .manifest
            .samples
            .iter()
            .filter(|s| s.split == split)
            .collect();
        let identities: BTreeSet<_> = samples.iter().map(|s| &s.identity).collect();
        let sessions: BTreeSet<_> = samples.iter().map(|s| (&s.identity, &s.session)).collect();
        serde_json::json!({"samples": samples.len(), "identities": identities.len(),
            "identity_sessions": sessions.len()})
    };
    Ok(
        serde_json::json!({"scope": "local dataset structure and byte validation only",
        "dataset_valid": true, "manifest_sha256": checked.manifest_sha256,
        "dataset_sha256": checked.dataset_sha256, "sensor_mode": checked.manifest.sensor_mode,
        "calibration": split_report(Split::Calibration), "evaluation": split_report(Split::Evaluation),
        "provenance_verified": false, "model_loaded": false, "capture_started": false,
        "authentication_ready": false, "embeddings_saved": false, "raw_frames_saved": false}),
    )
}

pub fn evaluate(
    executable: &Path,
    models: &Path,
    manifest_path: &Path,
    target_far: f64,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value, Error> {
    if !target_far.is_finite() || !(0.0..1.0).contains(&target_far) {
        return Err("target FAR must be finite and in [0,1)".into());
    }
    let checked = check_dataset(manifest_path, &cancel)?;
    let manifest = &checked.manifest;
    let mut embeddings = Vec::new();
    let mut failures = BTreeMap::<String, usize>::new();
    let mut startup = Vec::new();
    let mut inference = Vec::new();
    let mut supervised = Vec::new();
    let mut max_rss = 0;
    let mut observations = Vec::new();
    let mut native_cpu_us = 0;
    let mut annotated_faces = 0;
    let mut correct_face_counts = 0;
    let mut worker = Worker::spawn_stream(executable, models, cancel.clone())?.initialize()?;
    startup.push(worker.startup_ms as u64);
    for (index, sample) in manifest.samples.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("model evaluation cancelled".into());
        }
        let rgb = checked.read_sample(index)?;
        let started = Instant::now();
        let mut result = worker.infer_frame(sample.width, sample.height, &rgb)?;
        native_cpu_us += result.cpu_time_us;
        if let Some(expected) = sample.expected_faces {
            annotated_faces += 1;
            correct_face_counts += usize::from(result.faces == Some(expected));
        }
        supervised.push(started.elapsed().as_millis() as u64);
        inference.push(result.inference_ms);
        max_rss = max_rss.max(result.peak_rss_kib);
        if let Some(failure) = result.failure {
            *failures.entry(failure.to_string()).or_default() += 1;
        }
        observations.push(result.geometry.take());
        embeddings.push(result.embedding);
    }
    worker.finish()?;
    if manifest.version == 2 {
        let mut report =
            verification_evaluation::evaluate(manifest, &embeddings, &observations, target_far)?;
        report["manifest_sha256"] = checked.manifest_sha256.into();
        report["dataset_sha256"] = checked.dataset_sha256.into();
        report["sensor_mode"] = manifest.sensor_mode.clone().into();
        report["acquisition_failures"] = serde_json::to_value(failures)?;
        report["startup"] = timing(&mut startup);
        report["native_inference"] = timing(&mut inference);
        report["supervised_request"] = timing(&mut supervised);
        report["native_cpu_us"] = native_cpu_us.into();
        report["maximum_worker_rss_kib"] = max_rss.into();
        report["face_count_agreement"] =
            serde_json::to_value(rate(correct_face_counts, annotated_faces))?;
        return Ok(report);
    }
    let calibration = scores(&manifest.samples, &embeddings, Split::Calibration)?;
    let evaluation = scores(&manifest.samples, &embeddings, Split::Evaluation)?;
    let threshold = calibrate(&calibration, target_far);
    let available =
        threshold.is_some() && !evaluation.genuine.is_empty() && !evaluation.impostor.is_empty();
    Ok(
        serde_json::json!({"scope": "local pairwise model evaluation; no PAD or authentication verdict",
        "manifest_sha256": checked.manifest_sha256, "dataset_sha256": checked.dataset_sha256,
        "dataset_id": manifest.dataset_id, "sensor_mode": manifest.sensor_mode,
        "worker_mode":"bounded_stream", "native_cpu_us":native_cpu_us, "face_count_agreement":rate(correct_face_counts,annotated_faces),
        "sample_attempts": embeddings.len(), "successful_acquisitions": embeddings.iter().filter(|v| v.is_some()).count(),
        "acquisition_failures": failures, "evaluation_available": available, "calibration_target_empirical_far": target_far,
        "calibrated_threshold": threshold, "calibration": threshold.map(|t| score_report(&calibration,t)),
        "held_out_evaluation": threshold.filter(|_| available).map(|t| score_report(&evaluation,t)),
        "startup": timing(&mut startup), "native_inference": timing(&mut inference), "supervised_request": timing(&mut supervised),
        "maximum_worker_rss_kib": max_rss,
        "interval_assumption": "descriptive binomial Wilson intervals; all-pairs scores share subjects and are correlated; not population confidence or certification",
        "authentication_ready": false, "embeddings_saved": false, "raw_frames_saved": false}),
    )
}

pub fn benchmark(
    executable: &Path,
    models: &Path,
    rgb_path: &Path,
    width: u32,
    height: u32,
    iterations: u32,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value, Error> {
    benchmark_mode(
        executable,
        models,
        rgb_path,
        (width, height),
        iterations,
        cancel,
        false,
    )
}

pub fn benchmark_stream(
    executable: &Path,
    models: &Path,
    rgb_path: &Path,
    width: u32,
    height: u32,
    iterations: u32,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value, Error> {
    benchmark_mode(
        executable,
        models,
        rgb_path,
        (width, height),
        iterations,
        cancel,
        true,
    )
}

fn benchmark_mode(
    executable: &Path,
    models: &Path,
    rgb_path: &Path,
    dimensions: (u32, u32),
    iterations: u32,
    cancel: Arc<AtomicBool>,
    streaming: bool,
) -> Result<serde_json::Value, Error> {
    let (width, height) = dimensions;
    if !(1..=30).contains(&iterations) {
        return Err("benchmark iterations outside [1,30]".into());
    }
    let rgb = read_rgb(rgb_path, width, height)?;
    let mut startup = Vec::new();
    let mut inference = Vec::new();
    let mut supervised = Vec::new();
    let mut accepted = 0;
    let mut max_rss = 0;
    let mut cpu_us = 0;
    let mut worker = if streaming {
        let worker = Worker::spawn_stream(executable, models, cancel.clone())?.initialize()?;
        startup.push(worker.startup_ms as u64);
        Some(worker)
    } else {
        None
    };
    for _ in 0..iterations {
        let (result, elapsed) = if let Some(worker) = worker.as_mut() {
            let started = Instant::now();
            let result = worker.infer_frame(width, height, &rgb)?;
            (result, started.elapsed())
        } else {
            let worker = Worker::spawn(executable, models, cancel.clone())?.initialize()?;
            startup.push(worker.startup_ms as u64);
            let started = Instant::now();
            let result = worker.infer(width, height, &rgb)?;
            (result, started.elapsed())
        };
        cpu_us += result.cpu_time_us;
        supervised.push(elapsed.as_millis() as u64);
        inference.push(result.inference_ms);
        accepted += usize::from(result.embedding.is_some());
        max_rss = max_rss.max(result.peak_rss_kib);
    }
    if let Some(worker) = worker {
        worker.finish()?;
    }
    Ok(
        serde_json::json!({"scope": "explicit-file functional performance only; no biometric accuracy estimate",
        "worker_mode":if streaming {"bounded_stream"} else {"single_probe"}, "native_cpu_us":cpu_us,
        "iterations": iterations, "accepted": accepted, "width": width, "height": height,
        "startup": timing(&mut startup), "native_inference": timing(&mut inference),
        "supervised_request": timing(&mut supervised), "maximum_worker_rss_kib": max_rss,
        "authentication_ready": false, "embeddings_saved": false, "raw_frames_saved": false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    struct DatasetFixture(PathBuf);
    impl DatasetFixture {
        fn new() -> Self {
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("hello-dataset-{}-{suffix}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("images")).unwrap();
            let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
                "../../../docs/evaluation-manifest.example.json"
            ))
            .unwrap();
            for (index, sample) in manifest["samples"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                sample["width"] = 16.into();
                sample["height"] = 16.into();
                std::fs::write(
                    root.join(sample["path"].as_str().unwrap()),
                    vec![index as u8; 16 * 16 * 3],
                )
                .unwrap();
            }
            std::fs::write(
                root.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            Self(root)
        }
        fn manifest(&self) -> PathBuf {
            self.0.join("manifest.json")
        }
        fn image(&self, index: usize) -> PathBuf {
            let manifest: Manifest =
                serde_json::from_slice(&std::fs::read(self.manifest()).unwrap()).unwrap();
            self.0.join(&manifest.samples[index].path)
        }
    }
    impl Drop for DatasetFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn preflight_is_aggregate_only_and_binds_all_image_bytes() {
        let dataset = DatasetFixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let first = dataset_check(&dataset.manifest(), cancel.clone()).unwrap();
        assert_eq!(first["calibration"]["identities"], 2);
        assert_eq!(first["evaluation"]["identity_sessions"], 4);
        assert_eq!(first["model_loaded"], false);
        assert_eq!(first["provenance_verified"], false);
        let serialized = first.to_string();
        for label in ["cal-a", "day-1", ".rgb", "REPLACE-WITH"] {
            assert!(!serialized.contains(label));
        }
        assert_eq!(
            first,
            dataset_check(&dataset.manifest(), cancel.clone()).unwrap()
        );
        std::fs::write(dataset.image(7), vec![99; 16 * 16 * 3]).unwrap();
        let changed = dataset_check(&dataset.manifest(), cancel).unwrap();
        assert_eq!(first["manifest_sha256"], changed["manifest_sha256"]);
        assert_ne!(first["dataset_sha256"], changed["dataset_sha256"]);
    }

    #[test]
    fn late_duplicate_is_rejected_before_any_worker_is_started() {
        let dataset = DatasetFixture::new();
        std::fs::copy(dataset.image(0), dataset.image(7)).unwrap();
        let error = evaluate(
            Path::new("/never-start-worker"),
            Path::new("/missing-models"),
            &dataset.manifest(),
            0.01,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_err();
        assert!(error.to_string().contains("duplicate image bytes"));
    }

    #[test]
    fn changed_files_and_symlinks_cannot_replace_checked_sample() {
        let dataset = DatasetFixture::new();
        let checked = check_dataset(&dataset.manifest(), &AtomicBool::new(false)).unwrap();
        checked.read_sample(7).unwrap();
        std::fs::write(dataset.image(7), vec![99; 16 * 16 * 3]).unwrap();
        assert!(
            checked
                .read_sample(7)
                .unwrap_err()
                .to_string()
                .contains("changed after validation")
        );
        std::fs::remove_file(dataset.image(7)).unwrap();
        std::os::unix::fs::symlink(dataset.image(0), dataset.image(7)).unwrap();
        assert!(check_dataset(&dataset.manifest(), &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn every_identity_needs_sessions_and_preflight_honors_cancellation() {
        let dataset = DatasetFixture::new();
        assert!(check_dataset(&dataset.manifest(), &AtomicBool::new(true)).is_err());
        let mut manifest: Manifest =
            serde_json::from_slice(&std::fs::read(dataset.manifest()).unwrap()).unwrap();
        manifest.samples[1].session = manifest.samples[0].session.clone();
        assert!(
            manifest
                .validate()
                .unwrap_err()
                .to_string()
                .contains("two acquisition sessions")
        );
        std::fs::write(dataset.image(7), b"short").unwrap();
        assert!(
            check_dataset(&dataset.manifest(), &AtomicBool::new(false))
                .err()
                .unwrap()
                .to_string()
                .contains("exact byte count")
        );
    }
    fn sample(id: &str, identity: &str, session: &str, split: Split) -> Sample {
        Sample {
            id: id.into(),
            identity: identity.into(),
            session: session.into(),
            split,
            path: PathBuf::from(format!("{id}.rgb")),
            width: 32,
            height: 32,
            role: None,
            claimed_identity: None,
            attempt: None,
            expected_faces: None,
        }
    }
    #[test]
    fn manifest_rejects_identity_leakage_and_path_traversal_before_reads() {
        let mut manifest: Manifest = serde_json::from_str(include_str!(
            "../../../docs/evaluation-manifest.example.json"
        ))
        .unwrap();
        manifest.validate().unwrap();
        manifest.samples[4].identity = manifest.samples[0].identity.clone();
        assert!(manifest.validate().is_err());
        manifest.samples[4].identity = "different".into();
        manifest.samples[0].path = "../outside.rgb".into();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn failed_acquisitions_remain_in_attempt_counts() {
        let samples = vec![
            sample("a", "one", "first", Split::Evaluation),
            sample("b", "one", "second", Split::Evaluation),
            sample("c", "two", "first", Split::Evaluation),
        ];
        let embeddings = vec![
            Some(Embedding::new([1; 32], vec![1.0, 0.0]).unwrap()),
            None,
            Some(Embedding::new([1; 32], vec![0.0, 1.0]).unwrap()),
        ];
        let scores = scores(&samples, &embeddings, Split::Evaluation).unwrap();
        assert_eq!(scores.genuine_attempts, 1);
        assert_eq!(scores.impostor_attempts, 2);
        assert!(scores.genuine.is_empty());
        assert_eq!(
            score_report(&scores, 0.5)["pairs_with_acquisition_failure"],
            2
        );
    }

    #[test]
    fn held_out_pairs_and_cross_session_rules_are_preserved() {
        let samples = vec![
            sample("a", "one", "first", Split::Calibration),
            sample("b", "one", "second", Split::Calibration),
            sample("c", "two", "first", Split::Calibration),
            sample("d", "three", "first", Split::Evaluation),
            sample("e", "three", "first", Split::Evaluation),
            sample("f", "three", "second", Split::Evaluation),
            sample("g", "four", "first", Split::Evaluation),
        ];
        let embeddings = samples
            .iter()
            .map(|s| {
                Some(
                    Embedding::new(
                        [1; 32],
                        if s.identity == "one" || s.identity == "three" {
                            vec![1.0, 0.0]
                        } else {
                            vec![0.0, 1.0]
                        },
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let calibration = scores(&samples, &embeddings, Split::Calibration).unwrap();
        let threshold = calibrate(&calibration, 0.0).unwrap();
        let evaluation = scores(&samples, &embeddings, Split::Evaluation).unwrap();
        assert_eq!(calibration.genuine.len(), 1);
        assert_eq!(evaluation.genuine.len(), 2); // The same-session pair is excluded.
        assert_eq!(evaluation.impostor.len(), 3);
        assert_eq!(
            score_report(&evaluation, threshold)["false_acceptance"]["events"],
            0
        );
        assert_eq!(
            score_report(&evaluation, threshold)["false_rejection"]["events"],
            0
        );
    }
    #[test]
    fn zero_observed_errors_do_not_imply_zero_upper_uncertainty() {
        let result = rate(0, 10).unwrap();
        assert_eq!(result.rate, 0.0);
        assert!(result.wilson_95_descriptive[1] > 0.25);
        assert!(rate(0, 0).is_none());
    }
    #[test]
    fn calibration_does_not_treat_missing_or_perfect_impostor_scores_as_success() {
        let scores = Scores {
            genuine: Zeroizing::new(vec![1.0]),
            impostor: Zeroizing::new(vec![1.0]),
            genuine_attempts: 1,
            impostor_attempts: 1,
        };
        assert!(calibrate(&scores, 0.01).is_none());
    }
}
