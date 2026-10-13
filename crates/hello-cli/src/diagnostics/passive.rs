//! Short, explicit 1:1 verification without a head-motion challenge. Not evaluated PAD.
use super::*;

pub struct PassiveProgress {
    pub accepted: usize,
    pub hint: String,
}

pub async fn verify_person_observed<F, Fut, P>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    mut capture: F,
    mut progress: P,
) -> Result<ComparisonSummary, Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
    P: FnMut(PassiveProgress),
{
    let check_cancel = || -> Result<(), Error> {
        if cancel.load(Ordering::Acquire) {
            Err("Recognition cancelled".into())
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let revision = {
        let reference = reference.lock().map_err(|_| "reference unavailable")?;
        reference.validate_strict_recognition(identity, ir)?;
        reference.revision
    };
    let mut scores = Vec::with_capacity(3);
    let mut digests = Vec::new();
    let mut attempts = 0;
    let mut weakest = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut hint = "Look straight at the camera. No head turn is needed.".to_owned();
    for _ in 0..16 {
        check_cancel()?;
        if Instant::now() >= deadline {
            break;
        }
        progress(PassiveProgress {
            accepted: scores.len(),
            hint: hint.clone(),
        });
        let probe = match capture().await {
            Ok(probe) => probe,
            Err(error) if error.is::<TransientCapture>() => {
                check_cancel()?;
                hint = format!("{error} Look straight ahead — waiting for a clear sample.");
                continue;
            }
            Err(error) => return Err(error),
        };
        check_cancel()?;
        if Instant::now() >= deadline {
            break;
        }
        if digests.contains(&probe.frame_digest) {
            return Err("Repeated camera frame — scan stopped".into());
        }
        digests.push(probe.frame_digest);
        attempts += probe.report["sample_capture_attempts"]
            .as_u64()
            .unwrap_or(1);
        if probe.faces.is_some_and(|n| n > 1) {
            return Err("Exactly one face is required".into());
        }
        if let Some(reason) = probe
            .sample_quality
            .and_then(|q| q.rejection(probe.confidence.unwrap_or(0.0)))
        {
            hint = quality_hint(reason);
            continue;
        }
        if probe.faces != Some(1) || probe.report["diagnostic_passed"] != true {
            if probe.faces == Some(0)
                || matches!(
                    probe.report["rejection"].as_str(),
                    Some("sample quality is insufficient" | "exactly one face is required")
                )
            {
                hint =
                    "No clear face detected — look straight ahead with both eyes visible.".into();
                continue;
            }
            return Err("Invalid model output — scan stopped".into());
        }
        if probe.sample_quality.is_none() {
            return Err("Cannot assess face quality".into());
        }
        let geometry = probe
            .geometry
            .as_ref()
            .filter(|g| g.valid())
            .ok_or("Valid face geometry unavailable")?;
        let evidence = {
            let locked = reference.lock().map_err(|_| "reference unavailable")?;
            locked.validate_strict_recognition(identity, ir)?;
            if locked.revision != revision {
                return Err("Reference changed during recognition".into());
            }
            let Some(Reference::Multi(template)) = &locked.reference else {
                return Err("reference unavailable".into());
            };
            template.verification_evidence(
                probe.embedding.as_ref().ok_or("Brak cech twarzy")?,
                geometry,
            )?
        };
        if !evidence.score.is_finite() {
            return Err("Invalid comparison result".into());
        }
        if evidence.score < STRICT_THRESHOLD_FLOOR {
            let mut summary = strict_summary(vec![evidence.score], attempts, false);
            summary.verification_protocol = VerificationProtocol::PassiveFront;
            summary.verification_evidence = Some(evidence);
            summary.failure_reason = Some(if !evidence.shape_compatible {
                "landmark_shape_mismatch"
            } else if evidence.centroid_cosine < STRICT_THRESHOLD_FLOOR {
                "centroid_similarity_below_threshold"
            } else {
                "exemplar_similarity_below_threshold"
            });
            return Ok(summary);
        }
        if weakest.is_none_or(|prior: hello_face::verification::VerificationEvidence| {
            evidence.score < prior.score
        }) {
            weakest = Some(evidence);
        }
        scores.push(evidence.score);
        hint =
            "Face matches. Keep looking straight ahead while the next frame is confirmed.".into();
        if scores.len() == 3 {
            check_cancel()?;
            let mut summary = strict_summary(scores, attempts, false);
            summary.verification_protocol = VerificationProtocol::PassiveFront;
            summary.verification_evidence = weakest;
            progress(PassiveProgress {
                accepted: 3,
                hint: "Three matching frames — finishing verification.".into(),
            });
            check_cancel()?;
            let locked = reference.lock().map_err(|_| "reference unavailable")?;
            locked.validate_strict_recognition(identity, ir)?;
            if locked.revision != revision {
                return Err("Reference changed during recognition".into());
            }
            return Ok(summary);
        }
    }
    check_cancel()?;
    Err(format!(
        "Three clear frames were not obtained ({}/3). {hint}",
        scores.len()
    )
    .into())
}
