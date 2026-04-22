//! Secret-material wrappers with redacted diagnostics and zeroization.
//!
//! These types make secret-bearing byte buffers explicit in function signatures.
//! They intentionally avoid `Clone`, serde derives, and equality operations.

use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Approved boundary for exporting raw wrapped secret bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretExportKind {
    /// Exporting into `SecureStorageEffects` or equivalent secure local storage.
    SecureStorage,
    /// Exporting into a key-wrapping or encryption boundary.
    KeyWrapping,
}

/// Explicit context token required before raw secret bytes can leave a wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretExportContext {
    kind: SecretExportKind,
    boundary: &'static str,
}

impl SecretExportContext {
    /// Authorizes export directly into a secure-storage boundary.
    #[must_use]
    pub const fn secure_storage(boundary: &'static str) -> Self {
        Self {
            kind: SecretExportKind::SecureStorage,
            boundary,
        }
    }

    /// Authorizes export directly into a key-wrapping boundary.
    #[must_use]
    pub const fn key_wrapping(boundary: &'static str) -> Self {
        Self {
            kind: SecretExportKind::KeyWrapping,
            boundary,
        }
    }

    /// Returns the export kind.
    #[must_use]
    pub const fn kind(self) -> SecretExportKind {
        self.kind
    }

    /// Returns the named boundary authorizing the export.
    #[must_use]
    pub const fn boundary(self) -> &'static str {
        self.boundary
    }
}

/// Opaque byte buffer for secret material.
pub struct SecretBytes {
    bytes: Vec<u8>,
}

impl SecretBytes {
    /// Imports owned secret bytes into a zeroizing wrapper.
    #[must_use]
    pub fn import(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Imports borrowed secret bytes into a zeroizing wrapper.
    #[must_use]
    pub fn import_from_slice(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
        }
    }

    /// Returns the number of bytes held by this wrapper.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether this wrapper contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Exposes the secret bytes to a caller that is already in a secret-handling
    /// boundary. Prefer passing the wrapper itself when possible.
    #[must_use]
    pub fn expose_secret(&self) -> &[u8] {
        &self.bytes
    }

    /// Exports the raw secret bytes by consuming the wrapper.
    #[must_use]
    pub fn export_secret(mut self, _context: SecretExportContext) -> Vec<u8> {
        core::mem::take(&mut self.bytes)
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBytes")
            .field("len", &self.len())
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl Zeroize for SecretBytes {
    fn zeroize(&mut self) {
        self.bytes.zeroize();
    }
}

impl ZeroizeOnDrop for SecretBytes {}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.zeroize();
    }
}

macro_rules! secret_newtype {
    ($name:ident, $expose:ident, $export:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name {
            bytes: SecretBytes,
        }

        impl $name {
            /// Imports owned bytes into this secret wrapper.
            #[must_use]
            pub fn import(bytes: Vec<u8>) -> Self {
                Self {
                    bytes: SecretBytes::import(bytes),
                }
            }

            /// Imports borrowed bytes into this secret wrapper.
            #[must_use]
            pub fn import_from_slice(bytes: &[u8]) -> Self {
                Self {
                    bytes: SecretBytes::import_from_slice(bytes),
                }
            }

            /// Returns the number of wrapped bytes.
            #[must_use]
            pub fn len(&self) -> usize {
                self.bytes.len()
            }

            /// Returns whether this wrapper contains no bytes.
            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.bytes.is_empty()
            }

            /// Exposes the wrapped bytes to a caller that is already in the
            /// appropriate secret-handling boundary.
            #[must_use]
            pub fn $expose(&self) -> &[u8] {
                self.bytes.expose_secret()
            }

            /// Exports the raw wrapped bytes by consuming the wrapper.
            #[must_use]
            pub fn $export(self, context: SecretExportContext) -> Vec<u8> {
                self.bytes.export_secret(context)
            }

            /// Converts this typed wrapper back into the generic secret wrapper.
            #[must_use]
            pub fn into_secret_bytes(self) -> SecretBytes {
                self.bytes
            }
        }

        impl From<SecretBytes> for $name {
            fn from(bytes: SecretBytes) -> Self {
                Self { bytes }
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("len", &self.len())
                    .field("bytes", &"<redacted>")
                    .finish()
            }
        }

        impl Zeroize for $name {
            fn zeroize(&mut self) {
                self.bytes.zeroize();
            }
        }

        impl ZeroizeOnDrop for $name {}
    };
}

secret_newtype!(
    PrivateKeyBytes,
    expose_private_key,
    export_private_key,
    "Opaque private-key bytes."
);
secret_newtype!(
    SigningShareBytes,
    expose_signing_share,
    export_signing_share,
    "Opaque threshold signing-share bytes."
);
secret_newtype!(
    EncryptedSecretBlob,
    expose_encrypted_secret_blob,
    export_encrypted_secret_blob,
    "Opaque encrypted secret payload bytes."
);

#[cfg(test)]
mod tests {
    use super::{
        EncryptedSecretBlob, PrivateKeyBytes, SecretBytes, SecretExportContext, SecretExportKind,
        SigningShareBytes,
    };
    use zeroize::ZeroizeOnDrop;

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

    #[test]
    fn secret_bytes_debug_is_redacted() {
        let secret = SecretBytes::import(vec![77, 88, 99]);

        let debug = format!("{secret:?}");

        assert!(debug.contains("SecretBytes"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("len: 3"));
        assert!(!debug.contains("77"));
        assert!(!debug.contains("88"));
        assert!(!debug.contains("99"));
    }

    #[test]
    fn typed_secret_wrappers_expose_only_explicitly() {
        let private_key = PrivateKeyBytes::import_from_slice(&[7, 8]);
        let signing_share = SigningShareBytes::import_from_slice(&[9, 10]);
        let encrypted_blob = EncryptedSecretBlob::import_from_slice(&[11, 12]);

        assert_eq!(private_key.expose_private_key(), &[7, 8]);
        assert_eq!(signing_share.expose_signing_share(), &[9, 10]);
        assert_eq!(encrypted_blob.expose_encrypted_secret_blob(), &[11, 12]);
        assert!(format!("{private_key:?}").contains("<redacted>"));
        assert!(format!("{signing_share:?}").contains("<redacted>"));
        assert!(format!("{encrypted_blob:?}").contains("<redacted>"));
    }

    #[test]
    fn secret_wrappers_zeroize_on_drop_and_export_explicitly() {
        assert_zeroize_on_drop::<SecretBytes>();
        assert_zeroize_on_drop::<PrivateKeyBytes>();
        assert_zeroize_on_drop::<SigningShareBytes>();
        assert_zeroize_on_drop::<EncryptedSecretBlob>();

        let secure_storage = SecretExportContext::secure_storage("unit-test secure storage");
        let key_wrapping = SecretExportContext::key_wrapping("unit-test key wrapping");
        assert_eq!(secure_storage.kind(), SecretExportKind::SecureStorage);
        assert_eq!(key_wrapping.boundary(), "unit-test key wrapping");

        assert_eq!(
            SecretBytes::import_from_slice(&[1, 2]).export_secret(secure_storage),
            vec![1, 2]
        );
        assert_eq!(
            PrivateKeyBytes::import_from_slice(&[3, 4]).export_private_key(secure_storage),
            vec![3, 4]
        );
        assert_eq!(
            SigningShareBytes::import_from_slice(&[5, 6]).export_signing_share(secure_storage),
            vec![5, 6]
        );
        assert_eq!(
            EncryptedSecretBlob::import_from_slice(&[7, 8])
                .export_encrypted_secret_blob(key_wrapping),
            vec![7, 8]
        );
    }
}
