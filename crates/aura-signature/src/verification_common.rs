use crate::{AuthenticationError, Result};
use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

pub(crate) fn verify_ed25519_signature(
    message: &[u8],
    signature: &Ed25519Signature,
    public_key: &Ed25519VerifyingKey,
    crypto_error: impl FnOnce(String) -> AuthenticationError,
    invalid_error: impl FnOnce() -> AuthenticationError,
) -> Result<()> {
    let valid = aura_core::ed25519_verify(message, signature, public_key)
        .map_err(|e| crypto_error(e.to_string()))?;

    if valid {
        Ok(())
    } else {
        Err(invalid_error())
    }
}
