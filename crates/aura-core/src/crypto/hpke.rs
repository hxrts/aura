//! HPKE (Hybrid Public Key Encryption) key types

use crate::secrets::{PrivateKeyBytes, SecretExportContext, SecretExportKind};
use core::fmt;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// HPKE public key bytes (X25519 serialized representation).
// aura-security: secret-derive-justified owner=security-refactor expires=before-release remediation=work/2.md HPKE public keys are non-secret transport material; clone/serde are required for wire/storage use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkePublicKey(pub Vec<u8>);

// aura-security: secret-derive-justified owner=security-refactor expires=before-release remediation=work/2.md Encoded HPKE keypairs exist only behind explicit secure export/import boundaries.
#[derive(Serialize, Deserialize)]
struct EncodedHpkeKeyPair {
    public: Vec<u8>,
    private: Vec<u8>,
}

/// HPKE private key bytes.
///
/// This wrapper redacts diagnostics, zeroizes on drop, and requires an
/// explicit export context before raw bytes can leave the wrapper.
pub struct HpkePrivateKey {
    bytes: PrivateKeyBytes,
}

impl HpkePrivateKey {
    /// Import freshly generated private-key bytes into a zeroizing wrapper.
    #[must_use]
    pub fn import(bytes: Vec<u8>) -> Self {
        Self {
            bytes: PrivateKeyBytes::import(bytes),
        }
    }

    /// Borrow the wrapped private-key bytes inside a secret-handling boundary.
    #[must_use]
    pub fn expose_private_key(&self) -> &[u8] {
        self.bytes.expose_private_key()
    }

    /// Export the private key for direct secure-storage persistence.
    pub fn export_for_secure_storage(
        &self,
        context: SecretExportContext,
    ) -> Result<Vec<u8>, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::SecureStorage)?;
        Ok(self.bytes.expose_private_key().to_vec())
    }

    /// Import a private key that crossed a secure-storage boundary.
    pub fn import_from_secure_storage(
        bytes: &[u8],
        context: SecretExportContext,
    ) -> Result<Self, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::SecureStorage)?;
        Ok(Self {
            bytes: PrivateKeyBytes::import_from_slice(bytes),
        })
    }

    /// Export the private key for key wrapping or envelope encryption.
    pub fn export_for_key_wrapping(
        &self,
        context: SecretExportContext,
    ) -> Result<Vec<u8>, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::KeyWrapping)?;
        Ok(self.bytes.expose_private_key().to_vec())
    }

    /// Import a private key that crossed an explicit key-wrapping boundary.
    pub fn import_from_key_wrapping(
        bytes: &[u8],
        context: SecretExportContext,
    ) -> Result<Self, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::KeyWrapping)?;
        Ok(Self {
            bytes: PrivateKeyBytes::import_from_slice(bytes),
        })
    }
}

impl Default for HpkePrivateKey {
    fn default() -> Self {
        Self::import(Vec::new())
    }
}

impl fmt::Debug for HpkePrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HpkePrivateKey")
            .field("len", &self.expose_private_key().len())
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl Zeroize for HpkePrivateKey {
    fn zeroize(&mut self) {
        self.bytes.zeroize();
    }
}

impl ZeroizeOnDrop for HpkePrivateKey {}

/// HPKE keypair with explicit secret-handling boundaries.
pub struct HpkeKeyPair {
    public: HpkePublicKey,
    private: HpkePrivateKey,
}

impl HpkeKeyPair {
    /// Create a keypair from freshly generated key material.
    #[must_use]
    pub fn new(public: Vec<u8>, private: Vec<u8>) -> Self {
        Self {
            public: HpkePublicKey(public),
            private: HpkePrivateKey::import(private),
        }
    }

    /// Borrow the public key.
    #[must_use]
    pub fn public_key(&self) -> &HpkePublicKey {
        &self.public
    }

    /// Borrow the private key.
    #[must_use]
    pub fn private_key(&self) -> &HpkePrivateKey {
        &self.private
    }

    /// Export the keypair for direct secure-storage persistence.
    pub fn export_for_secure_storage(
        &self,
        context: SecretExportContext,
    ) -> Result<Vec<u8>, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::SecureStorage)?;
        crate::util::serialization::to_vec(&EncodedHpkeKeyPair {
            public: self.public.0.clone(),
            private: self.private.expose_private_key().to_vec(),
        })
    }

    /// Import a keypair that crossed a secure-storage boundary.
    pub fn import_from_secure_storage(
        bytes: &[u8],
        context: SecretExportContext,
    ) -> Result<Self, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::SecureStorage)?;
        let encoded: EncodedHpkeKeyPair = crate::util::serialization::from_slice(bytes)?;
        Ok(Self::new(encoded.public, encoded.private))
    }

    /// Export the keypair for explicit key wrapping or envelope encryption.
    pub fn export_for_key_wrapping(
        &self,
        context: SecretExportContext,
    ) -> Result<Vec<u8>, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::KeyWrapping)?;
        crate::util::serialization::to_vec(&EncodedHpkeKeyPair {
            public: self.public.0.clone(),
            private: self.private.expose_private_key().to_vec(),
        })
    }

    /// Import a keypair that crossed an explicit key-wrapping boundary.
    pub fn import_from_key_wrapping(
        bytes: &[u8],
        context: SecretExportContext,
    ) -> Result<Self, crate::util::serialization::SerializationError> {
        validate_context(context, SecretExportKind::KeyWrapping)?;
        let encoded: EncodedHpkeKeyPair = crate::util::serialization::from_slice(bytes)?;
        Ok(Self::new(encoded.public, encoded.private))
    }
}

impl Default for HpkeKeyPair {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new())
    }
}

impl fmt::Debug for HpkeKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HpkeKeyPair")
            .field("public_len", &self.public.0.len())
            .field("private", &self.private)
            .finish()
    }
}

impl Zeroize for HpkeKeyPair {
    fn zeroize(&mut self) {
        self.public.0.zeroize();
        self.private.zeroize();
    }
}

impl ZeroizeOnDrop for HpkeKeyPair {}

fn validate_context(
    context: SecretExportContext,
    expected_kind: SecretExportKind,
) -> Result<(), crate::util::serialization::SerializationError> {
    if context.kind() == expected_kind {
        Ok(())
    } else {
        Err(
            crate::util::serialization::SerializationError::InvalidFormat(format!(
                "hpke key export requires {expected_kind:?} context, got {:?} from {}",
                context.kind(),
                context.boundary(),
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{HpkeKeyPair, HpkePrivateKey};
    use crate::secrets::{SecretExportContext, SecretExportKind};
    use zeroize::{Zeroize, ZeroizeOnDrop};

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

    #[test]
    fn hpke_private_key_debug_is_redacted() {
        let private = HpkePrivateKey::import(vec![7, 8, 9]);
        let debug = format!("{private:?}");

        assert!(debug.contains("HpkePrivateKey"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("7"));
        assert!(!debug.contains("8"));
        assert!(!debug.contains("9"));
    }

    #[test]
    fn hpke_keypair_debug_is_redacted() {
        let keypair = HpkeKeyPair::new(vec![1, 2, 3], vec![4, 5, 6]);
        let debug = format!("{keypair:?}");

        assert!(debug.contains("HpkeKeyPair"));
        assert!(debug.contains("HpkePrivateKey"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("4"));
        assert!(!debug.contains("5"));
        assert!(!debug.contains("6"));
    }

    #[test]
    fn hpke_private_key_zeroize_is_observable() {
        let mut private = HpkePrivateKey::import(vec![10, 11, 12]);
        private.zeroize();

        assert!(private.expose_private_key().is_empty());
    }

    #[test]
    fn hpke_key_material_requires_explicit_secure_export_context() {
        assert_zeroize_on_drop::<HpkePrivateKey>();
        assert_zeroize_on_drop::<HpkeKeyPair>();

        let secure_storage = SecretExportContext::secure_storage("hpke-test storage");
        let key_wrapping = SecretExportContext::key_wrapping("hpke-test wrapping");
        assert_eq!(secure_storage.kind(), SecretExportKind::SecureStorage);
        assert_eq!(key_wrapping.kind(), SecretExportKind::KeyWrapping);

        let private = HpkePrivateKey::import(vec![1, 2, 3]);
        assert_eq!(
            private
                .export_for_secure_storage(secure_storage)
                .expect("secure-storage export"),
            vec![1, 2, 3]
        );

        let keypair = HpkeKeyPair::new(vec![4, 5, 6], vec![7, 8, 9]);
        let encoded = keypair
            .export_for_key_wrapping(key_wrapping)
            .expect("key-wrapping export");
        let restored = HpkeKeyPair::import_from_key_wrapping(&encoded, key_wrapping)
            .expect("key-wrapping import");

        assert_eq!(restored.public_key().0, vec![4, 5, 6]);
        assert_eq!(restored.private_key().expose_private_key(), &[7, 8, 9]);
        assert!(HpkePrivateKey::import_from_secure_storage(
            &[1, 2, 3],
            SecretExportContext::key_wrapping("wrong-boundary"),
        )
        .is_err());
    }
}
