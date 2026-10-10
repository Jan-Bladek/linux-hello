//! Authenticated local template envelopes and Linux descriptor-relative storage.
//! Keys must be provisioned by a separately authenticated trusted component.
//! This crate does not claim TPM protection or protection from a compromised root.

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hello_face::{MAX_TEMPLATE_BYTES, Template};
use thiserror::Error;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"LHELLO01";
const HEADER_LEN: usize = 40;
const TAG_LEN: usize = 16;
pub const MAX_ENVELOPE_BYTES: usize = HEADER_LEN + MAX_TEMPLATE_BYTES + TAG_LEN;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("secure randomness is unavailable")]
    RandomUnavailable,
    #[error("encrypted template is malformed or unauthentic")]
    InvalidEnvelope,
    #[error("template owner does not match the authorized context")]
    OwnerMismatch,
    #[error("storage requires a private, owned directory and regular private files")]
    InsecurePath,
    #[error("template serialization is invalid")]
    InvalidTemplate,
    #[error("storage I/O failed")]
    Io(#[source] std::io::Error),
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Redacted by absence of Debug; the key is overwritten when dropped.
pub struct SecretKey(Zeroizing<[u8; 32]>);

impl SecretKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn generate() -> Result<Self, StorageError> {
        let mut bytes = Zeroizing::new([0; 32]);
        getrandom::fill(&mut *bytes).map_err(|_| StorageError::RandomUnavailable)?;
        Ok(Self(bytes))
    }
}

pub fn encrypt_template(
    key: &SecretKey,
    owner_uid: u32,
    template: &Template,
) -> Result<Vec<u8>, StorageError> {
    let plaintext = template
        .to_bytes()
        .map_err(|_| StorageError::InvalidTemplate)?;
    if plaintext.len() > MAX_TEMPLATE_BYTES {
        return Err(StorageError::InvalidTemplate);
    }
    let ciphertext_len = plaintext
        .len()
        .checked_add(TAG_LEN)
        .ok_or(StorageError::InvalidTemplate)?;
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut nonce).map_err(|_| StorageError::RandomUnavailable)?;
    let mut header = [0_u8; HEADER_LEN];
    header[0..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&owner_uid.to_le_bytes());
    header[12..36].copy_from_slice(&nonce);
    header[36..40].copy_from_slice(&(ciphertext_len as u32).to_le_bytes());
    let cipher =
        XChaCha20Poly1305::new_from_slice(&*key.0).map_err(|_| StorageError::InvalidEnvelope)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .map_err(|_| StorageError::InvalidEnvelope)?;
    let mut envelope = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    envelope.extend_from_slice(&header);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

pub fn decrypt_template(
    key: &SecretKey,
    expected_owner: u32,
    envelope: &[u8],
) -> Result<Template, StorageError> {
    if envelope.len() < HEADER_LEN + TAG_LEN
        || envelope.len() > MAX_ENVELOPE_BYTES
        || &envelope[..8] != MAGIC
    {
        return Err(StorageError::InvalidEnvelope);
    }
    let uid = u32::from_le_bytes(
        envelope[8..12]
            .try_into()
            .map_err(|_| StorageError::InvalidEnvelope)?,
    );
    if uid != expected_owner {
        return Err(StorageError::OwnerMismatch);
    }
    let declared = u32::from_le_bytes(
        envelope[36..40]
            .try_into()
            .map_err(|_| StorageError::InvalidEnvelope)?,
    ) as usize;
    if declared != envelope.len() - HEADER_LEN {
        return Err(StorageError::InvalidEnvelope);
    }
    let nonce: [u8; 24] = envelope[12..36]
        .try_into()
        .map_err(|_| StorageError::InvalidEnvelope)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&*key.0).map_err(|_| StorageError::InvalidEnvelope)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &envelope[HEADER_LEN..],
                    aad: &envelope[..HEADER_LEN],
                },
            )
            .map_err(|_| StorageError::InvalidEnvelope)?,
    );
    Template::from_bytes(&plaintext).map_err(|_| StorageError::InvalidTemplate)
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::SecureStore;

#[cfg(test)]
mod tests {
    use super::*;
    use hello_face::{Embedding, Enrollment, QualityMetrics};

    pub(super) fn template() -> Template {
        let mut enrollment = Enrollment::new(0.8).unwrap();
        for index in 0..3 {
            enrollment
                .add(
                    [index; 32],
                    Embedding::new([9; 32], vec![1.0, 0.05 * f32::from(index)]).unwrap(),
                    1,
                    QualityMetrics {
                        mean_intensity: 0.5,
                        contrast: 0.1,
                        edge_energy: 0.1,
                    },
                )
                .unwrap();
        }
        enrollment.finish().unwrap()
    }

    #[test]
    fn envelope_roundtrip_is_randomized_and_owner_bound() {
        let key = SecretKey::from_bytes([7; 32]);
        let template = template();
        let first = encrypt_template(&key, 1000, &template).unwrap();
        let second = encrypt_template(&key, 1000, &template).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            decrypt_template(&key, 1000, &first).unwrap().sample_count(),
            3
        );
        assert!(matches!(
            decrypt_template(&key, 1001, &first),
            Err(StorageError::OwnerMismatch)
        ));
        assert!(decrypt_template(&SecretKey::from_bytes([8; 32]), 1000, &first).is_err());
    }

    #[test]
    fn every_envelope_byte_is_authenticated_and_lengths_are_bounded() {
        let key = SecretKey::from_bytes([7; 32]);
        let envelope = encrypt_template(&key, 1000, &template()).unwrap();
        for index in 0..envelope.len() {
            let mut changed = envelope.clone();
            changed[index] ^= 1;
            assert!(
                decrypt_template(&key, 1000, &changed).is_err(),
                "tamper at byte {index}"
            );
        }
        for length in 0..envelope.len() {
            assert!(decrypt_template(&key, 1000, &envelope[..length]).is_err());
        }
        assert!(decrypt_template(&key, 1000, &vec![0; MAX_ENVELOPE_BYTES + 1]).is_err());
    }
}
