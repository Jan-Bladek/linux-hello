//! Shared bounded face diagnostic for CLI and GUI; never grants authentication.
use crate::face_worker::{Error, Worker};
mod live;
mod passive;
use hello_camera::{CameraSession, CaptureConfig, DeviceIdentity};
pub use hello_face::sampling::{REFERENCE_SAMPLES, sample_instruction};
pub use live::LiveCapture;
pub use passive::{PassiveProgress, verify_person_observed};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Only transient acquisition failure may be retried; protocol/model errors stay fatal.
#[derive(Debug)]
pub struct TransientCapture(pub &'static str);
impl std::fmt::Display for TransientCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for TransientCapture {}
const POSE_ATTEMPTS: usize = 16;
const POSE_TIME: Duration = Duration::from_secs(15);

/// Private biometric result; intentionally has no serialization or Debug implementation.
pub struct FaceProbe {
    pub report: serde_json::Value,
    embedding: Option<hello_face::Embedding>,
    frame_digest: [u8; 32],
    quality: hello_face::QualityMetrics,
    faces: Option<usize>,
    sample_quality: Option<hello_face::sampling::SampleQuality>,
    confidence: Option<f32>,
    geometry: Option<hello_face::geometry::FaceGeometry>,
}

enum Reference {
    Single(DeviceIdentity, bool, hello_face::Embedding),
    Multi(hello_face::Template),
}
struct PendingEnrollment {
    binding: [u8; 32],
    enrollment: hello_face::Enrollment,
    display_name: Option<zeroize::Zeroizing<String>>,
}

/// Explicit diagnostic samples; no trusted identity enrollment or authentication.
#[derive(Default)]
pub struct SessionReference {
    reference: Option<Reference>,
    pending: Option<PendingEnrollment>,
    revision: u64,
}

static NEXT_REFERENCE_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn capture_binding(identity: &DeviceIdentity, ir: bool) -> Result<[u8; 32], Error> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"linux-hello-diagnostic-camera-v1");
    digest.update(serde_json::to_vec(identity)?);
    digest.update([u8::from(ir)]);
    Ok(digest.finalize().into())
}

impl SessionReference {
    /// A caller supplying a protected template must still validate camera binding
    /// and strict enrollment before opening the sensor. No result is cached.
    pub fn from_template(template: hello_face::Template) -> Result<Self, Error> {
        if template.capture_binding().is_none()
            || template.dimensions() != 128
            || !template.strict_available()
            || template.sample_count() != REFERENCE_SAMPLES
        {
            return Err("incompatible or incomplete reference".into());
        }
        Ok(Self {
            reference: Some(Reference::Multi(template)),
            pending: None,
            revision: NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed),
        })
    }
    pub fn begin_anonymous_enrollment(
        &mut self,
        identity: &DeviceIdentity,
        ir: bool,
    ) -> Result<(), Error> {
        self.pending = Some(PendingEnrollment {
            binding: capture_binding(identity, ir)?,
            enrollment: hello_face::Enrollment::new_pose_aware(0.8)?,
            display_name: None,
        });
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    pub fn has_reference(&self) -> bool {
        self.reference.is_some()
    }
    pub fn enrollment_active(&self) -> bool {
        self.pending.is_some()
    }
    pub fn enrollment_sample_count(&self) -> usize {
        self.pending
            .as_ref()
            .map_or(0, |pending| pending.enrollment.sample_count())
    }
    pub fn enrollment_display_name(&self) -> Option<&str> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.display_name.as_deref().map(|name| name.as_str()))
    }
    pub fn display_name(&self) -> Option<&str> {
        match &self.reference {
            Some(Reference::Multi(template)) => template.display_name(),
            _ => None,
        }
    }

    pub fn rename(&mut self, name: &str) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err("finish or cancel adding a person before renaming".into());
        }
        let Some(Reference::Multi(template)) = &mut self.reference else {
            return Err("collect a person reference before renaming".into());
        };
        template.set_display_name(name)?;
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn begin_named_enrollment(
        &mut self,
        identity: &DeviceIdentity,
        ir: bool,
        name: &str,
    ) -> Result<(), Error> {
        let name = hello_face::normalize_display_name(name)?;
        self.pending = Some(PendingEnrollment {
            binding: capture_binding(identity, ir)?,
            enrollment: hello_face::Enrollment::new_pose_aware(0.8)?,
            display_name: Some(zeroize::Zeroizing::new(name)),
        });
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn cancel_enrollment(&mut self) {
        if self.pending.take().is_some() {
            self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Recognition experiments require the completed eight-sample reference.
    pub fn validate_recognition(&self, identity: &DeviceIdentity, ir: bool) -> Result<(), Error> {
        self.validate_comparison(identity, ir)?;
        if !self.template_available() || self.reference_sample_count() < REFERENCE_SAMPLES {
            return Err(
                "complete eight reference samples or unlock an eight-sample reference first".into(),
            );
        }
        Ok(())
    }
    pub fn validate_strict_recognition(
        &self,
        identity: &DeviceIdentity,
        ir: bool,
    ) -> Result<(), Error> {
        self.validate_recognition(identity, ir)?;
        if !matches!(&self.reference, Some(Reference::Multi(t)) if t.strict_available()) {
            return Err("Zapisany wzorzec nie zawiera geometrii i poszczególnych ujęć. Zeskanuj swoją twarz ponownie; stary zapis nie wystarcza do ścisłego rozpoznawania.".into());
        }
        Ok(())
    }
    /// Check the actual reference and camera binding before opening a camera.
    pub fn validate_comparison(&self, identity: &DeviceIdentity, ir: bool) -> Result<(), Error> {
        match &self.reference {
            None => Err("remember a face or unlock a saved reference first".into()),
            Some(Reference::Single(expected, expected_ir, _))
                if expected != identity || *expected_ir != ir =>
            {
                Err("camera or IR mode differs from reference".into())
            }
            Some(Reference::Multi(template))
                if template.capture_binding() != Some(capture_binding(identity, ir)?) =>
            {
                Err("camera or IR mode differs from saved reference".into())
            }
            _ => Ok(()),
        }
    }
    pub fn clear(&mut self) {
        self.reference = None;
        self.pending = None;
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
    }
    pub fn template_available(&self) -> bool {
        self.pending.is_none() && matches!(self.reference, Some(Reference::Multi(_)))
    }

    pub fn remember(
        &mut self,
        identity: DeviceIdentity,
        ir: bool,
        probe: FaceProbe,
    ) -> Result<(), Error> {
        let embedding = probe
            .embedding
            .ok_or("one suitable face is required to remember a reference")?;
        self.reference = Some(Reference::Single(identity, ir, embedding));
        self.pending = None;
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn reference_sample_count(&self) -> usize {
        match &self.reference {
            Some(Reference::Multi(t)) => t.sample_count(),
            _ => 0,
        }
    }

    pub fn next_instruction(&self) -> &'static str {
        sample_instruction(
            self.pending
                .as_ref()
                .map_or(0, |p| p.enrollment.sample_count()),
        )
    }

    /// Eight distinct, quality-checked samples; 0.8 is a provisional coherence
    /// filter, never an evaluated verification threshold or identity verdict.
    pub fn add_sample(
        &mut self,
        identity: &DeviceIdentity,
        ir: bool,
        probe: FaceProbe,
    ) -> Result<usize, Error> {
        let binding = capture_binding(identity, ir)?;
        if !probe.quality.has_signal() {
            return Err(hello_face::FaceError::PoorQuality.into());
        }
        let quality = probe
            .sample_quality
            .ok_or("face detail could not be assessed; look straight at the camera")?;
        if let Some(reason) = quality.rejection(probe.confidence.unwrap_or(0.0)) {
            return Err(reason.into());
        }
        let embedding = probe
            .embedding
            .ok_or("one suitable face is required for a sample")?;
        if self.pending.as_ref().is_some_and(|p| p.binding != binding) {
            return Err(
                "camera or IR mode changed during sampling; forget reference to restart".into(),
            );
        }
        let pending = self.pending.get_or_insert(PendingEnrollment {
            binding,
            enrollment: hello_face::Enrollment::new_pose_aware(0.8)?,
            display_name: None,
        });
        if let Some(geometry) = probe.geometry {
            pending.enrollment.add_with_geometry(
                probe.frame_digest,
                embedding,
                probe.faces.unwrap_or(0),
                quality.face_quality,
                geometry,
            )?;
        } else {
            pending.enrollment.add(
                probe.frame_digest,
                embedding,
                probe.faces.unwrap_or(0),
                quality.face_quality,
            )?;
        }
        let count = pending.enrollment.sample_count();
        if count == REFERENCE_SAMPLES {
            let pending = self.pending.take().expect("complete accepted sample set");
            let mut template = pending.enrollment.finish()?.bind_capture(binding)?;
            if let Some(name) = &pending.display_name {
                template.set_display_name(name)?;
            }
            self.reference = Some(Reference::Multi(template));
        }
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(count)
    }

    pub fn save(&self, path: &std::path::Path, password: &[u8]) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err("finish all eight new samples before saving".into());
        }
        let Some(Reference::Multi(template)) = &self.reference else {
            return Err("collect eight reference samples before saving".into());
        };
        let store = hello_storage::SecureStore::open_passphrase(path, password, true)?;
        // A wrong passphrase must never overwrite an existing reference.
        let _ = store.load()?;
        store.save(template)?;
        Ok(())
    }

    pub fn load(&mut self, path: &std::path::Path, password: &[u8]) -> Result<(), Error> {
        let store = hello_storage::SecureStore::open_passphrase(path, password, false)?;
        let template = store.load()?.ok_or("no saved reference found")?;
        if template.capture_binding().is_none() || template.dimensions() != 128 {
            return Err("saved reference is incompatible with camera diagnostics".into());
        }
        self.reference = Some(Reference::Multi(template));
        self.pending = None;
        self.revision = NEXT_REFERENCE_REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn delete_saved(&mut self, path: &std::path::Path) -> Result<bool, Error> {
        let removed = hello_storage::SecureStore::remove_local_reference(path)?;
        self.clear();
        Ok(removed)
    }

    /// Returns a cosine score only; no threshold or authentication verdict.
    pub fn compare(
        &self,
        identity: &DeviceIdentity,
        ir: bool,
        probe: &FaceProbe,
    ) -> Result<Option<f32>, Error> {
        let Some(reference) = &self.reference else {
            return Ok(None);
        };
        let embedding = probe
            .embedding
            .as_ref()
            .ok_or("one suitable face is required for comparison")?;
        match reference {
            Reference::Single(expected, expected_ir, reference) => {
                if expected != identity || *expected_ir != ir {
                    return Err(
                        "camera or IR mode changed; forget the reference and capture a new one"
                            .into(),
                    );
                }
                Ok(Some(reference.similarity(embedding)?))
            }
            Reference::Multi(template) => {
                if template.capture_binding() != Some(capture_binding(identity, ir)?) {
                    return Err("camera or IR mode differs from saved reference; use the original settings or collect new samples".into());
                }
                Ok(Some(template.similarity(embedding)?))
            }
        }
    }
}

/// Conservative provisional threshold, not calibrated for this camera/IR.
pub const DEFAULT_RECOGNITION_THRESHOLD: f32 = 0.80;
pub const STRICT_THRESHOLD_FLOOR: f32 = 0.80;

/// Complete/resume the eight-frame set, retrying transient acquisition quality
/// within a bounded pose budget and retaining accepted samples on failure.
pub async fn collect_reference<F, Fut, P>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    capture: F,
    progress: P,
) -> Result<usize, Error>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
    P: FnMut(usize),
{
    collect_reference_observed(reference, identity, ir, cancel, capture, progress, |_| {}).await
}

pub async fn collect_reference_observed<F, Fut, P, Q>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    mut capture: F,
    mut progress: P,
    mut feedback: Q,
) -> Result<usize, Error>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
    P: FnMut(usize),
    Q: FnMut(&str),
{
    let check_cancel = || -> Result<(), Error> {
        if cancel.load(Ordering::Acquire) {
            Err("face scan cancelled".into())
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let mut count = {
        let reference = reference.lock().map_err(|_| "reference unavailable")?;
        let pending = reference
            .pending
            .as_ref()
            .ok_or("start a face scan first")?;
        if pending.binding != capture_binding(identity, ir)? {
            return Err("camera or IR mode changed during scanning".into());
        }
        pending.enrollment.sample_count()
    };
    while count < REFERENCE_SAMPLES {
        check_cancel()?;
        progress(count);
        let deadline = Instant::now() + POSE_TIME;
        let mut accepted = None;
        let mut inconsistent_frames = 0;
        let mut last_hint = "Nie widzę czytelnej twarzy".to_owned();
        for _ in 0..8 {
            check_cancel()?;
            if Instant::now() >= deadline {
                break;
            }
            let probe = match capture(count).await {
                Ok(probe) => probe,
                Err(error) if error.is::<TransientCapture>() => {
                    check_cancel()?;
                    last_hint = error.to_string();
                    feedback(&last_hint);
                    continue;
                }
                Err(error) => return Err(error),
            };
            check_cancel()?;
            if probe.faces.is_some_and(|n| n > 1) {
                return Err("Wymagana dokładnie jedna twarz".into());
            }
            if probe.faces == Some(0) {
                last_hint = "Nie widzę twarzy — pokaż oba oczy".into();
                feedback(&last_hint);
                continue;
            }
            if let Some(reason) = probe
                .sample_quality
                .and_then(|q| q.rejection(probe.confidence.unwrap_or(0.0)))
            {
                last_hint = quality_hint(reason);
                feedback(&last_hint);
                continue;
            }
            let added = reference
                .lock()
                .map_err(|_| "reference unavailable")?
                .add_sample(identity, ir, probe);
            match added {
                Ok(count) => {
                    accepted = Some(count);
                    break;
                }
                Err(error)
                    if matches!(
                        error.downcast_ref::<hello_face::FaceError>(),
                        Some(hello_face::FaceError::EnrollmentPose)
                    ) =>
                {
                    last_hint = "Pierwsze trzy ujęcia zbieram na wprost. Nie skręcaj jeszcze głowy — przytrzymaj pozycję.".into();
                    feedback(&last_hint);
                    continue;
                }
                Err(error)
                    if matches!(
                        error.downcast_ref::<hello_face::FaceError>(),
                        Some(hello_face::FaceError::InconsistentSample)
                    ) && inconsistent_frames < 2 =>
                {
                    inconsistent_frames += 1;
                    last_hint = "Ujęcie podczas skrętu nie pasuje do poprzednich. Zmniejsz skręt i przytrzymaj głowę — ponawiam".into();
                    feedback(&last_hint);
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        check_cancel()?;
        count = accepted.ok_or_else(|| {
            format!("{last_hint}. Upłynął czas tego ujęcia; przyjęte próbki zachowano.")
        })?;
    }
    progress(count);
    Ok(count)
}

#[derive(Clone, Copy)]
pub struct RecognitionThreshold(f32);

impl RecognitionThreshold {
    pub fn new(value: f32) -> Result<Self, Error> {
        if !value.is_finite() || value <= 0.0 || value > 1.0 {
            return Err(
                "recognition threshold must be a finite number greater than 0 and at most 1".into(),
            );
        }
        Ok(Self(value))
    }
    pub fn value(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionDecision {
    Recognized,
    NotRecognized,
    Inconclusive,
}

#[derive(Clone, Copy)]
pub enum TrialLabel {
    SavedPerson,
    OtherPerson,
}

pub enum TrialOutcome {
    Decision(RecognitionDecision),
    AcquisitionFailed,
    Cancelled,
}

#[derive(Default, serde::Serialize)]
pub struct TrialCounts {
    recognized: usize,
    not_recognized: usize,
    inconclusive: usize,
    acquisition_failed: usize,
}

/// Manual, user-labelled trials; aggregate counts only, never an accuracy claim.
#[derive(Default)]
pub struct RecognitionTrials {
    context: Option<(u64, u32)>,
    saved_person: TrialCounts,
    other_person: TrialCounts,
    total: usize,
}

impl RecognitionTrials {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn validate(
        &self,
        reference: &SessionReference,
        threshold: RecognitionThreshold,
    ) -> Result<(), Error> {
        if self.total >= 1000 {
            return Err("trial limit reached; clear trials to start again".into());
        }
        if self
            .context
            .is_some_and(|context| context != (reference.revision, threshold.value().to_bits()))
        {
            return Err(
                "reference or threshold changed; clear trials before starting a new set".into(),
            );
        }
        Ok(())
    }

    /// Cancellation never changes counters or establishes a trial context.
    pub fn record(
        &mut self,
        reference: &SessionReference,
        threshold: RecognitionThreshold,
        label: TrialLabel,
        outcome: TrialOutcome,
    ) -> Result<(), Error> {
        if matches!(outcome, TrialOutcome::Cancelled) {
            return Ok(());
        }
        self.validate(reference, threshold)?;
        if !reference.template_available() || reference.reference_sample_count() < REFERENCE_SAMPLES
        {
            return Err("an eight-sample reference is required for trials".into());
        }
        self.context = Some((reference.revision, threshold.value().to_bits()));
        let counts = match label {
            TrialLabel::SavedPerson => &mut self.saved_person,
            TrialLabel::OtherPerson => &mut self.other_person,
        };
        match outcome {
            TrialOutcome::Decision(RecognitionDecision::Recognized) => counts.recognized += 1,
            TrialOutcome::Decision(RecognitionDecision::NotRecognized) => {
                counts.not_recognized += 1
            }
            TrialOutcome::Decision(RecognitionDecision::Inconclusive) => counts.inconclusive += 1,
            TrialOutcome::AcquisitionFailed => counts.acquisition_failed += 1,
            TrialOutcome::Cancelled => unreachable!("cancelled trials returned before mutation"),
        }
        self.total += 1;
        Ok(())
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"saved_person": self.saved_person, "other_person": self.other_person,
            "total_trials": self.total, "threshold": self.context.map(|(_, bits)| f32::from_bits(bits)),
            "labels_user_declared": true, "calibrated_accuracy": false, "authentication_ready": false})
    }

    pub fn message(&self) -> String {
        let a = &self.saved_person;
        let b = &self.other_person;
        let threshold = self.context.map_or_else(
            || "unset".into(),
            |(_, bits)| format!("{:.3}", f32::from_bits(bits)),
        );
        format!(
            "Trials: {} · threshold {threshold}\nSaved person: {} recognized, {} not recognized, {} inconclusive, {} failed.\nOther person: {} recognized, {} not recognized, {} inconclusive, {} failed.",
            self.total,
            a.recognized,
            a.not_recognized,
            a.inconclusive,
            a.acquisition_failed,
            b.recognized,
            b.not_recognized,
            b.inconclusive,
            b.acquisition_failed
        )
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum VerificationProtocol {
    ComparisonOnly,
    MeasuredMotion,
    PassiveFront,
}

/// Constructed only after three complete, validated acquisitions.
#[derive(Debug, serde::Serialize)]
pub struct ComparisonSummary {
    samples: usize,
    capture_attempts: u64,
    transition_identity_retries: u64,
    verification_evidence: Option<hello_face::verification::VerificationEvidence>,
    failure_reason: Option<&'static str>,
    cosine_min: f32,
    cosine_median: f32,
    cosine_max: f32,
    strict_identity: bool,
    verification_protocol: VerificationProtocol,
    motion_challenge_passed: bool,
    pad_validated: bool,
    authentication_ready: bool,
    raw_frames_saved: bool,
    embedding_saved: bool,
}

impl ComparisonSummary {
    pub fn failure_reason(&self) -> Option<&'static str> {
        self.failure_reason
    }
    pub fn minimum(&self) -> f32 {
        self.cosine_min
    }
    pub fn median(&self) -> f32 {
        self.cosine_median
    }
    pub fn maximum(&self) -> f32 {
        self.cosine_max
    }
    pub fn classify(&self, threshold: RecognitionThreshold) -> RecognitionDecision {
        let protocol_complete = match self.verification_protocol {
            VerificationProtocol::ComparisonOnly => !self.strict_identity,
            VerificationProtocol::MeasuredMotion => {
                self.motion_challenge_passed && self.samples == 5
            }
            VerificationProtocol::PassiveFront => self.samples == 3,
        };
        if self.strict_identity && (!protocol_complete || self.cosine_min < STRICT_THRESHOLD_FLOOR)
        {
            return RecognitionDecision::NotRecognized;
        }
        if self.cosine_min >= threshold.value() {
            RecognitionDecision::Recognized
        } else if self.strict_identity || self.cosine_max < threshold.value() {
            RecognitionDecision::NotRecognized
        } else {
            RecognitionDecision::Inconclusive
        }
    }
}

/// Three fresh quality-checked comparisons. No partial summary, identity threshold,
/// or biometric vectors are returned; acquisition errors and cancellation abort.
pub async fn compare_three<F, Fut>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    mut capture: F,
) -> Result<ComparisonSummary, Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
{
    let check_cancel = || -> Result<(), Error> {
        if cancel.load(Ordering::Acquire) {
            Err("face comparison cancelled".into())
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    reference
        .lock()
        .map_err(|_| "reference unavailable")?
        .validate_comparison(identity, ir)?;
    let mut scores = Vec::with_capacity(3);
    let mut digests = Vec::with_capacity(3);
    let mut attempts = 0;
    for _ in 0..3 {
        check_cancel()?;
        let probe = capture().await?;
        check_cancel()?;
        if probe.faces != Some(1) || probe.report["diagnostic_passed"] != true {
            return Err("one suitable face is required for each comparison".into());
        }
        let quality = probe
            .sample_quality
            .ok_or("face detail could not be assessed")?;
        if let Some(reason) = quality.rejection(probe.confidence.unwrap_or(0.0)) {
            return Err(reason.into());
        }
        if digests.contains(&probe.frame_digest) {
            return Err("repeated frame rejected; three fresh captures are required".into());
        }
        let score = reference
            .lock()
            .map_err(|_| "reference unavailable")?
            .compare(identity, ir, &probe)?
            .ok_or("reference unavailable")?;
        if !score.is_finite() || !(-1.0..=1.0).contains(&score) {
            return Err("invalid comparison score".into());
        }
        scores.push(score);
        digests.push(probe.frame_digest);
        attempts += probe.report["sample_capture_attempts"]
            .as_u64()
            .unwrap_or(1);
    }
    check_cancel()?;
    scores.sort_by(f32::total_cmp);
    Ok(ComparisonSummary {
        samples: 3,
        capture_attempts: attempts,
        transition_identity_retries: 0,
        verification_evidence: None,
        failure_reason: None,
        cosine_min: scores[0],
        cosine_median: scores[1],
        cosine_max: scores[2],
        strict_identity: false,
        verification_protocol: VerificationProtocol::ComparisonOnly,
        motion_challenge_passed: false,
        pad_validated: false,
        authentication_ready: false,
        raw_frames_saved: false,
        embedding_saved: false,
    })
}

/// A randomized measured head-motion check. Not evaluated PAD; replay remains possible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionStep {
    Front,
    TurnImageLeft,
    ReturnFront,
    TurnImageRight,
    FinishFront,
}
impl MotionStep {
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Front => "Patrz prosto — ustalam pozycję bazową (1/5)",
            Self::TurnImageLeft => "Lekko skręć głowę w stronę lewej krawędzi podglądu",
            Self::TurnImageRight => "Lekko skręć głowę w stronę prawej krawędzi podglądu",
            Self::ReturnFront => "Wróć do patrzenia prosto (3/5)",
            Self::FinishFront => "Ponownie patrz prosto — kończę sprawdzanie (5/5)",
        }
    }
    fn reached(self, yaw: f32, baseline: f32) -> bool {
        if !yaw.is_finite() || !baseline.is_finite() {
            return false;
        }
        match self {
            Self::Front => true,
            Self::TurnImageLeft => (-0.45..=-0.06).contains(&(yaw - baseline)),
            Self::TurnImageRight => (0.06..=0.45).contains(&(yaw - baseline)),
            Self::ReturnFront | Self::FinishFront => (yaw - baseline).abs() <= 0.05,
        }
    }
}

/// UI-only measured motion feedback. Not identity confidence or PAD evidence.
pub struct MotionProgress {
    pub step: MotionStep,
    pub percent: u8,
    pub hint: String,
    pub quality_rejected: bool,
}
impl MotionStep {
    fn progress(self, yaw: f32, baseline: f32) -> MotionProgress {
        let delta = yaw - baseline;
        let signed = match self {
            Self::TurnImageLeft => -delta,
            _ => delta,
        };
        let (percent, hint) = match self {
            Self::TurnImageLeft | Self::TurnImageRight => {
                let percent = (signed / 0.06 * 100.0).clamp(0.0, 100.0) as u8;
                let hint = if signed < -0.02 {
                    "Skręcasz w przeciwną stronę — kieruj się krawędzią podglądu wskazaną w poleceniu."
                } else if signed > 0.45 {
                    "Skręt jest za duży. Obróć głowę trochę z powrotem; oba oczy powinny być widoczne."
                } else if percent >= 100 {
                    "Ruch zmierzony. Przytrzymaj tę pozycję."
                } else {
                    "Wystarczy mały skręt głowy. Nie przechylaj jej ani nie przesuwaj całego ciała."
                };
                (percent, hint)
            }
            Self::Front => (
                0,
                "Ustalam Twoją naturalną pozycję. Przytrzymaj głowę; na tym etapie nie wykonuj skrętu.",
            ),
            _ => (
                if self.reached(yaw, baseline) { 100 } else { 0 },
                "Wróć do pozycji początkowej i przytrzymaj głowę.",
            ),
        };
        MotionProgress {
            step: self,
            percent,
            hint: hint.into(),
            quality_rejected: false,
        }
    }
}
fn quality_hint(reason: &str) -> String {
    match reason {
        "face exposure is too dark; adjust lighting or IR mode" => "Twarz jest za ciemna. Poczekaj na ekspozycję; ustaw twarz bliżej środka podglądu.",
        "face exposure is too bright; adjust lighting or IR mode" => "Twarz jest za jasna. Odsuń się trochę od kamery lub mocnego światła.",
        "face exposure contains too many clipped pixels; adjust lighting or IR distance" => "Część twarzy jest prześwietlona lub bardzo ciemna. Odsuń się trochę i zachowaj oba oczy w kadrze.",
        "move closer so the camera sees more detail in your face" => "Twarz jest za mała w obrazie. Przysuń się do kamery, zachowując całą twarz w kadrze.",
        "move back or toward the center; keep the whole face inside the image" => "Twarz wychodzi poza kadr lub jest za blisko. Odsuń się trochę i ustaw ją pośrodku podglądu.",
        "face detail is too soft; hold still and check the lens" => "Brakuje szczegółów twarzy. Przytrzymaj głowę nieruchomo i sprawdź czystość obiektywu.",
        _ => "Ujęcie chwilowo nieczytelne. Przytrzymaj głowę, pozostaw oba oczy w kadrze — ponawiam próbę.",
    }.into()
}

/// Two owner-validated observations may bridge a short unusable-frame gap.
#[derive(Default)]
struct BaselineWindow {
    candidate: Option<(f32, Instant)>,
}
impl BaselineWindow {
    fn observe(&mut self, yaw: f32, now: Instant) -> Option<f32> {
        if let Some((prior, at)) = self.candidate
            && now.duration_since(at) <= Duration::from_secs(5)
            && (yaw - prior).abs() <= 0.05
        {
            self.candidate = None;
            return Some((prior + yaw) * 0.5);
        }
        self.candidate = Some((yaw, now));
        None
    }
}

/// Strict owner comparison: five measured poses, per-exemplar features,
/// shape gate, duplicate rejection, and no positive result on any failed gate.
pub async fn recognize_person<F, Fut>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    capture: F,
) -> Result<ComparisonSummary, Error>
where
    F: FnMut(MotionStep) -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
{
    recognize_person_observed(reference, identity, ir, cancel, capture, |_| {}).await
}

pub async fn recognize_person_observed<F, Fut, P>(
    reference: &std::sync::Mutex<SessionReference>,
    identity: &DeviceIdentity,
    ir: bool,
    cancel: &AtomicBool,
    mut capture: F,
    mut progress: P,
) -> Result<ComparisonSummary, Error>
where
    F: FnMut(MotionStep) -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
    P: FnMut(MotionProgress),
{
    let check_cancel = || -> Result<(), Error> {
        if cancel.load(Ordering::Acquire) {
            Err("Rozpoznawanie anulowane".into())
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let expected_revision = {
        let locked = reference.lock().map_err(|_| "reference unavailable")?;
        locked.validate_strict_recognition(identity, ir)?;
        locked.revision
    };
    // OS entropy, generated per attempt before camera acquisition; no time-based pattern.
    use std::io::Read;
    let mut random = [0_u8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let steps = motion_steps(random[0] & 1 != 0);
    let mut baseline = 0.0;
    let mut baseline_window = BaselineWindow::default();
    let mut identity_minimum = f32::INFINITY;
    let mut scores = Vec::with_capacity(5);
    let mut digests = Vec::with_capacity(5 * POSE_ATTEMPTS);
    let mut attempts = 0;
    let mut transition_identity_retries = 0;
    let mut minimum_evidence: Option<hello_face::verification::VerificationEvidence> = None;
    for step in steps {
        let turning = matches!(step, MotionStep::TurnImageLeft | MotionStep::TurnImageRight);
        let mut weak_turn_frames = 0;
        let mut turn_window = BaselineWindow::default();
        let mut reached = false;
        let mut last_hint = "Nie potwierdzono wymaganego ruchu".to_owned();
        let deadline = Instant::now() + POSE_TIME;
        for _ in 0..POSE_ATTEMPTS {
            check_cancel()?;
            if Instant::now() >= deadline {
                break;
            }
            let probe = match capture(step).await {
                Ok(probe) => probe,
                Err(error) if error.is::<TransientCapture>() => {
                    check_cancel()?;
                    turn_window = BaselineWindow::default();
                    last_hint = format!("{error} Nadal czekam na czytelne ujęcie.");
                    progress(MotionProgress {
                        step,
                        percent: 0,
                        hint: last_hint.clone(),
                        quality_rejected: true,
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            check_cancel()?;
            if digests.contains(&probe.frame_digest) {
                return Err("Powtórzona klatka: kontrola ruchu przerwana".into());
            }
            digests.push(probe.frame_digest);
            attempts += probe.report["sample_capture_attempts"]
                .as_u64()
                .unwrap_or(1);
            if probe.faces.is_some_and(|n| n > 1) {
                return Err("Wymagana dokładnie jedna twarz".into());
            }
            // Retry only acquisition/quality rejection. Never retry a worker/protocol error
            // or reinterpret low identity evidence as an exposure problem.
            let quality_reason = probe
                .sample_quality
                .and_then(|q| q.rejection(probe.confidence.unwrap_or(0.0)));
            if let Some(reason) = quality_reason {
                turn_window = BaselineWindow::default();
                last_hint = quality_hint(reason);
                progress(MotionProgress {
                    step,
                    percent: 0,
                    hint: last_hint.clone(),
                    quality_rejected: true,
                });
                continue;
            }
            if probe.faces != Some(1) || probe.report["diagnostic_passed"] != true {
                let rejection = probe.report["rejection"].as_str().unwrap_or("");
                if probe.faces == Some(0)
                    || rejection == "sample quality is insufficient"
                    || rejection == "exactly one face is required"
                {
                    turn_window = BaselineWindow::default();
                    last_hint="Nie widzę wyraźnej twarzy. Zmniejsz skręt, pokaż oba oczy — ponawiam ujęcie.".into();
                    progress(MotionProgress {
                        step,
                        percent: 0,
                        hint: last_hint.clone(),
                        quality_rejected: true,
                    });
                    continue;
                }
                return Err("Nieprawidłowy wynik modelu; kontrola przerwana".into());
            }
            if probe.sample_quality.is_none() {
                return Err("Nie można ocenić jakości twarzy".into());
            }
            let geometry = probe
                .geometry
                .as_ref()
                .filter(|g| g.valid())
                .ok_or("Brak prawidłowej geometrii twarzy")?;
            let feedback = step.progress(geometry.yaw(), baseline);
            last_hint = feedback.hint.clone();
            progress(feedback);
            // Identity is checked on EVERY acquisition, including incorrect poses.
            let evidence = {
                let locked = reference.lock().map_err(|_| "reference unavailable")?;
                locked.validate_strict_recognition(identity, ir)?;
                if locked.revision != expected_revision {
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
            let score = evidence.score;
            if !score.is_finite() {
                return Err("Nieprawidłowy wynik porównania".into());
            }
            // During a turn, allow only two unusable identity observations in
            // total. They never count as owner evidence or complete a stage.
            if score < STRICT_THRESHOLD_FLOOR {
                if turning && weak_turn_frames < 2 {
                    weak_turn_frames += 1;
                    transition_identity_retries += 1;
                    turn_window = BaselineWindow::default();
                    last_hint = "Ujęcie podczas ruchu nie potwierdza twarzy. Zatrzymaj niewielki skręt i pokaż oba oczy — ponawiam.".into();
                    progress(MotionProgress {
                        step,
                        percent: 0,
                        hint: last_hint.clone(),
                        quality_rejected: true,
                    });
                    continue;
                }
                let mut summary = strict_summary(vec![score], attempts, false);
                summary.transition_identity_retries = transition_identity_retries;
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
            identity_minimum = identity_minimum.min(score);
            if minimum_evidence.is_none_or(|prior| score < prior.score) {
                minimum_evidence = Some(evidence);
            }
            if step == MotionStep::Front {
                let yaw = geometry.yaw();
                if let Some(measured_baseline) = baseline_window.observe(yaw, Instant::now()) {
                    // Calibrate the actual owner's offset; five-point IR landmarks
                    // do not establish a universal zero-yaw frontal position.
                    baseline = measured_baseline;
                    progress(MotionProgress {step,percent:0,quality_rejected:false,
                        hint:"Pozycja bazowa ustalona z dwóch stabilnych ujęć. Za chwilę pokażę kierunek skrętu.".into()});
                } else {
                    last_hint =
                        "Pozycja bazowa: 1/2 ujęć. Przytrzymaj głowę nieruchomo, pokaż oba oczy."
                            .into();
                    progress(MotionProgress {
                        step,
                        percent: 0,
                        quality_rejected: false,
                        hint: last_hint.clone(),
                    });
                    continue;
                }
            } else if !step.reached(geometry.yaw(), baseline) {
                turn_window = BaselineWindow::default();
                continue;
            } else if turning
                && turn_window
                    .observe(geometry.yaw(), Instant::now())
                    .is_none()
            {
                last_hint = "Skręt zmierzony: 1/2 stabilnych ujęć. Przytrzymaj lekki skręt; nie obracaj głowy dalej.".into();
                progress(MotionProgress {
                    step,
                    percent: 100,
                    hint: last_hint.clone(),
                    quality_rejected: false,
                });
                continue;
            }
            scores.push(score);
            reached = true;
            break;
        }
        if !reached {
            return Err(format!(
                "Nie zakończono etapu: {}. {last_hint} Spróbuj ponownie.",
                step.instruction()
            )
            .into());
        }
    }
    check_cancel()?;
    let mut summary = strict_summary(scores, attempts, true);
    // Include all usable identity observations, including baseline and pose retries.
    summary.cosine_min = summary.cosine_min.min(identity_minimum);
    summary.transition_identity_retries = transition_identity_retries;
    summary.verification_evidence = minimum_evidence;
    Ok(summary)
}
fn motion_steps(right_first: bool) -> [MotionStep; 5] {
    use MotionStep::*;
    if right_first {
        [
            Front,
            TurnImageRight,
            ReturnFront,
            TurnImageLeft,
            FinishFront,
        ]
    } else {
        [
            Front,
            TurnImageLeft,
            ReturnFront,
            TurnImageRight,
            FinishFront,
        ]
    }
}
fn strict_summary(mut scores: Vec<f32>, attempts: u64, motion_passed: bool) -> ComparisonSummary {
    scores.sort_by(f32::total_cmp);
    ComparisonSummary {
        samples: scores.len(),
        capture_attempts: attempts,
        transition_identity_retries: 0,
        verification_evidence: None,
        failure_reason: None,
        cosine_min: scores[0],
        cosine_median: scores[scores.len() / 2],
        cosine_max: *scores.last().expect("nonempty"),
        strict_identity: true,
        verification_protocol: VerificationProtocol::MeasuredMotion,
        motion_challenge_passed: motion_passed,
        pad_validated: false,
        authentication_ready: false,
        raw_frames_saved: false,
        embedding_saved: false,
    }
}

pub async fn face_probe(
    executable: PathBuf,
    path: PathBuf,
    models: PathBuf,
    ir_emitter: bool,
    expected: Option<DeviceIdentity>,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value, Error> {
    Ok(
        inspect_face(executable, path, models, ir_emitter, expected, cancel)
            .await?
            .report,
    )
}

/// Retry up to three fresh bounded acquisitions for a sample-quality failure.
/// Each attempt releases the camera, restores IR and reaps its isolated worker.
pub async fn inspect_sample(
    executable: PathBuf,
    path: PathBuf,
    models: PathBuf,
    ir_emitter: bool,
    expected: Option<DeviceIdentity>,
    cancel: Arc<AtomicBool>,
) -> Result<FaceProbe, Error> {
    inspect_sample_observed(executable, path, models, ir_emitter, expected, cancel, None).await
}

/// Optional parent-side in-memory display of acquired frames; never part of metadata.
pub type FrameObserver = Arc<dyn Fn(&hello_camera::Frame) + Send + Sync>;

pub async fn inspect_sample_observed(
    executable: PathBuf,
    path: PathBuf,
    models: PathBuf,
    ir_emitter: bool,
    expected: Option<DeviceIdentity>,
    cancel: Arc<AtomicBool>,
    observer: Option<FrameObserver>,
) -> Result<FaceProbe, Error> {
    retry_samples(|| {
        inspect_face_observed(
            executable.clone(),
            path.clone(),
            models.clone(),
            ir_emitter,
            expected.clone(),
            cancel.clone(),
            observer.clone(),
        )
    })
    .await
}

async fn retry_samples<F, Fut>(mut capture: F) -> Result<FaceProbe, Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<FaceProbe, Error>>,
{
    let mut last = None;
    for attempt in 1..=3 {
        let mut probe = capture().await?;
        probe.report["sample_capture_attempts"] = attempt.into();
        let acceptable = probe.embedding.is_some()
            && probe.sample_quality.is_some_and(|quality| {
                quality.rejection(probe.confidence.unwrap_or(0.0)).is_none()
            });
        if acceptable {
            return Ok(probe);
        }
        last = Some(probe);
    }
    Ok(last.expect("three completed acquisitions"))
}

pub async fn inspect_face(
    executable: PathBuf,
    path: PathBuf,
    models: PathBuf,
    ir_emitter: bool,
    expected: Option<DeviceIdentity>,
    cancel: Arc<AtomicBool>,
) -> Result<FaceProbe, Error> {
    inspect_face_observed(executable, path, models, ir_emitter, expected, cancel, None).await
}

/// Wait for startup AND three consecutive acceptable, stable global exposures.
/// Does not relax the separate face-ROI quality gate after inference.
struct ExposureSettling {
    minimum_frames: u32,
    previous: Option<hello_face::QualityMetrics>,
    stable: u8,
}
impl ExposureSettling {
    fn new(ir: bool) -> Self {
        Self {
            minimum_frames: if ir { 18 } else { 8 },
            previous: None,
            stable: 0,
        }
    }
    fn ready(&mut self, index: u32, quality: hello_face::QualityMetrics) -> bool {
        if index < self.minimum_frames || !quality.acceptable() {
            self.stable = 0;
            self.previous = None;
            return false;
        }
        let unchanged = self.previous.is_some_and(|p| {
            (p.mean_intensity - quality.mean_intensity).abs() <= 0.025
                && (p.contrast - quality.contrast).abs() <= 0.02
        });
        self.stable = if unchanged {
            self.stable.saturating_add(1)
        } else {
            1
        };
        self.previous = Some(quality);
        self.stable >= 3
    }
}

pub async fn inspect_face_observed(
    executable: PathBuf,
    path: PathBuf,
    models: PathBuf,
    ir_emitter: bool,
    expected: Option<DeviceIdentity>,
    cancel: Arc<AtomicBool>,
    observer: Option<FrameObserver>,
) -> Result<FaceProbe, Error> {
    if cancel.load(Ordering::Acquire) {
        return Err("face check cancelled".into());
    }
    // Spawn on this persistent calling thread for Linux parent-death semantics.
    let worker = Worker::spawn(&executable, &models, cancel.clone())?;
    let mut initialization = tokio::task::spawn_blocking(move || worker.initialize());
    let worker = tokio::select! {
        biased;
        _ = cancelled(&cancel) => {
            cancel.store(true, Ordering::Release);
            let _ = initialization.await;
            return Err("face probe cancelled before camera access".into());
        },
        result = &mut initialization => result??,
    };
    let startup_ms = worker.startup_ms;
    let device = tokio::task::spawn_blocking(hello_camera::enumerate)
        .await??
        .into_iter()
        .find(|d| d.path == path)
        .ok_or("device was not found in V4L2 enumeration; use the exact reported path")?;
    if expected.is_some_and(|identity| identity != device.identity) {
        return Err(hello_camera::CameraError::DeviceChanged.into());
    }
    if cancel.load(Ordering::Acquire) {
        return Err("face check cancelled".into());
    }
    let emitter = if ir_emitter {
        Some(hello_camera::IrEmitterLease::acquire(&device)?)
    } else {
        None
    };
    let mut config = CaptureConfig::preview(device)?;
    let candidate_frames = if ir_emitter { 90 } else { 30 };
    config.max_frames = candidate_frames;
    config.reconnect_attempts = 0;
    let mut session = CameraSession::start(config)?;
    let captured: Result<hello_camera::Frame, Error> = async {
        // Allow exposure to settle; IR samples on this host need more than four
        // frames. Retain a bounded candidate without changing quality thresholds.
        let mut last = None;
        let mut settling = ExposureSettling::new(ir_emitter);
        for index in 0..candidate_frames {
            let frame = tokio::select! {
                biased;
                _ = cancelled(&cancel) => {
                    return Err("face probe cancelled; camera released".into());
                },
                frame = session.next_frame() => frame?,
            };
            let Some(frame) = frame else {
                break;
            };
            if index == 0
                && let Some(emitter) = &emitter
            {
                emitter.rearm_streaming()?;
            }
            if let Some(observer) = &observer {
                observer(&frame);
            }
            let quality = frame_quality(&frame)?;
            if settling.ready(index, quality) {
                return Ok(frame);
            }
            last = Some(frame);
        }
        last.ok_or_else(|| "diagnostic frame unavailable".into())
    }
    .await;
    let stopped = session.stop().await;
    if let Some(emitter) = emitter {
        emitter.restore()?;
    }
    let captured = captured?;
    stopped?;
    // Only private pipes carry pixels; the closed camera stays in the parent.
    let width = captured.width;
    let height = captured.height;
    let rgb = zeroize::Zeroizing::new(captured.to_rgb()?);
    drop(captured);
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(width.to_le_bytes());
    digest.update(height.to_le_bytes());
    digest.update(&*rgb);
    let frame_digest = digest.finalize().into();
    let started = Instant::now();
    let mut inference = tokio::task::spawn_blocking(move || worker.infer(width, height, &rgb));
    let mut result = tokio::select! {
        biased;
        _ = cancelled(&cancel) => {
            cancel.store(true, Ordering::Release);
            let _ = inference.await;
            return Err("face probe cancelled; camera released and worker reaped".into());
        },
        result = &mut inference => result??,
    };
    let mut report = result.metadata();
    report["adapter"] = "OpenCV YuNet/SFace CPU diagnostic".into();
    report["detector_sha256"] = hello_face::opencv_engine::DETECTOR_SHA256.into();
    report["recognizer_sha256"] = hello_face::opencv_engine::RECOGNIZER_SHA256.into();
    report["width"] = width.into();
    report["height"] = height.into();
    report["startup_ms"] = serde_json::json!(startup_ms);
    report["supervised_inference_ms"] = serde_json::json!(started.elapsed().as_millis());
    report["worker_isolated"] = true.into();
    report["ir_face_mode_requested"] = ir_emitter.into();
    report["ir_face_mode_restored"] = ir_emitter.into();
    report["worker_limits"] = serde_json::json!({"address_space_mib": 1024, "cpu_seconds": 5,
        "phase_wall_seconds": 8, "core_dumps": false, "new_files_network_processes": false});
    Ok(FaceProbe {
        report,
        embedding: result.embedding.take(),
        frame_digest,
        quality: result.quality,
        faces: result.faces,
        sample_quality: result.sample_quality,
        confidence: result.confidence,
        geometry: result.geometry,
    })
}

pub fn frame_quality(captured: &hello_camera::Frame) -> Result<hello_face::QualityMetrics, Error> {
    use hello_face::{Frame, PixelFormat, QualityMetrics};
    let rgb;
    let frame = if captured.pixel_format == hello_camera::PixelFormat::Grey {
        Frame {
            width: captured.width as usize,
            height: captured.height as usize,
            stride: captured.stride as usize,
            format: PixelFormat::Gray8,
            pixels: &captured.data,
        }
    } else {
        rgb = zeroize::Zeroizing::new(captured.to_rgb()?);
        Frame {
            width: captured.width as usize,
            height: captured.height as usize,
            stride: captured.width as usize * 3,
            format: PixelFormat::Rgb8,
            pixels: &rgb,
        }
    };
    Ok(QualityMetrics::measure(&frame)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> DeviceIdentity {
        DeviceIdentity {
            sysfs_path: "/test/camera".into(),
            device_number: 1,
            driver: "test".into(),
            card: "test".into(),
            bus_info: "test".into(),
        }
    }
    fn geometric_probe(id: u8, yaw: f32, values: Vec<f32>) -> FaceProbe {
        let mut p = probe(values);
        p.frame_digest = [id; 32];
        p.geometry = Some(
            hello_face::geometry::FaceGeometry::measure(&hello_face::Detection {
                bounds: [0.0, 0.0, 200.0, 240.0],
                landmarks: [
                    [50.0, 60.0],
                    [150.0, 60.0],
                    [100.0 + yaw * 100.0, 120.0],
                    [65.0, 180.0],
                    [135.0, 180.0],
                ],
                confidence: 0.99,
            })
            .unwrap(),
        );
        p
    }
    fn strict_reference() -> std::sync::Mutex<SessionReference> {
        let mut r = SessionReference::default();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            r.add_sample(&identity(), true, geometric_probe(id, 0.0, vec![1.0, 0.0]))
                .unwrap();
        }
        std::sync::Mutex::new(r)
    }
    fn step_yaw(step: MotionStep) -> f32 {
        match step {
            MotionStep::TurnImageLeft => -0.16,
            MotionStep::TurnImageRight => 0.16,
            _ => 0.0,
        }
    }
    #[test]
    fn baseline_pair_expires_and_still_requires_stable_geometry() {
        let now = Instant::now();
        let mut baseline = BaselineWindow::default();
        assert!(baseline.observe(0.24, now).is_none());
        assert!(
            (baseline
                .observe(0.26, now + Duration::from_secs(5))
                .unwrap()
                - 0.25)
                .abs()
                < 0.001
        );
        assert!(baseline.observe(0.24, now).is_none());
        assert!(
            baseline
                .observe(0.24, now + Duration::from_secs(6))
                .is_none()
        );
        assert!(
            baseline
                .observe(-0.24, now + Duration::from_secs(7))
                .is_none()
        );
        assert!(
            (baseline
                .observe(-0.22, now + Duration::from_secs(8))
                .unwrap()
                + 0.23)
                .abs()
                < 0.001
        );
    }

    #[tokio::test]
    async fn first_stage_keeps_owner_sample_across_short_quality_and_darkness_gaps() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let result = if calls == 3 {
                Err(TransientCapture("Ciemna klatka").into())
            } else {
                let mut p = geometric_probe(40 + calls, step_yaw(step), vec![1.0, 0.0]);
                if calls == 2 {
                    p.sample_quality
                        .as_mut()
                        .unwrap()
                        .face_quality
                        .mean_intensity = 0.95;
                }
                if calls == 4 {
                    p.faces = Some(0);
                    p.report["diagnostic_passed"] = false.into();
                    p.sample_quality = None;
                    p.geometry = None;
                    p.embedding = None;
                }
                Ok(p)
            };
            std::future::ready(result)
        })
        .await
        .unwrap();
        assert_eq!(calls, 11); // two baseline frames, three gaps, and two observations per turn
        assert!(result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
    }

    #[tokio::test]
    async fn baseline_gap_does_not_allow_a_different_person_to_supply_second_frame() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let result = if calls == 2 {
                Err(TransientCapture("Ciemna klatka").into())
            } else {
                Ok(geometric_probe(
                    40 + calls,
                    step_yaw(step),
                    if calls == 1 {
                        vec![1.0, 0.0]
                    } else {
                        vec![0.7, 0.71414286]
                    },
                ))
            };
            std::future::ready(result)
        })
        .await
        .unwrap();
        assert_eq!(calls, 3);
        assert!(!result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
    }

    #[tokio::test]
    async fn passive_verification_accepts_three_owner_frames_without_claiming_motion_or_pad() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = verify_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            || {
                calls += 1;
                std::future::ready(Ok(geometric_probe(40 + calls, 0.24, vec![1.0, 0.0])))
            },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(result.samples, 3);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
        assert!(!result.motion_challenge_passed);
        assert!(!result.pad_validated);
        assert!(!result.authentication_ready);
        assert_eq!(
            serde_json::to_value(result).unwrap()["verification_protocol"],
            "passive_front"
        );
    }
    #[tokio::test]
    async fn passive_verification_rejects_an_unknown_frame_instead_of_median_voting() {
        for weak_at in [1, 3] {
            let reference = strict_reference();
            let cancel = AtomicBool::new(false);
            let mut calls = 0;
            let result = verify_person_observed(
                &reference,
                &identity(),
                true,
                &cancel,
                || {
                    calls += 1;
                    std::future::ready(Ok(geometric_probe(
                        40 + calls,
                        0.0,
                        if calls == weak_at {
                            vec![0.7, 0.71414286]
                        } else {
                            vec![1.0, 0.0]
                        },
                    )))
                },
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(calls, weak_at);
            assert_eq!(
                result.classify(RecognitionThreshold::new(0.8).unwrap()),
                RecognitionDecision::NotRecognized
            );
        }
        let mut partial = strict_summary(vec![1.0, 1.0], 2, false);
        partial.verification_protocol = VerificationProtocol::PassiveFront;
        assert_eq!(
            partial.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
    }
    #[tokio::test]
    async fn passive_quality_recovery_is_bounded_and_duplicate_frames_still_fail() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = verify_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            || {
                calls += 1;
                let mut p = geometric_probe(40 + calls, 0.0, vec![1.0, 0.0]);
                if calls <= 8 {
                    p.sample_quality
                        .as_mut()
                        .unwrap()
                        .face_quality
                        .mean_intensity = 0.95;
                }
                std::future::ready(Ok(p))
            },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(calls, 11);
        assert_eq!(result.samples, 3);
        let mut calls = 0;
        let error = verify_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            || {
                calls += 1;
                let mut p = geometric_probe(40 + calls, 0.0, vec![1.0, 0.0]);
                p.faces = Some(0);
                p.sample_quality = None;
                p.report["diagnostic_passed"] = false.into();
                std::future::ready(Ok(p))
            },
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(calls, 16);
        assert!(error.to_string().contains("0/3"));
        let error = verify_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            || std::future::ready(Ok(geometric_probe(40, 0.0, vec![1.0, 0.0]))),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Powtórzona"));
    }
    #[tokio::test]
    async fn passive_cancellation_reference_change_multiple_faces_and_worker_errors_fail_closed() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        assert!(
            verify_person_observed(
                &reference,
                &identity(),
                true,
                &cancel,
                || {
                    let mut p = geometric_probe(40, 0.0, vec![1.0, 0.0]);
                    p.faces = Some(2);
                    std::future::ready(Ok(p))
                },
                |_| {}
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("jedna twarz")
        );
        let mut calls = 0;
        assert!(
            verify_person_observed(
                &reference,
                &identity(),
                true,
                &cancel,
                || {
                    calls += 1;
                    std::future::ready(Err("invalid worker reply".into()))
                },
                |_| {}
            )
            .await
            .is_err()
        );
        assert_eq!(calls, 1);
        assert!(
            verify_person_observed(
                &reference,
                &identity(),
                true,
                &cancel,
                || {
                    reference.lock().unwrap().rename("Changed").unwrap();
                    std::future::ready(Ok(geometric_probe(40, 0.0, vec![1.0, 0.0])))
                },
                |_| {}
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("Wzorzec zmienił")
        );
        assert!(
            verify_person_observed(
                &reference,
                &identity(),
                true,
                &cancel,
                || {
                    cancel.store(true, Ordering::Release);
                    std::future::ready(Ok(geometric_probe(40, 0.0, vec![1.0, 0.0])))
                },
                |_| {}
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("anulowane")
        );
    }

    #[tokio::test]
    async fn passive_final_progress_cannot_publish_success_after_stop() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = verify_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            || {
                calls += 1;
                std::future::ready(Ok(geometric_probe(40 + calls, 0.0, vec![1.0, 0.0])))
            },
            |feedback| {
                if feedback.accepted == 3 {
                    cancel.store(true, Ordering::Release);
                }
            },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("anulowane"));
    }

    #[tokio::test]
    async fn baseline_uses_two_stable_owner_observations_with_nonzero_landmark_offset() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let yaw = if calls == 1 {
                0.24
            } else if calls == 2 {
                0.26
            } else {
                0.25 + step_yaw(step)
            };
            std::future::ready(Ok(geometric_probe(40 + calls, yaw, vec![1.0, 0.0])))
        })
        .await
        .unwrap();
        assert_eq!(calls, 8);
        assert_eq!(result.samples, 5);
        assert_eq!(result.capture_attempts, 8);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
    }
    #[tokio::test]
    async fn unstable_baseline_never_starts_turns_or_reports_recognition() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            assert_eq!(step, MotionStep::Front);
            calls += 1;
            std::future::ready(Ok(geometric_probe(
                40 + calls,
                if calls % 2 == 0 { 0.24 } else { -0.24 },
                vec![1.0, 0.0],
            )))
        })
        .await;
        assert_eq!(calls, POSE_ATTEMPTS as u8);
        assert!(result.unwrap_err().to_string().contains("Pozycja bazowa"));
    }
    #[tokio::test]
    async fn baseline_observations_are_included_in_stricter_user_threshold() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let values = if calls == 1 {
                vec![0.85, 0.5267827]
            } else {
                vec![1.0, 0.0]
            };
            std::future::ready(Ok(geometric_probe(40 + calls, step_yaw(step), values)))
        })
        .await
        .unwrap();
        assert!(result.minimum() < 0.9);
        assert_eq!(
            result.verification_evidence.unwrap().score,
            result.minimum()
        );
        assert!(result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.9).unwrap()),
            RecognitionDecision::NotRecognized
        );
    }
    #[test]
    fn exposure_waits_for_ir_warmup_and_consecutive_stable_frames() {
        let q = hello_face::QualityMetrics {
            mean_intensity: 0.5,
            contrast: 0.1,
            edge_energy: 0.1,
        };
        let mut gate = ExposureSettling::new(true);
        for index in 0..20 {
            assert!(!gate.ready(index, q));
        }
        assert!(gate.ready(20, q));
        let mut changed = q;
        changed.mean_intensity = 0.6;
        assert!(!gate.ready(21, changed));
        assert!(!gate.ready(22, changed));
        assert!(gate.ready(23, changed));
        changed.mean_intensity = 0.99;
        assert!(!gate.ready(24, changed));
        assert!(!gate.ready(25, q));
        assert!(!gate.ready(26, q));
        assert!(gate.ready(27, q));
    }
    #[test]
    fn small_measured_turns_pass_but_static_wrong_direction_and_extreme_turns_do_not() {
        assert!(MotionStep::TurnImageLeft.reached(-0.065, 0.0));
        assert!(MotionStep::TurnImageRight.reached(0.065, 0.0));
        assert!(!MotionStep::TurnImageLeft.reached(0.065, 0.0));
        assert!(!MotionStep::TurnImageLeft.reached(0.0, 0.0));
        assert!(!MotionStep::TurnImageLeft.reached(-0.5, 0.0));
        let feedback = MotionStep::TurnImageLeft.progress(0.08, 0.0);
        assert_eq!(feedback.percent, 0);
        assert!(feedback.hint.contains("przeciwną"));
    }
    #[tokio::test]
    async fn recognition_waits_through_eight_bad_frames_then_completes_all_poses() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let mut p = geometric_probe(40 + calls, step_yaw(step), vec![1.0, 0.0]);
            if calls <= 8 {
                p.sample_quality
                    .as_mut()
                    .unwrap()
                    .face_quality
                    .mean_intensity = 0.95;
            }
            std::future::ready(Ok(p))
        })
        .await
        .unwrap();
        assert_eq!(calls, 16);
        assert!(result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
    }

    #[tokio::test]
    async fn transient_darkness_retries_but_protocol_error_still_stops_immediately() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let result = if calls <= 8 {
                Err(TransientCapture("Ciemna klatka").into())
            } else {
                Ok(geometric_probe(40 + calls, step_yaw(step), vec![1.0, 0.0]))
            };
            std::future::ready(result)
        })
        .await
        .unwrap();
        assert!(result.motion_challenge_passed);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |_| {
            calls += 1;
            std::future::ready(Err("invalid worker packet".into()))
        })
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid worker packet")
        );
        assert_eq!(calls, 1);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |_| {
            calls += 1;
            std::future::ready(Err(TransientCapture("Ciemna klatka").into()))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, POSE_ATTEMPTS);
    }

    #[tokio::test]
    async fn enrollment_retries_darkness_and_bad_quality_without_losing_accepted_samples() {
        let mut r = SessionReference::default();
        r.begin_anonymous_enrollment(&identity(), true).unwrap();
        let reference = std::sync::Mutex::new(r);
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = collect_reference(
            &reference,
            &identity(),
            true,
            &cancel,
            |count| {
                calls += 1;
                let result = if calls <= 2 {
                    Err(TransientCapture("Ciemna klatka").into())
                } else {
                    let mut p = sample(20 + count as u8);
                    if calls <= 4 {
                        p.sample_quality
                            .as_mut()
                            .unwrap()
                            .face_quality
                            .mean_intensity = 0.95;
                    }
                    Ok(p)
                };
                std::future::ready(result)
            },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(result, 8);
        assert_eq!(calls, 12);
        assert!(reference.lock().unwrap().template_available());
    }

    #[tokio::test]
    async fn transient_exposure_retries_same_pose_without_weakening_identity_gate() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut id = 40;
        let mut feedbacks = Vec::new();
        let result = recognize_person_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            |step| {
                id += 1;
                let mut p = geometric_probe(id, step_yaw(step), vec![1.0, 0.0]);
                if id == 43 {
                    p.sample_quality
                        .as_mut()
                        .unwrap()
                        .face_quality
                        .mean_intensity = 0.95;
                }
                if id == 44 {
                    p.sample_quality
                        .as_mut()
                        .unwrap()
                        .face_quality
                        .mean_intensity = 0.05;
                }
                std::future::ready(Ok(p))
            },
            |feedback| feedbacks.push(feedback.hint),
        )
        .await
        .unwrap();
        assert_eq!(id, 50);
        assert_eq!(result.samples, 5);
        assert_eq!(result.capture_attempts, 10);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
        assert!(feedbacks.iter().any(|s| s.contains("za jasna")));
        assert!(feedbacks.iter().any(|s| s.contains("za ciemna")));
    }
    #[tokio::test]
    async fn persistent_bad_exposure_stops_bounded_with_useful_reason() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut id = 40;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            id += 1;
            let mut p = geometric_probe(id, step_yaw(step), vec![1.0, 0.0]);
            p.sample_quality
                .as_mut()
                .unwrap()
                .face_quality
                .mean_intensity = 0.95;
            std::future::ready(Ok(p))
        })
        .await;
        assert_eq!(id, 40 + POSE_ATTEMPTS as u8);
        assert!(result.unwrap_err().to_string().contains("za jasna"));
    }
    #[tokio::test]
    async fn strict_recognition_requires_all_measured_poses_without_claiming_pad() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut id = 40;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            id += 1;
            std::future::ready(Ok(geometric_probe(id, step_yaw(step), vec![1.0, 0.0])))
        })
        .await
        .unwrap();
        assert_eq!(id, 48);
        assert_eq!(result.samples, 5);
        assert!(result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.80).unwrap()),
            RecognitionDecision::Recognized
        );
        assert!(!result.pad_validated);
        assert!(!result.authentication_ready);
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("ratios"));
        assert!(!json.contains("values"));
    }
    #[tokio::test]
    async fn stationary_photo_and_repeated_frames_fail_motion_challenge() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut id = 40;
        let result = recognize_person(&reference, &identity(), true, &cancel, |_| {
            id += 1;
            std::future::ready(Ok(geometric_probe(id, 0.0, vec![1.0, 0.0])))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(id, 42 + POSE_ATTEMPTS as u8); // two baseline captures plus bounded turn attempts
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            std::future::ready(Ok(geometric_probe(40, step_yaw(step), vec![1.0, 0.0])))
        })
        .await;
        assert!(result.unwrap_err().to_string().contains("Powtórzona"));
    }
    #[tokio::test]
    async fn transient_turn_identity_and_shape_misses_need_two_stable_owner_frames() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let mut p = geometric_probe(
                40 + calls,
                step_yaw(step),
                if calls == 3 {
                    vec![0.7, 0.71414286]
                } else {
                    vec![1.0, 0.0]
                },
            );
            if calls == 4 {
                p.geometry = Some(
                    hello_face::geometry::FaceGeometry::measure(&hello_face::Detection {
                        bounds: [0.0, 0.0, 200.0, 240.0],
                        landmarks: [
                            [50.0, 60.0],
                            [150.0, 60.0],
                            [100.0 + step_yaw(step) * 100.0, 120.0],
                            [15.0, 180.0],
                            [185.0, 180.0],
                        ],
                        confidence: 0.99,
                    })
                    .unwrap(),
                );
            }
            std::future::ready(Ok(p))
        })
        .await
        .unwrap();
        assert_eq!(calls, 10);
        assert_eq!(result.transition_identity_retries, 2);
        assert!(result.minimum() >= 0.8);
        assert!(result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::Recognized
        );
    }

    #[tokio::test]
    async fn a_good_turn_frame_does_not_reset_the_weak_frame_budget_or_complete_turn() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            let weak = matches!(calls, 4 | 6 | 8);
            std::future::ready(Ok(geometric_probe(
                40 + calls,
                step_yaw(step),
                if weak {
                    vec![0.7, 0.71414286]
                } else {
                    vec![1.0, 0.0]
                },
            )))
        })
        .await
        .unwrap();
        assert_eq!(calls, 8); // first two front frames, then alternating strong/weak turn frames
        assert_eq!(result.transition_identity_retries, 2);
        assert!(!result.motion_challenge_passed);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
    }

    #[tokio::test]
    async fn enrollment_discards_transient_inconsistent_turn_frames_and_never_stores_them() {
        let mut r = SessionReference::default();
        r.begin_anonymous_enrollment(&identity(), true).unwrap();
        let reference = std::sync::Mutex::new(r);
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let mut hints = Vec::new();
        let result = collect_reference_observed(
            &reference,
            &identity(),
            true,
            &cancel,
            |count| {
                calls += 1;
                let mut p = sample(40 + calls);
                if calls == 2 || calls == 3 {
                    let mut values = vec![0.0; 128];
                    values[0] = 0.7;
                    values[1] = 0.71414286;
                    p.embedding = probe(values).embedding;
                }
                assert_eq!(
                    count,
                    if calls <= 3 {
                        usize::from(calls > 1)
                    } else {
                        (calls - 3) as usize
                    }
                );
                std::future::ready(Ok(p))
            },
            |_| {},
            |hint| hints.push(hint.to_owned()),
        )
        .await
        .unwrap();
        assert_eq!(result, 8);
        assert_eq!(calls, 10);
        assert_eq!(hints.len(), 2);
        assert!(hints.iter().all(|hint| hint.contains("Zmniejsz skręt")));
        let r = reference.lock().unwrap();
        assert_eq!(r.reference_sample_count(), 8);
    }

    #[tokio::test]
    async fn impostor_that_passed_old_threshold_fails_before_motion() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            calls += 1;
            std::future::ready(Ok(geometric_probe(
                40,
                step_yaw(step),
                vec![0.7, 0.71414286],
            )))
        })
        .await
        .unwrap();
        assert_eq!(calls, 1);
        assert!(result.minimum() > 0.363);
        assert!(result.minimum() < 0.8);
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.363).unwrap()),
            RecognitionDecision::NotRecognized
        );
        assert!(!result.motion_challenge_passed);
    }
    #[tokio::test]
    async fn shape_mismatch_blocks_even_identical_embeddings_and_legacy_cannot_fallback() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            let mut p = geometric_probe(40, step_yaw(step), vec![1.0, 0.0]);
            p.geometry = Some(
                hello_face::geometry::FaceGeometry::measure(&hello_face::Detection {
                    bounds: [0.0, 0.0, 200.0, 240.0],
                    landmarks: [
                        [50.0, 60.0],
                        [150.0, 60.0],
                        [100.0, 120.0],
                        [15.0, 180.0],
                        [185.0, 180.0],
                    ],
                    confidence: 0.99,
                })
                .unwrap(),
            );
            std::future::ready(Ok(p))
        })
        .await
        .unwrap();
        assert_eq!(
            result.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
        let mut legacy = SessionReference::default();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            legacy.add_sample(&identity(), true, sample(id)).unwrap();
        }
        let legacy = std::sync::Mutex::new(legacy);
        let mut calls = 0;
        assert!(
            recognize_person(&legacy, &identity(), true, &cancel, |_| {
                calls += 1;
                std::future::ready(Ok(sample(40)))
            })
            .await
            .is_err()
        );
        assert_eq!(calls, 0);
    }
    #[tokio::test]
    async fn strict_cancellation_and_wrong_camera_never_return_a_match() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        assert!(
            recognize_person(&reference, &identity(), true, &cancel, |step| {
                cancel.store(true, Ordering::Release);
                std::future::ready(Ok(geometric_probe(40, step_yaw(step), vec![1.0, 0.0])))
            })
            .await
            .is_err()
        );
        cancel.store(false, Ordering::Release);
        let mut calls = 0;
        assert!(
            recognize_person(&reference, &identity(), false, &cancel, |_| {
                calls += 1;
                std::future::ready(Ok(sample(40)))
            })
            .await
            .is_err()
        );
        assert_eq!(calls, 0);
    }
    #[tokio::test]
    async fn replacing_reference_during_challenge_aborts() {
        let reference = strict_reference();
        let cancel = AtomicBool::new(false);
        let result = recognize_person(&reference, &identity(), true, &cancel, |step| {
            reference.lock().unwrap().rename("Zmiana").unwrap();
            std::future::ready(Ok(geometric_probe(40, step_yaw(step), vec![1.0, 0.0])))
        })
        .await;
        assert!(result.unwrap_err().to_string().contains("zmienił"));
    }
    #[test]
    fn encrypted_reference_preserves_strict_features_and_geometry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private");
        let password = b"test-only-long-reference-password";
        let mut reference = SessionReference::default();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            let mut values = vec![0.0; 128];
            values[0] = 1.0;
            reference
                .add_sample(&identity(), true, geometric_probe(id, 0.0, values))
                .unwrap();
        }
        reference.rename("Właściciel").unwrap();
        reference.save(&path, password).unwrap();
        let mut loaded = SessionReference::default();
        loaded.load(&path, password).unwrap();
        loaded
            .validate_strict_recognition(&identity(), true)
            .unwrap();
        assert_eq!(loaded.display_name(), Some("Właściciel"));
        for entry in std::fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let encrypted = std::fs::read(entry.path()).unwrap();
                assert!(
                    !encrypted
                        .windows("Właściciel".len())
                        .any(|w| w == "Właściciel".as_bytes())
                );
                assert!(!encrypted.windows(6).any(|w| w == b"ratios"));
            }
        }
    }
    #[test]
    fn random_orders_require_both_turns_and_return_to_center() {
        assert_ne!(motion_steps(true), motion_steps(false));
        assert!(!MotionStep::TurnImageLeft.reached(0.0, 0.0));
        assert!(!MotionStep::ReturnFront.reached(0.16, 0.0));
        assert!(!MotionStep::Front.reached(f32::NAN, 0.0));
        let incomplete = strict_summary(vec![1.0; 5], 5, false);
        assert_eq!(
            incomplete.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
        let weak = strict_summary(vec![0.7, 0.9, 0.9, 0.9, 0.9], 5, true);
        assert_eq!(
            weak.classify(RecognitionThreshold::new(0.8).unwrap()),
            RecognitionDecision::NotRecognized
        );
    }
    fn probe(values: Vec<f32>) -> FaceProbe {
        FaceProbe {
            report: serde_json::json!({"diagnostic_passed":true}),
            embedding: Some(hello_face::Embedding::new([1; 32], values).unwrap()),
            frame_digest: [1; 32],
            faces: Some(1),
            sample_quality: Some(hello_face::sampling::SampleQuality {
                face_width: 128,
                face_height: 128,
                inter_eye_pixels: 45.0,
                coverage: 0.2,
                clearance: 0.1,
                clipped_fraction: 0.01,
                overexposed_fraction: 0.01,
                face_quality: hello_face::QualityMetrics {
                    mean_intensity: 0.5,
                    contrast: 0.1,
                    edge_energy: 0.1,
                },
            }),
            confidence: Some(0.99),
            geometry: None,
            quality: hello_face::QualityMetrics {
                mean_intensity: 0.5,
                contrast: 0.1,
                edge_energy: 0.1,
            },
        }
    }
    #[test]
    fn session_reference_returns_scores_and_forget_removes_it() {
        let mut reference = SessionReference::default();
        let same = probe(vec![1.0, 0.0]);
        assert_eq!(reference.compare(&identity(), true, &same).unwrap(), None);
        reference
            .remember(identity(), true, probe(vec![1.0, 0.0]))
            .unwrap();
        assert_eq!(
            reference.compare(&identity(), true, &same).unwrap(),
            Some(1.0)
        );
        assert_eq!(
            reference
                .compare(&identity(), true, &probe(vec![-1.0, 0.0]))
                .unwrap(),
            Some(-1.0)
        );
        reference.clear();
        assert_eq!(reference.compare(&identity(), true, &same).unwrap(), None);
    }
    #[test]
    fn comparison_rejects_camera_mode_and_model_changes() {
        let mut reference = SessionReference::default();
        reference
            .remember(identity(), true, probe(vec![1.0, 0.0]))
            .unwrap();
        assert!(
            reference
                .compare(&identity(), false, &probe(vec![1.0, 0.0]))
                .is_err()
        );
        let mut different = identity();
        different.device_number = 2;
        assert!(
            reference
                .compare(&different, true, &probe(vec![1.0, 0.0]))
                .is_err()
        );
        let mut different = probe(vec![1.0, 0.0]);
        different.embedding = Some(hello_face::Embedding::new([2; 32], vec![1.0, 0.0]).unwrap());
        assert!(reference.compare(&identity(), true, &different).is_err());
    }
    #[test]
    fn rejected_capture_preserves_previous_reference_and_cannot_be_compared() {
        let mut reference = SessionReference::default();
        reference
            .remember(identity(), true, probe(vec![1.0, 0.0]))
            .unwrap();
        let rejected = || FaceProbe {
            report: serde_json::json!({"diagnostic_passed":false}),
            embedding: None,
            frame_digest: [0; 32],
            faces: Some(0),
            sample_quality: None,
            confidence: None,
            geometry: None,
            quality: hello_face::QualityMetrics {
                mean_intensity: 0.5,
                contrast: 0.1,
                edge_energy: 0.1,
            },
        };
        assert!(reference.remember(identity(), true, rejected()).is_err());
        assert!(reference.compare(&identity(), true, &rejected()).is_err());
        assert_eq!(
            reference
                .compare(&identity(), true, &probe(vec![1.0, 0.0]))
                .unwrap(),
            Some(1.0)
        );
    }

    fn sample(id: u8) -> FaceProbe {
        let mut values = vec![0.0; 128];
        values[0] = 1.0;
        let mut sample = probe(values);
        sample.frame_digest = [id; 32];
        sample
    }

    #[test]
    fn sampling_rejects_duplicates_mixed_cameras_and_inconsistent_faces() {
        let mut reference = SessionReference::default();
        assert_eq!(
            reference.add_sample(&identity(), true, sample(1)).unwrap(),
            1
        );
        assert!(!reference.template_available());
        assert!(reference.add_sample(&identity(), true, sample(1)).is_err());
        assert!(reference.add_sample(&identity(), false, sample(2)).is_err());
        let mut inconsistent = sample(2);
        let mut values = vec![0.0; 128];
        values[0] = -1.0;
        inconsistent.embedding = Some(hello_face::Embedding::new([1; 32], values).unwrap());
        assert!(
            reference
                .add_sample(&identity(), true, inconsistent)
                .is_err()
        );
        assert_eq!(
            reference.add_sample(&identity(), true, sample(2)).unwrap(),
            2
        );
        for id in 3..=REFERENCE_SAMPLES as u8 {
            assert_eq!(
                reference.add_sample(&identity(), true, sample(id)).unwrap(),
                usize::from(id)
            );
            assert_eq!(
                reference.template_available(),
                usize::from(id) == REFERENCE_SAMPLES
            );
        }
        assert!(reference.template_available());
        assert_eq!(
            reference.compare(&identity(), true, &sample(4)).unwrap(),
            Some(1.0)
        );
        reference.clear();
        assert!(!reference.template_available());
        assert_eq!(
            reference.add_sample(&identity(), false, sample(1)).unwrap(),
            1
        );
    }

    #[test]
    fn named_enrollment_keeps_the_old_person_on_rejection_and_cancellation() {
        let mut reference = SessionReference::default();
        reference
            .begin_named_enrollment(&identity(), true, "Jan")
            .unwrap();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
        }
        assert_eq!(reference.display_name(), Some("Jan"));
        assert!(
            reference
                .begin_named_enrollment(&identity(), true, "\n")
                .is_err()
        );
        assert!(reference.template_available());
        reference
            .begin_named_enrollment(&identity(), true, " Ewa ")
            .unwrap();
        assert_eq!(reference.enrollment_display_name(), Some("Ewa"));
        let mut rejected = sample(20);
        rejected.confidence = Some(0.89);
        assert!(reference.add_sample(&identity(), true, rejected).is_err());
        assert_eq!(reference.enrollment_sample_count(), 0);
        assert_eq!(reference.display_name(), Some("Jan"));
        assert!(reference.rename("Ewa").is_err());
        reference.cancel_enrollment();
        assert!(reference.template_available());
        assert_eq!(reference.display_name(), Some("Jan"));
        reference
            .begin_named_enrollment(&identity(), true, "Ewa")
            .unwrap();
        for id in 20..20 + REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
        }
        assert!(!reference.enrollment_active());
        assert_eq!(reference.display_name(), Some("Ewa"));
        reference.rename(" Ewa Nowak ").unwrap();
        assert_eq!(reference.display_name(), Some("Ewa Nowak"));
        assert_eq!(
            reference.compare(&identity(), true, &sample(50)).unwrap(),
            Some(1.0)
        );
    }

    #[test]
    fn encrypted_multisample_reference_survives_a_fresh_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reference");
        let password = b"test reference phrase";
        let mut original = SessionReference::default();
        original.remember(identity(), true, sample(1)).unwrap();
        assert!(original.save(&path, password).is_err());
        assert!(!path.exists());
        for id in 1..=REFERENCE_SAMPLES as u8 {
            original.add_sample(&identity(), true, sample(id)).unwrap();
        }
        original.save(&path, password).unwrap();
        original.rename("Żaneta Kowalska").unwrap();
        original.save(&path, password).unwrap();
        let encrypted = std::fs::read(path.join("template.lht")).unwrap();
        assert!(
            !encrypted
                .windows("Żaneta Kowalska".len())
                .any(|window| window == "Żaneta Kowalska".as_bytes())
        );
        assert!(original.save(&path, b"different reference phrase").is_err());
        assert_eq!(std::fs::read(path.join("template.lht")).unwrap(), encrypted);
        drop(original);
        let mut reopened = SessionReference::default();
        reopened.load(&path, password).unwrap();
        assert_eq!(reopened.display_name(), Some("Żaneta Kowalska"));
        assert!(reopened.template_available());
        assert_eq!(
            reopened.compare(&identity(), true, &sample(4)).unwrap(),
            Some(1.0)
        );
        assert!(reopened.compare(&identity(), false, &sample(4)).is_err());
        assert!(reopened.load(&path, b"different reference phrase").is_err());
        assert_eq!(
            reopened.compare(&identity(), true, &sample(4)).unwrap(),
            Some(1.0)
        );
        assert!(reopened.delete_saved(&path).unwrap());
        assert!(!path.join("template.lht").exists());
        assert_eq!(
            reopened.compare(&identity(), true, &sample(4)).unwrap(),
            None
        );
    }

    #[test]
    fn face_region_rejection_keeps_sampling_progress_unchanged() {
        let mut reference = SessionReference::default();
        let mut soft = sample(1);
        soft.sample_quality
            .as_mut()
            .unwrap()
            .face_quality
            .edge_energy = 0.001;
        assert!(
            reference
                .add_sample(&identity(), true, soft)
                .unwrap_err()
                .to_string()
                .contains("soft")
        );
        assert_eq!(reference.next_instruction(), sample_instruction(0));
        reference.add_sample(&identity(), true, sample(1)).unwrap();
        let mut uncertain = sample(2);
        uncertain.confidence = Some(0.89);
        assert!(reference.add_sample(&identity(), true, uncertain).is_err());
        assert_eq!(reference.next_instruction(), sample_instruction(1));
        assert_eq!(
            reference.add_sample(&identity(), true, sample(2)).unwrap(),
            2
        );
    }

    #[test]
    fn previously_saved_three_sample_templates_remain_readable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reference");
        let password = b"test reference phrase";
        let mut enrollment = hello_face::Enrollment::new(0.8).unwrap();
        for id in 1..=3 {
            let probe = sample(id);
            enrollment
                .add(
                    probe.frame_digest,
                    probe.embedding.unwrap(),
                    1,
                    probe.quality,
                )
                .unwrap();
        }
        let template = enrollment
            .finish()
            .unwrap()
            .bind_capture(capture_binding(&identity(), true).unwrap())
            .unwrap();
        let store = hello_storage::SecureStore::open_passphrase(&path, password, true).unwrap();
        store.save(&template).unwrap();
        drop(store);
        let mut reference = SessionReference::default();
        reference.load(&path, password).unwrap();
        assert_eq!(reference.reference_sample_count(), 3);
        assert!(reference.template_available());
        assert_eq!(
            reference.compare(&identity(), true, &sample(4)).unwrap(),
            Some(1.0)
        );
        assert_eq!(
            reference.add_sample(&identity(), true, sample(1)).unwrap(),
            1
        );
        assert!(!reference.template_available());
        assert!(reference.save(&path, password).is_err());
    }

    #[tokio::test]
    async fn automatic_scan_can_resume_after_error_and_waits_for_a_name() {
        let mut reference = SessionReference::default();
        reference
            .begin_named_enrollment(&identity(), true, "Jan")
            .unwrap();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
        }
        reference
            .begin_anonymous_enrollment(&identity(), true)
            .unwrap();
        let reference = std::sync::Mutex::new(reference);
        let cancel = AtomicBool::new(false);
        let result = collect_reference(
            &reference,
            &identity(),
            true,
            &cancel,
            |count| async move {
                if count == 2 {
                    Err("capture failed".into())
                } else {
                    Ok(sample(20 + count as u8))
                }
            },
            |_| {},
        )
        .await;
        assert!(result.is_err());
        assert_eq!(reference.lock().unwrap().enrollment_sample_count(), 2);
        assert_eq!(reference.lock().unwrap().display_name(), Some("Jan"));
        let mut progress = Vec::new();
        assert_eq!(
            collect_reference(
                &reference,
                &identity(),
                true,
                &cancel,
                |count| async move { Ok(sample(20 + count as u8)) },
                |count| progress.push(count)
            )
            .await
            .unwrap(),
            8
        );
        assert_eq!(progress, vec![2, 3, 4, 5, 6, 7, 8]);
        let mut reference = reference.lock().unwrap();
        assert!(reference.template_available());
        assert_eq!(reference.display_name(), None);
        reference.rename("Ewa").unwrap();
        assert_eq!(reference.display_name(), Some("Ewa"));
    }

    #[tokio::test]
    async fn automatic_scan_cancellation_does_not_add_the_cancelled_probe() {
        let mut reference = SessionReference::default();
        reference
            .begin_anonymous_enrollment(&identity(), true)
            .unwrap();
        let reference = std::sync::Mutex::new(reference);
        let cancel = AtomicBool::new(false);
        let result = collect_reference(
            &reference,
            &identity(),
            true,
            &cancel,
            |count| {
                if count == 2 {
                    cancel.store(true, Ordering::Release);
                }
                async move { Ok(sample(30 + count as u8)) }
            },
            |_| {},
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(reference.lock().unwrap().enrollment_sample_count(), 2);
        assert!(!reference.lock().unwrap().has_reference());
        reference.lock().unwrap().cancel_enrollment();
        assert!(!reference.lock().unwrap().enrollment_active());
    }

    #[tokio::test]
    async fn scan_preflight_and_pre_cancelled_observer_never_capture() {
        let mut reference = SessionReference::default();
        reference
            .begin_anonymous_enrollment(&identity(), true)
            .unwrap();
        let reference = std::sync::Mutex::new(reference);
        let cancel = AtomicBool::new(false);
        let mut captures = 0;
        assert!(
            collect_reference(
                &reference,
                &identity(),
                false,
                &cancel,
                |_| {
                    captures += 1;
                    async { Ok(sample(1)) }
                },
                |_| {}
            )
            .await
            .is_err()
        );
        cancel.store(true, Ordering::Release);
        assert!(
            collect_reference(
                &reference,
                &identity(),
                true,
                &cancel,
                |_| {
                    captures += 1;
                    async { Ok(sample(1)) }
                },
                |_| {}
            )
            .await
            .is_err()
        );
        assert_eq!(captures, 0);
        let frames = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observer_frames = frames.clone();
        let result = inspect_sample_observed(
            "/nonexistent/worker".into(),
            "/nonexistent/camera".into(),
            "/nonexistent/models".into(),
            true,
            None,
            Arc::new(AtomicBool::new(true)),
            Some(Arc::new(move |_| {
                observer_frames.fetch_add(1, Ordering::Relaxed);
            })),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(frames.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn comparison_series_summarizes_signed_scores_without_an_identity_verdict() {
        let mut reference = SessionReference::default();
        reference.remember(identity(), true, sample(10)).unwrap();
        let reference = std::sync::Mutex::new(reference);
        let cancel = AtomicBool::new(false);
        let mut count = 0;
        let summary = compare_three(&reference, &identity(), true, &cancel, || {
            count += 1;
            let mut probe = sample(count);
            let mut values = vec![0.0; 128];
            match count {
                1 => values[0] = 1.0,
                2 => values[0] = -1.0,
                _ => values[1] = 1.0,
            }
            probe.embedding = Some(hello_face::Embedding::new([1; 32], values).unwrap());
            async move { Ok(probe) }
        })
        .await
        .unwrap();
        assert_eq!(count, 3);
        assert_eq!(summary.minimum(), -1.0);
        assert_eq!(summary.median(), 0.0);
        assert_eq!(summary.maximum(), 1.0);
        let report = serde_json::to_value(summary).unwrap();
        assert_eq!(report["authentication_ready"], false);
        assert!(report.get("embedding").is_none());
    }

    #[test]
    fn manual_trials_separate_labels_decisions_failures_and_cancellation() {
        let mut reference = SessionReference::default();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
        }
        let threshold = RecognitionThreshold::new(DEFAULT_RECOGNITION_THRESHOLD).unwrap();
        let mut trials = RecognitionTrials::default();
        for label in [TrialLabel::SavedPerson, TrialLabel::OtherPerson] {
            for decision in [
                RecognitionDecision::Recognized,
                RecognitionDecision::NotRecognized,
                RecognitionDecision::Inconclusive,
            ] {
                trials
                    .record(
                        &reference,
                        threshold,
                        label,
                        TrialOutcome::Decision(decision),
                    )
                    .unwrap();
            }
            trials
                .record(
                    &reference,
                    threshold,
                    label,
                    TrialOutcome::AcquisitionFailed,
                )
                .unwrap();
            let before = trials.report();
            trials
                .record(&reference, threshold, label, TrialOutcome::Cancelled)
                .unwrap();
            assert_eq!(trials.report(), before);
        }
        let report = trials.report();
        assert_eq!(report["total_trials"], 8);
        for label in ["saved_person", "other_person"] {
            for counter in [
                "recognized",
                "not_recognized",
                "inconclusive",
                "acquisition_failed",
            ] {
                assert_eq!(report[label][counter], 1);
            }
        }
        assert_eq!(report["calibrated_accuracy"], false);
        assert_eq!(report["authentication_ready"], false);
        trials.clear();
        assert_eq!(trials.report()["total_trials"], 0);
        assert!(trials.report()["threshold"].is_null());
        trials
            .record(
                &reference,
                threshold,
                TrialLabel::SavedPerson,
                TrialOutcome::Cancelled,
            )
            .unwrap();
        assert!(trials.context.is_none());
    }

    #[test]
    fn manual_trials_do_not_mix_thresholds_or_reference_generations() {
        let make_reference = || {
            let mut reference = SessionReference::default();
            for id in 1..=REFERENCE_SAMPLES as u8 {
                reference.add_sample(&identity(), true, sample(id)).unwrap();
            }
            reference
        };
        let mut reference = make_reference();
        let threshold = RecognitionThreshold::new(0.363).unwrap();
        let mut trials = RecognitionTrials::default();
        trials
            .record(
                &reference,
                threshold,
                TrialLabel::SavedPerson,
                TrialOutcome::Decision(RecognitionDecision::Recognized),
            )
            .unwrap();
        let before = trials.report();
        assert!(
            trials
                .validate(&reference, RecognitionThreshold::new(0.5).unwrap())
                .is_err()
        );
        assert!(
            trials
                .record(
                    &make_reference(),
                    threshold,
                    TrialLabel::OtherPerson,
                    TrialOutcome::AcquisitionFailed
                )
                .is_err()
        );
        reference.clear();
        assert!(trials.validate(&reference, threshold).is_err());
        assert_eq!(trials.report(), before);
        trials.clear();
        assert!(
            trials
                .record(
                    &reference,
                    threshold,
                    TrialLabel::SavedPerson,
                    TrialOutcome::AcquisitionFailed
                )
                .is_err()
        );
        assert_eq!(trials.report()["total_trials"], 0);
        reference = make_reference();
        trials
            .record(
                &reference,
                threshold,
                TrialLabel::SavedPerson,
                TrialOutcome::AcquisitionFailed,
            )
            .unwrap();
        assert_eq!(trials.report()["total_trials"], 1);
    }

    #[test]
    fn manual_trials_are_bounded_and_resettable() {
        let mut reference = SessionReference::default();
        for id in 1..=REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
        }
        let threshold = RecognitionThreshold::new(0.363).unwrap();
        let mut trials = RecognitionTrials::default();
        for _ in 0..1000 {
            trials
                .record(
                    &reference,
                    threshold,
                    TrialLabel::OtherPerson,
                    TrialOutcome::Decision(RecognitionDecision::NotRecognized),
                )
                .unwrap();
        }
        let before = trials.report();
        assert!(trials.validate(&reference, threshold).is_err());
        assert!(
            trials
                .record(
                    &reference,
                    threshold,
                    TrialLabel::OtherPerson,
                    TrialOutcome::AcquisitionFailed
                )
                .is_err()
        );
        assert_eq!(trials.report(), before);
        trials.clear();
        assert!(trials.validate(&reference, threshold).is_ok());
    }

    #[test]
    fn recognition_requires_all_three_scores_and_preserves_ambiguous_results() {
        let threshold = RecognitionThreshold::new(0.363).unwrap();
        for (scores, expected) in [
            ([0.7, 0.8, 0.9], RecognitionDecision::Recognized),
            ([-1.0, 0.0, 0.3], RecognitionDecision::NotRecognized),
            ([0.2, 0.8, 0.9], RecognitionDecision::Inconclusive),
            ([0.2, 0.3, 0.9], RecognitionDecision::Inconclusive),
            ([0.363; 3], RecognitionDecision::Recognized),
        ] {
            let summary = ComparisonSummary {
                samples: 3,
                capture_attempts: 3,
                transition_identity_retries: 0,
                verification_evidence: None,
                failure_reason: None,
                cosine_min: scores[0],
                cosine_median: scores[1],
                cosine_max: scores[2],
                strict_identity: false,
                verification_protocol: VerificationProtocol::ComparisonOnly,
                motion_challenge_passed: false,
                pad_validated: false,
                authentication_ready: false,
                raw_frames_saved: false,
                embedding_saved: false,
            };
            assert_eq!(summary.classify(threshold), expected);
            assert!(!summary.authentication_ready);
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 1.01] {
            assert!(RecognitionThreshold::new(value).is_err());
        }
    }

    #[test]
    fn recognition_requires_completed_eight_sample_template_and_matching_camera() {
        let mut reference = SessionReference::default();
        assert!(reference.validate_recognition(&identity(), true).is_err());
        reference.remember(identity(), true, sample(10)).unwrap();
        assert!(reference.validate_recognition(&identity(), true).is_err());
        for id in 1..=REFERENCE_SAMPLES as u8 {
            reference.add_sample(&identity(), true, sample(id)).unwrap();
            assert_eq!(
                reference.validate_recognition(&identity(), true).is_ok(),
                usize::from(id) == REFERENCE_SAMPLES
            );
        }
        assert!(reference.validate_recognition(&identity(), false).is_err());
        reference.add_sample(&identity(), true, sample(20)).unwrap();
        assert!(reference.validate_recognition(&identity(), true).is_err());
    }

    #[tokio::test]
    async fn comparison_preflight_rejects_pending_only_reference_and_wrong_mode_without_capture() {
        let cancel = AtomicBool::new(false);
        let mut captures = 0;
        for remembered in [false, true] {
            let mut reference = SessionReference::default();
            reference.add_sample(&identity(), true, sample(1)).unwrap();
            if remembered {
                reference.remember(identity(), true, sample(10)).unwrap();
            }
            let reference = std::sync::Mutex::new(reference);
            assert!(
                compare_three(&reference, &identity(), false, &cancel, || {
                    captures += 1;
                    async { Ok(sample(2)) }
                })
                .await
                .is_err()
            );
        }
        assert_eq!(captures, 0);
    }

    #[tokio::test]
    async fn comparison_series_aborts_on_rejection_duplicate_model_change_or_capture_error() {
        for failure in 0..4 {
            let mut reference = SessionReference::default();
            reference.remember(identity(), true, sample(10)).unwrap();
            let reference = std::sync::Mutex::new(reference);
            let cancel = AtomicBool::new(false);
            let mut count = 0;
            let result = compare_three(&reference, &identity(), true, &cancel, || {
                count += 1;
                let mut probe = sample(count);
                if count == 2 {
                    match failure {
                        0 => probe.confidence = Some(0.89),
                        1 => probe.frame_digest = [1; 32],
                        2 => {
                            probe.embedding =
                                Some(hello_face::Embedding::new([2; 32], vec![1.0; 128]).unwrap())
                        }
                        _ => {}
                    }
                }
                let error = failure == 3 && count == 2;
                async move {
                    if error {
                        Err("worker failed".into())
                    } else {
                        Ok(probe)
                    }
                }
            })
            .await;
            assert!(result.is_err());
            assert_eq!(count, 2);
            assert_eq!(
                reference
                    .lock()
                    .unwrap()
                    .compare(&identity(), true, &sample(3))
                    .unwrap(),
                Some(1.0)
            );
        }
    }

    #[tokio::test]
    async fn cancelled_comparison_never_returns_a_partial_summary() {
        let mut reference = SessionReference::default();
        reference.remember(identity(), true, sample(10)).unwrap();
        let reference = std::sync::Mutex::new(reference);
        for pre_cancelled in [true, false] {
            let cancel = AtomicBool::new(pre_cancelled);
            let mut count = 0;
            let result = compare_three(&reference, &identity(), true, &cancel, || {
                count += 1;
                cancel.store(true, Ordering::Release);
                async { Ok(sample(1)) }
            })
            .await;
            assert!(result.unwrap_err().to_string().contains("cancelled"));
            assert_eq!(count, usize::from(!pre_cancelled));
        }
    }

    #[tokio::test]
    async fn sample_retries_stop_at_first_usable_capture_and_are_bounded() {
        let mut attempts = 0;
        let result = retry_samples(|| {
            attempts += 1;
            let mut probe = sample(attempts);
            if attempts < 2 {
                probe
                    .sample_quality
                    .as_mut()
                    .unwrap()
                    .face_quality
                    .edge_energy = 0.001;
            }
            async move { Ok(probe) }
        })
        .await
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(result.report["sample_capture_attempts"], 2);
        let mut attempts = 0;
        let result = retry_samples(|| {
            attempts += 1;
            let mut probe = sample(attempts);
            probe.confidence = Some(0.89);
            async move { Ok(probe) }
        })
        .await
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(result.report["sample_capture_attempts"], 3);
    }

    #[tokio::test]
    async fn sample_retry_does_not_retry_cancellation_or_worker_errors() {
        let mut attempts = 0;
        let result = retry_samples(|| {
            attempts += 1;
            let mut probe = sample(attempts);
            probe.confidence = Some(0.89);
            let error = attempts == 2;
            async move {
                if error {
                    Err("face check cancelled".into())
                } else {
                    Ok(probe)
                }
            }
        })
        .await;
        assert_eq!(attempts, 2);
        assert_eq!(result.err().unwrap().to_string(), "face check cancelled");
    }
    #[tokio::test]
    async fn pre_cancelled_check_never_opens_worker_models_or_camera() {
        let result = face_probe(
            "/nonexistent/worker".into(),
            "/nonexistent/camera".into(),
            "/nonexistent/models".into(),
            true,
            None,
            Arc::new(AtomicBool::new(true)),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "face check cancelled");
    }
}
