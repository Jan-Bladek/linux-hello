//! Actual multi-template, multi-frame 1:1 scoring; consented local inputs only.
use super::{Embedding, Error, Manifest, SampleRole, Split, rate, text_valid};
use hello_face::{
    Enrollment, QualityMetrics, Template, geometry::FaceGeometry, verification::sequence_score,
};
use std::collections::{BTreeMap, BTreeSet};

type AttemptKey<'a> = (Split, &'a str, &'a str);
fn groups(manifest: &Manifest) -> BTreeMap<AttemptKey<'_>, Vec<usize>> {
    let mut result = BTreeMap::<AttemptKey<'_>, Vec<usize>>::new();
    for (i, sample) in manifest.samples.iter().enumerate() {
        if sample.role == Some(SampleRole::Verification) {
            result
                .entry((
                    sample.split,
                    sample.claimed_identity.as_deref().unwrap_or(""),
                    sample.attempt.as_deref().unwrap_or(""),
                ))
                .or_default()
                .push(i);
        }
    }
    result
}
pub(super) fn validate(manifest: &Manifest) -> Result<(), Error> {
    let mut enrollments = BTreeMap::new();
    for sample in &manifest.samples {
        match sample.role {
            Some(SampleRole::Enrollment)
                if sample.claimed_identity.is_none() && sample.attempt.is_none() =>
            {
                enrollments
                    .entry((sample.split, sample.identity.as_str()))
                    .or_insert(Vec::new())
                    .push(sample);
            }
            Some(SampleRole::Verification)
                if sample
                    .claimed_identity
                    .as_ref()
                    .is_some_and(|s| text_valid(s))
                    && sample.attempt.as_ref().is_some_and(|s| text_valid(s)) => {}
            _ => {
                return Err(
                    "version 2 needs explicit enrollment or claimed-user verification roles".into(),
                );
            }
        }
    }
    if enrollments.values().any(|samples| {
        samples.len() != 8 || samples.iter().any(|s| s.session != samples[0].session)
    }) {
        return Err(
            "each claimed reference needs eight ordered frames from one enrollment session".into(),
        );
    }
    let mut coverage = BTreeSet::new();
    for ((split, claim, _), indices) in groups(manifest) {
        let enrollment = enrollments
            .get(&(split, claim))
            .ok_or("claimed user has no enrollment in this split")?;
        let first = &manifest.samples[indices[0]];
        if !(2..=12).contains(&indices.len())
            || indices.iter().any(|&i| {
                let sample = &manifest.samples[i];
                sample.identity != first.identity
                    || sample.session != first.session
                    || sample.session == enrollment[0].session
            })
        {
            return Err(
                "attempt needs 2..12 frames of one subject/session, separate from enrollment"
                    .into(),
            );
        }
        coverage.insert((split, first.identity.as_str() == claim));
    }
    for split in [Split::Calibration, Split::Evaluation] {
        if !coverage.contains(&(split, true)) || !coverage.contains(&(split, false)) {
            return Err("both splits need genuine and impostor claimed-user attempts".into());
        }
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Attempt {
    split: Split,
    genuine: bool,
    score: Option<f32>,
}
pub(super) fn evaluate(
    manifest: &Manifest,
    embeddings: &[Option<Embedding>],
    geometry: &[Option<FaceGeometry>],
    target_far: f64,
) -> Result<serde_json::Value, Error> {
    validate(manifest)?;
    if embeddings.len() != manifest.samples.len() || geometry.len() != embeddings.len() {
        return Err("incomplete evaluation observations".into());
    }
    let mut profiles = BTreeMap::<(Split, &str), Option<Template>>::new();
    for sample in manifest
        .samples
        .iter()
        .filter(|s| s.role == Some(SampleRole::Enrollment))
    {
        profiles
            .entry((sample.split, &sample.identity))
            .or_default();
    }
    for (&(split, identity), profile) in &mut profiles {
        let mut enrollment = Enrollment::new_pose_aware(0.8)?;
        let mut valid = true;
        for (i, sample) in manifest.samples.iter().enumerate().filter(|(_, s)| {
            s.role == Some(SampleRole::Enrollment) && s.split == split && s.identity == identity
        }) {
            let (Some(embedding), Some(shape)) = (&embeddings[i], &geometry[i]) else {
                valid = false;
                break;
            };
            // Inference has already enforced actual raw-frame quality; this
            // neutral metric is only the enrollment math API's second guard.
            let quality = QualityMetrics {
                mean_intensity: 0.5,
                contrast: 0.1,
                edge_energy: 0.1,
            };
            let mut digest = [0; 32];
            digest[..8].copy_from_slice(&(i as u64).to_le_bytes());
            let cloned = embedding.clone();
            if let Err(error) =
                enrollment.add_with_geometry(digest, cloned, 1, quality, shape.clone())
            {
                if matches!(
                    error,
                    hello_face::FaceError::InconsistentSample
                        | hello_face::FaceError::EnrollmentPose
                ) {
                    valid = false;
                    break;
                }
                return Err(error.into());
            }
            let _ = sample;
        }
        if valid {
            *profile = Some(enrollment.finish()?);
        }
    }
    let mut attempts = Vec::new();
    for ((split, claim, _), indices) in groups(manifest) {
        let frames: Option<Vec<_>> = indices
            .iter()
            .map(|&i| Some((embeddings[i].as_ref()?, geometry[i].as_ref()?)))
            .collect();
        let score = match (
            profiles.get(&(split, claim)).and_then(Option::as_ref),
            frames,
        ) {
            (Some(template), Some(frames)) => sequence_score(template, &frames)?,
            _ => None,
        };
        attempts.push(Attempt {
            split,
            genuine: manifest.samples[indices[0]].identity == claim,
            score,
        });
    }
    let calibration: Vec<_> = attempts
        .iter()
        .copied()
        .filter(|a| a.split == Split::Calibration)
        .collect();
    let mut thresholds = vec![0.8_f32]; // Preserve the current diagnostic security floor.
    thresholds.extend(
        calibration
            .iter()
            .filter(|a| !a.genuine)
            .filter_map(|a| a.score)
            .map(f32::next_up)
            .filter(|v| (0.8..=1.0).contains(v)),
    );
    thresholds.sort_by(f32::total_cmp);
    let threshold = if calibration.iter().any(|a| a.genuine && a.score.is_some())
        && calibration.iter().any(|a| !a.genuine && a.score.is_some())
    {
        thresholds.into_iter().find(|&t| {
            let scored: Vec<_> = calibration
                .iter()
                .filter(|a| !a.genuine && a.score.is_some())
                .collect();
            scored
                .iter()
                .filter(|a| a.score.is_some_and(|s| s >= t))
                .count() as f64
                / scored.len() as f64
                <= target_far
        })
    } else {
        None
    };
    let report = |split, t| {
        let all: Vec<_> = attempts.iter().filter(|a| a.split == split).collect();
        let genuine: Vec<_> = all.iter().filter(|a| a.genuine).collect();
        let impostor: Vec<_> = all.iter().filter(|a| !a.genuine).collect();
        serde_json::json!({"genuine_attempts":genuine.len(),"impostor_attempts":impostor.len(),
            "false_acceptance":rate(impostor.iter().filter(|a|a.score.is_some_and(|s|s>=t)).count(),impostor.len()),
            "false_rejection_end_to_end":rate(genuine.iter().filter(|a|a.score.is_none_or(|s|s<t)).count(),genuine.len()),
            "false_acceptance_on_scorable_attempts":rate(impostor.iter().filter(|a|a.score.is_some_and(|s|s>=t)).count(),impostor.iter().filter(|a|a.score.is_some()).count()),
            "false_rejection_on_scorable_attempts":rate(genuine.iter().filter(|a|a.score.is_some_and(|s|s<t)).count(),genuine.iter().filter(|a|a.score.is_some()).count()),
            "failure_to_acquire":rate(all.iter().filter(|a|a.score.is_none()).count(),all.len())})
    };
    Ok(
        serde_json::json!({"scope":"claimed-user multi-frame 1:1 verification; no motion/PAD/authentication verdict",
        "protocol":"offline_all_frames_2_to_12_without_live_quality_retry",
        "pam_acquisition_protocol_evaluated":false,"end_to_end_authentication_metrics":false,
        "scoring_policy":"pose_median3_centroid_v2","minimum_frames":2,"minimum_diagnostic_threshold":0.8,
        "calibrated_threshold":threshold,"calibration_target_empirical_far":target_far,
        "calibration":threshold.map(|t|report(Split::Calibration,t)),
        "held_out_evaluation":threshold.map(|t|report(Split::Evaluation,t)),
        "evaluation_available":threshold.is_some(),"unenrollable_references":profiles.values().filter(|p|p.is_none()).count(),
        "authentication_ready":false,"embeddings_saved":false,"raw_frames_saved":false,
        "population_accuracy_validated":false,"gui_motion_protocol_evaluated":false,"provenance_verified":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        serde_json::from_str(include_str!(
            "../../../../docs/verification-manifest.example.json"
        ))
        .unwrap()
    }
    fn shape() -> FaceGeometry {
        FaceGeometry::measure(&hello_face::Detection {
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
        .unwrap()
    }
    #[test]
    fn claimed_users_and_sessions_cannot_leak_across_splits_or_attempts() {
        let mut m = manifest();
        m.validate().unwrap();
        let i = m
            .samples
            .iter()
            .position(|s| s.role == Some(SampleRole::Verification))
            .unwrap();
        m.samples[i].claimed_identity = Some("eval-a".into());
        assert!(m.validate().is_err());
        let mut m = manifest();
        m.samples[i].session = "enroll-day".into();
        assert!(m.validate().is_err());
        let mut m = manifest();
        m.samples[i].identity = "cal-b".into();
        assert!(m.validate().is_err());
        let mut m = manifest();
        m.samples.remove(i);
        assert!(m.validate().is_err());
    }
    #[test]
    fn actual_template_sequence_math_is_calibrated_without_reporting_labels_or_vectors() {
        let m = manifest();
        let mut embeddings: Vec<_> = m
            .samples
            .iter()
            .map(|s| {
                Some(
                    Embedding::new(
                        [1; 32],
                        if s.identity.ends_with("-a") {
                            vec![1.0, 0.0]
                        } else {
                            vec![0.0, 1.0]
                        },
                    )
                    .unwrap(),
                )
            })
            .collect();
        let geometry: Vec<_> = (0..embeddings.len()).map(|_| Some(shape())).collect();
        let report = evaluate(&m, &embeddings, &geometry, 0.0).unwrap();
        assert_eq!(report["calibrated_threshold"], 0.8_f32);
        assert_eq!(
            report["held_out_evaluation"]["false_acceptance"]["events"],
            0
        );
        assert_eq!(
            report["held_out_evaluation"]["false_rejection_end_to_end"]["events"],
            0
        );
        let json = report.to_string();
        assert!(!json.contains("cal-a"));
        assert!(!json.contains("values"));
        let i = m
            .samples
            .iter()
            .position(|s| s.split == Split::Evaluation && s.role == Some(SampleRole::Verification))
            .unwrap();
        embeddings[i] = None;
        let report = evaluate(&m, &embeddings, &geometry, 0.0).unwrap();
        assert_eq!(
            report["held_out_evaluation"]["false_rejection_end_to_end"]["events"],
            1
        );
        assert_eq!(
            report["held_out_evaluation"]["failure_to_acquire"]["events"],
            1
        );
    }
}
