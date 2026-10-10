//! Shared security status and a fail-closed local verification state machine.
//!
//! This crate does not mint an authentication credential. In particular, the
//! authentication purpose is disabled until a separately reviewed release.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecurityStatus {
    pub model_ready: bool,
    pub pad_ready: bool,
    pub hardware_validated: bool,
    pub storage_ready: bool,
    pub privileged_authentication_enabled: bool,
    pub warnings: Vec<String>,
}

impl Default for SecurityStatus {
    fn default() -> Self {
        Self {
            model_ready: false,
            pad_ready: false,
            hardware_validated: false,
            storage_ready: false,
            privileged_authentication_enabled: false,
            warnings: vec![
                "No licensed, evaluated face model is configured.".into(),
                "Presentation attack detection has not been validated.".into(),
                "Hardware trust and protected key provisioning are unverified.".into(),
                "Privileged biometric authentication is disabled in this release.".into(),
            ],
        }
    }
}

impl SecurityStatus {
    /// Reporting inputs cannot enable authentication in this unreleased path.
    pub fn authentication_ready(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    LocalDiagnostic,
    SystemAuthentication,
}

/// Inputs must come from transport peer credentials, never IPC request fields.
/// This type is intentionally not serializable/deserializable.
#[derive(Clone, Copy, Debug)]
pub struct RequestContext {
    pub peer_uid: u32,
    pub target_uid: u32,
    pub purpose: Purpose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    AwaitingCapture,
    AwaitingMatch,
    AwaitingPad,
    DiagnosticPassed,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum VerificationError {
    #[error("request context is unauthorized")]
    Unauthorized,
    #[error("privileged biometric authentication is not released")]
    AuthenticationDisabled,
    #[error("verification expired")]
    Expired,
    #[error("invalid verification policy or challenge")]
    InvalidPolicy,
    #[error("verification events arrived out of order")]
    InvalidTransition,
    #[error("sample evidence is not bound to this verification")]
    BindingMismatch,
    #[error("capture quality requirements were not met")]
    QualityRejected,
    #[error("biometric match requirements were not met")]
    MatchRejected,
    #[error("independent presentation attack detection did not pass")]
    PadRejected,
}

/// Opaque per-attempt identifier. Randomness is supplied by the trusted caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EvidenceBinding(pub [u8; 32]);

#[derive(Clone, Copy, Debug)]
pub struct VerificationPolicy {
    pub timeout: Duration,
    pub minimum_match: f32,
    pub minimum_pad: f32,
}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            minimum_match: 0.85,
            minimum_pad: 0.95,
        }
    }
}

/// PAD is independent of similarity and camera modality. IR is never a verdict.
#[derive(Clone, Copy, Debug)]
pub enum PadVerdict {
    Unavailable,
    AttackDetected,
    Live { confidence: f32 },
}

/// Separate PAD boundary. Implementations must evaluate fresh acquisition evidence
/// bound to the supplied attempt. Pixel modality and match similarity are not PAD.
/// No detector is implemented or configured in this release.
pub trait PresentationAttackDetector: Send {
    type Evidence;
    fn evaluate(&mut self, binding: EvidenceBinding, evidence: &Self::Evidence) -> PadVerdict;
}

/// A bounded, one-shot *diagnostic* attempt; success has no authentication power.
/// No wire representation exists for passing a verdict across a trust boundary.
pub struct VerificationSession {
    stage: Stage,
    binding: EvidenceBinding,
    policy: VerificationPolicy,
    deadline: Instant,
}

impl VerificationSession {
    pub fn new(
        context: RequestContext,
        binding: EvidenceBinding,
        policy: VerificationPolicy,
    ) -> Result<Self, VerificationError> {
        Self::new_at(context, binding, policy, Instant::now())
    }

    fn new_at(
        context: RequestContext,
        binding: EvidenceBinding,
        policy: VerificationPolicy,
        now: Instant,
    ) -> Result<Self, VerificationError> {
        if context.peer_uid != context.target_uid {
            return Err(VerificationError::Unauthorized);
        }
        if context.purpose == Purpose::SystemAuthentication {
            return Err(VerificationError::AuthenticationDisabled);
        }
        if binding.0 == [0; 32]
            || policy.timeout.is_zero()
            || policy.timeout > Duration::from_secs(30)
            || !unit_interval(policy.minimum_match)
            || !unit_interval(policy.minimum_pad)
        {
            return Err(VerificationError::InvalidPolicy);
        }
        let deadline = now
            .checked_add(policy.timeout)
            .ok_or(VerificationError::InvalidPolicy)?;
        Ok(Self {
            stage: Stage::AwaitingCapture,
            binding,
            policy,
            deadline,
        })
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn capture(
        &mut self,
        binding: EvidenceBinding,
        quality_accepted: bool,
    ) -> Result<(), VerificationError> {
        self.check(Stage::AwaitingCapture, binding, Instant::now())?;
        if !quality_accepted {
            return self.deny(VerificationError::QualityRejected);
        }
        self.stage = Stage::AwaitingMatch;
        Ok(())
    }

    pub fn matched(
        &mut self,
        binding: EvidenceBinding,
        similarity: f32,
    ) -> Result<(), VerificationError> {
        self.check(Stage::AwaitingMatch, binding, Instant::now())?;
        if !unit_interval(similarity) || similarity < self.policy.minimum_match {
            return self.deny(VerificationError::MatchRejected);
        }
        self.stage = Stage::AwaitingPad;
        Ok(())
    }

    pub fn presentation_check(
        &mut self,
        binding: EvidenceBinding,
        verdict: PadVerdict,
    ) -> Result<(), VerificationError> {
        self.check(Stage::AwaitingPad, binding, Instant::now())?;
        match verdict {
            PadVerdict::Live { confidence }
                if unit_interval(confidence) && confidence >= self.policy.minimum_pad =>
            {
                self.stage = Stage::DiagnosticPassed;
                Ok(())
            }
            _ => self.deny(VerificationError::PadRejected),
        }
    }

    pub fn cancel(&mut self) {
        self.stage = Stage::Denied;
    }

    fn check(
        &mut self,
        expected: Stage,
        binding: EvidenceBinding,
        now: Instant,
    ) -> Result<(), VerificationError> {
        if now >= self.deadline {
            return self.deny(VerificationError::Expired);
        }
        if self.stage != expected {
            return self.deny(VerificationError::InvalidTransition);
        }
        if self.binding != binding {
            return self.deny(VerificationError::BindingMismatch);
        }
        Ok(())
    }

    fn deny<T>(&mut self, error: VerificationError) -> Result<T, VerificationError> {
        self.stage = Stage::Denied;
        Err(error)
    }
}

fn unit_interval(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> RequestContext {
        RequestContext {
            peer_uid: 1000,
            target_uid: 1000,
            purpose: Purpose::LocalDiagnostic,
        }
    }

    fn session() -> VerificationSession {
        VerificationSession::new(
            context(),
            EvidenceBinding([42; 32]),
            VerificationPolicy::default(),
        )
        .expect("valid diagnostic context")
    }

    #[test]
    fn authentication_cannot_be_enabled_by_status_or_context() {
        let status = SecurityStatus {
            model_ready: true,
            pad_ready: true,
            hardware_validated: true,
            storage_ready: true,
            privileged_authentication_enabled: true,
            warnings: vec![],
        };
        assert!(!status.authentication_ready());
        let mut context = context();
        context.purpose = Purpose::SystemAuthentication;
        assert!(matches!(
            VerificationSession::new(
                context,
                EvidenceBinding([1; 32]),
                VerificationPolicy::default()
            ),
            Err(VerificationError::AuthenticationDisabled)
        ));
    }

    #[test]
    fn unauthorized_and_invalid_policy_are_rejected() {
        let mut context = context();
        context.target_uid = 0;
        assert!(matches!(
            VerificationSession::new(
                context,
                EvidenceBinding([1; 32]),
                VerificationPolicy::default()
            ),
            Err(VerificationError::Unauthorized)
        ));
        for minimum_match in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let policy = VerificationPolicy {
                minimum_match,
                ..VerificationPolicy::default()
            };
            assert!(
                VerificationSession::new(super::tests::context(), EvidenceBinding([1; 32]), policy)
                    .is_err()
            );
        }
    }

    #[test]
    fn pad_unavailability_cannot_be_replaced_by_a_good_match() {
        let mut attempt = session();
        let binding = EvidenceBinding([42; 32]);
        assert_eq!(attempt.capture(binding, true), Ok(()));
        assert_eq!(attempt.matched(binding, 1.0), Ok(()));
        assert_eq!(
            attempt.presentation_check(binding, PadVerdict::Unavailable),
            Err(VerificationError::PadRejected)
        );
        assert_eq!(attempt.stage(), Stage::Denied);
        assert!(
            attempt
                .presentation_check(binding, PadVerdict::Live { confidence: 1.0 })
                .is_err()
        );
    }

    #[test]
    fn simulated_diagnostic_is_single_use_and_bound() {
        let binding = EvidenceBinding([42; 32]);
        let mut attempt = session();
        attempt.capture(binding, true).unwrap();
        attempt.matched(binding, 0.99).unwrap();
        attempt
            .presentation_check(binding, PadVerdict::Live { confidence: 0.99 })
            .unwrap();
        assert_eq!(attempt.stage(), Stage::DiagnosticPassed);
        assert!(attempt.capture(binding, true).is_err());
        let mut attempt = session();
        assert_eq!(
            attempt.capture(EvidenceBinding([43; 32]), true),
            Err(VerificationError::BindingMismatch)
        );
        assert_eq!(attempt.stage(), Stage::Denied);
    }

    #[test]
    fn nan_and_expiry_fail_closed() {
        let binding = EvidenceBinding([42; 32]);
        let mut attempt = session();
        attempt.capture(binding, true).unwrap();
        assert_eq!(
            attempt.matched(binding, f32::NAN),
            Err(VerificationError::MatchRejected)
        );
        let now = Instant::now();
        let mut attempt =
            VerificationSession::new_at(context(), binding, VerificationPolicy::default(), now)
                .unwrap();
        assert_eq!(
            attempt.check(
                Stage::AwaitingCapture,
                binding,
                now + Duration::from_secs(11)
            ),
            Err(VerificationError::Expired)
        );
    }
}
