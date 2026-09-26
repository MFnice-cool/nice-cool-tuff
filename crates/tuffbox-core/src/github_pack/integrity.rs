//! Generic Ed25519 payload signatures. Not TuffSwarm-specific.

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("malformed public key")]
    BadPublicKey,
    #[error("malformed signature")]
    BadSignature,
    #[error("signature does not match payload")]
    VerifyFailed,
    #[error("pinned signer {pinned} does not match incoming {incoming}")]
    SignerChanged { pinned: String, incoming: String },
}

pub struct Ed25519KeyPair {
    signing: SigningKey,
}

impl Clone for Ed25519KeyPair {
    fn clone(&self) -> Self {
        Self {
            signing: self.signing.clone(),
        }
    }
}

impl std::fmt::Debug for Ed25519KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ed25519KeyPair")
            .field("public", &self.public_key_b64())
            .finish()
    }
}

impl Ed25519KeyPair {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn public_key_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.signing.verifying_key().as_bytes())
    }

    pub fn to_seed_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.signing.to_bytes())
    }

    pub fn from_seed_b64(seed: &str) -> Result<Self, SignatureError> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(seed.trim())
            .map_err(|_| SignatureError::BadPublicKey)?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| SignatureError::BadPublicKey)?;
        Ok(Self {
            signing: SigningKey::from_bytes(&arr),
        })
    }
}

pub fn sign_payload(key: &Ed25519KeyPair, payload: &[u8]) -> String {
    let sig = key.signing.sign(payload);
    base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
}

pub fn verify_payload(
    public_key_b64: &str,
    signature_b64: &str,
    payload: &[u8],
) -> Result<(), SignatureError> {
    let pk_bytes = base64::engine::general_purpose::STANDARD
        .decode(public_key_b64)
        .map_err(|_| SignatureError::BadPublicKey)?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SignatureError::BadPublicKey)?;
    let verifying = VerifyingKey::from_bytes(&pk_arr).map_err(|_| SignatureError::BadPublicKey)?;
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| SignatureError::BadSignature)?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SignatureError::BadSignature)?;
    verifying
        .verify(payload, &Signature::from_bytes(&sig_arr))
        .map_err(|_| SignatureError::VerifyFailed)
}

/// TOFU: pin the first signer; later updates must keep the same public key.
pub fn pin_or_check_signer(pinned: Option<&str>, incoming: &str) -> Result<String, SignatureError> {
    match pinned {
        None | Some("") => Ok(incoming.to_string()),
        Some(existing) if existing == incoming => Ok(existing.to_string()),
        Some(existing) => Err(SignatureError::SignerChanged {
            pinned: existing.to_string(),
            incoming: incoming.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let key = Ed25519KeyPair::generate();
        let payload = b"demo.tuffbox.json-canonical";
        let sig = sign_payload(&key, payload);
        verify_payload(&key.public_key_b64(), &sig, payload).unwrap();
        assert!(verify_payload(&key.public_key_b64(), &sig, b"tampered").is_err());
    }

    #[test]
    fn tofu_rejects_signer_change() {
        let a = Ed25519KeyPair::generate();
        let b = Ed25519KeyPair::generate();
        let pinned = pin_or_check_signer(None, &a.public_key_b64()).unwrap();
        assert_eq!(
            pin_or_check_signer(Some(&pinned), &a.public_key_b64()).unwrap(),
            pinned
        );
        assert!(matches!(
            pin_or_check_signer(Some(&pinned), &b.public_key_b64()),
            Err(SignatureError::SignerChanged { .. })
        ));
    }

    #[test]
    fn seed_round_trip_preserves_key() {
        let key = Ed25519KeyPair::generate();
        let restored = Ed25519KeyPair::from_seed_b64(&key.to_seed_b64()).unwrap();
        assert_eq!(key.public_key_b64(), restored.public_key_b64());
    }
}
