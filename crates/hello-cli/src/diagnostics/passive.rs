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
            Err("Rozpoznawanie anulowane".into())
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
    let mut hint = "Spójrz prosto w kamerę. Nie musisz skręcać głowy.".to_owned();
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
                hint = format!("{error} Patrz prosto — czekam na czytelne ujęcie.");
                continue;
            }
            Err(error) => return Err(error),
        };
        check_cancel()?;
        if Instant::now() >= deadline {
            break;
        }
        if digests.contains(&probe.frame_digest) {
            return Err("Powtórzona klatka kamery — skan przerwany".into());
        }
        digests.push(probe.frame_digest);
        attempts += probe.report["sample_capture_attempts"]
            .as_u64()
            .unwrap_or(1);
        if probe.faces.is_some_and(|n| n > 1) {
            return Err("Wymagana dokładnie jedna twarz".into());
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
                hint = "Nie widzę wyraźnej twarzy — patrz prosto i pokaż oba oczy.".into();
                continue;
            }
            return Err("Nieprawidłowy wynik modelu — skan przerwany".into());
        }
        if probe.sample_quality.is_none() {
            return Err("Nie można ocenić jakości twarzy".into());
        }
        let geometry = probe
            .geometry
            .as_ref()
            .filter(|g| g.valid())
            .ok_or("Brak prawidłowej geometrii twarzy")?;
        let evidence = {
            let locked = reference.lock().map_err(|_| "reference unavailable")?;
            locked.validate_strict_recognition(identity, ir)?;
            if locked.revision != revision {
                return Err("Wzorzec zmienił się podczas rozpoznawania".into());
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
            return Err("Nieprawidłowy wynik porównania".into());
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
        hint = "Twarz pasuje. Patrz prosto jeszcze chwilę — potwierdzam kolejne ujęcie.".into();
        if scores.len() == 3 {
            check_cancel()?;
            let mut summary = strict_summary(scores, attempts, false);
            summary.verification_protocol = VerificationProtocol::PassiveFront;
            summary.verification_evidence = weakest;
            progress(PassiveProgress {
                accepted: 3,
                hint: "Trzy zgodne ujęcia — kończę sprawdzanie.".into(),
            });
            check_cancel()?;
            let locked = reference.lock().map_err(|_| "reference unavailable")?;
            locked.validate_strict_recognition(identity, ir)?;
            if locked.revision != revision {
                return Err("Wzorzec zmienił się podczas rozpoznawania".into());
            }
            return Ok(summary);
        }
    }
    check_cancel()?;
    Err(format!(
        "Nie uzyskano trzech czytelnych ujęć ({}/3). {hint}",
        scores.len()
    )
    .into())
}
