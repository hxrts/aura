//! Selected profile allocation lifetime ledger; Unix descriptor backend only.
//! Public ordinary storage has no method returning these private backend objects.
use super::*;
use aura_core::effects::secret_lifetime::*;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

const DIRECTORY: &str = ".allocation-lifetimes-v1";
const MAGIC: &[u8] = b"AURA-LIFETIME-V1\0";
const MAX_RECORD_BYTES: usize = 524_288;

// aura-security: secret-derive-justified owner=allocation-lifetime-provider expires=before-release remediation=docs/100_crypto.md private persistence codec only; plaintext is Zeroizing, writes are AEAD-sealed, record Drop zeroizes secret bytes, and no Debug/Clone or public export is provided.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifetimeRecord {
    version: u16,
    reference: SecretAllocationReference,
    first_decision: Option<FirstDecision>,
    retired: bool,
    #[serde(with = "secret_codec")]
    secret: Zeroizing<Vec<u8>>,
}

mod secret_codec {
    use super::*;

    pub(super) fn serialize<S: serde::Serializer>(
        secret: &Zeroizing<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        secret.as_slice().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Zeroizing<Vec<u8>>, D::Error> {
        struct SecretVisitor;
        impl<'de> serde::de::Visitor<'de> for SecretVisitor {
            type Value = Zeroizing<Vec<u8>>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded secret byte sequence")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                // Reserve the admitted bound once so secret-bearing buffers never
                // move through Vec growth before their zeroizing owner drops.
                let mut secret = Zeroizing::new(Vec::with_capacity(MAX_SECRET_BYTES));
                while let Some(byte) = sequence.next_element::<u8>()? {
                    if secret.len() == MAX_SECRET_BYTES {
                        return Err(serde::de::Error::custom("secret byte limit exceeded"));
                    }
                    secret.push(byte);
                }
                Ok(secret)
            }
        }
        deserializer.deserialize_seq(SecretVisitor)
    }
}

// Count without retaining plaintext, then encode into one protected allocation.
// The slice writer cannot grow or release an earlier secret-bearing allocation.
fn encode_protected_json<T: Serialize>(
    record: &T,
    maximum: usize,
    operation: &str,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    struct Count {
        length: usize,
        maximum: usize,
    }
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.length = self
                .length
                .checked_add(bytes.len())
                .filter(|length| *length <= self.maximum)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "protected JSON byte limit exceeded",
                    )
                })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let failure = |source| AuraError::Serialization {
        message: operation.into(),
        source: Some(Arc::new(source)),
    };
    let mut count = Count { length: 0, maximum };
    serde_json::to_writer(&mut count, record).map_err(failure)?;
    let mut plaintext = Zeroizing::new(Vec::new());
    plaintext.resize(count.length, 0);
    let mut writer = std::io::Cursor::new(plaintext.as_mut_slice());
    serde_json::to_writer(&mut writer, record).map_err(failure)?;
    if writer.position() != count.length as u64 {
        return Err(AuraError::Serialization {
            message: operation.into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "protected JSON changed length between count and encoding",
            ))),
        });
    }
    Ok(plaintext)
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn protected_json_second_pass_cannot_grow_its_secret_buffer() {
        struct Growing(std::cell::Cell<bool>);
        impl Serialize for Growing {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let second = self.0.replace(true);
                serializer.serialize_str(if second { "longer secret" } else { "x" })
            }
        }
        let failure = encode_protected_json(&Growing(std::cell::Cell::new(false)), 1024, "growth")
            .expect_err("non-growing protected slice must reject changed encoding");
        let AuraError::Serialization {
            source: Some(source),
            ..
        } = failure
        else {
            panic!("bounded writer must preserve the actual serialization source");
        };
        assert_eq!(
            source
                .downcast_ref::<serde_json::Error>()
                .expect("actual non-growing writer IO source")
                .io_error_kind(),
            Some(std::io::ErrorKind::WriteZero)
        );
    }

    #[test]
    fn protected_json_encoding_preserves_wire_bounds_and_partial_failure_sources() {
        let bytes = vec![1_u8, 255, 0, 17];
        let expected = serde_json::to_vec(&bytes).expect("reference JSON");
        let encoded = encode_protected_json(&bytes, expected.len(), "fixture encode")
            .expect("exact protected budget");
        assert_eq!(encoded.as_slice(), expected);
        assert_eq!(encoded.len(), expected.len());
        let overflow = encode_protected_json(&bytes, expected.len() - 1, "fixture overflow")
            .expect_err("count pass must reject overflow before plaintext allocation");
        let AuraError::Serialization {
            source: Some(source),
            ..
        } = overflow
        else {
            panic!("overflow must preserve actual codec category and cause");
        };
        let codec = source
            .downcast_ref::<serde_json::Error>()
            .expect("native JSON IO error");
        assert_eq!(
            codec.io_error_kind(),
            Some(std::io::ErrorKind::InvalidInput)
        );
        struct Partial(std::cell::Cell<usize>);
        impl Serialize for Partial {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeSeq;
                let pass = self.0.get();
                self.0.set(pass + 1);
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element(&[91_u8, 92])?;
                if pass == 1 {
                    return Err(serde::ser::Error::custom(
                        "actual partial serializer failure",
                    ));
                }
                seq.serialize_element(&[93_u8, 94])?;
                seq.end()
            }
        }
        let failure = encode_protected_json(&Partial(std::cell::Cell::new(0)), 1024, "partial")
            .expect_err("second pass writes only into protected allocation before failure");
        let AuraError::Serialization {
            source: Some(source),
            ..
        } = failure
        else {
            panic!("partial serializer cause must survive");
        };
        assert!(source.downcast_ref::<serde_json::Error>().is_some());
        assert!(source
            .to_string()
            .contains("actual partial serializer failure"));
    }

    #[test]
    fn private_record_secret_codec_preserves_wire_and_rejects_unbounded_or_malformed_fields() {
        let record = LifetimeRecord {
            version: 1,
            reference: SecretAllocationReference {
                allocation: [7; 32],
                scope: vec![9],
            },
            first_decision: None,
            retired: false,
            secret: Zeroizing::new(vec![11, 12, 13]),
        };
        let original = serde_json::to_vec(&record).expect("historical private JSON");
        assert_eq!(
            encode_protected_json(
                &record,
                MAX_RECORD_BYTES - MAGIC.len() - 28,
                "record fixture"
            )
            .expect("protected private JSON")
            .as_slice(),
            original,
        );
        let mut wire = serde_json::to_value(&record).expect("private wire encoding");
        assert_eq!(wire["secret"], serde_json::json!([11, 12, 13]));
        let decoded: LifetimeRecord =
            serde_json::from_value(wire.clone()).expect("historical byte array");
        assert_eq!(decoded.secret.as_slice(), &[11, 12, 13]);
        wire["secret"] = serde_json::json!([11, 12, "malformed"]);
        assert!(serde_json::from_value::<LifetimeRecord>(wire.clone()).is_err());
        wire["secret"] = serde_json::json!(vec![11_u8; MAX_SECRET_BYTES + 1]);
        let error = serde_json::from_value::<LifetimeRecord>(wire)
            .err()
            .expect("bounded codec failure");
        assert!(error.to_string().contains("secret byte limit exceeded"));
        assert!(
            serde_json::from_str::<LifetimeRecord>(
                "{\"secret\":[11,12,13],\"version\":\"malformed\"}"
            )
            .is_err(),
            "later record failure retains field-owned secret cleanup"
        );
    }
}
impl Drop for LifetimeRecord {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum FirstDecision {
    Positive { bytes: Vec<u8> },
    Negative { bytes: Vec<u8> },
}

struct FilesystemLifetimeRoot {
    provider_identity: SecretLifetimeProviderIdentity,
    migration_birth_digest: [u8; 32],
    migration_handoff_digest: [u8; 32],
    migration_lifecycle_digest: [u8; 32],
    directory: crate::profile_directory::ProfileDirectory,
    provider_directory: crate::profile_directory::ProfileDirectory,
    root_identity: [u8; 32],
    key: Zeroizing<[u8; 32]>,
    gate: Arc<tokio::sync::Mutex<()>>,
    // Exact process-lifetime selected physical owner, not a diagnostic path.
    _profile: Arc<crate::profile_storage::OwnedProfileLease>,
}
struct FilesystemLifetimeAllocation {
    root: Arc<FilesystemLifetimeRoot>,
    reference: SecretAllocationReference,
}
// A local provider type owns the shared root; Arc itself is foreign and cannot
// implement the core backend trait under Rust's orphan rules.
struct FilesystemLifetimeProvider(Arc<FilesystemLifetimeRoot>);
impl std::ops::Deref for FilesystemLifetimeProvider {
    type Target = FilesystemLifetimeRoot;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
fn source_error(
    operation: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> AuraError {
    AuraError::Storage {
        message: format!("owned secret {operation}"),
        source: Some(Arc::new(source)),
    }
}
fn invalid(message: &'static str) -> AuraError {
    AuraError::invalid(message)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl FilesystemLifetimeRoot {
    fn path(reference: &SecretAllocationReference) -> PathBuf {
        PathBuf::from(format!("{}.lifetime", hex(&reference.allocation)))
    }
    fn validate(record: &LifetimeRecord) -> Result<(), AuraError> {
        if record.version != 1
            || record.reference.scope.is_empty()
            || record.reference.scope.len() > MAX_SCOPE_BYTES
        {
            return Err(invalid("invalid original owned secret birth"));
        }
        if record.retired {
            if !record.secret.is_empty()
                || !matches!(record.first_decision, Some(FirstDecision::Negative { .. }))
            {
                return Err(invalid("invalid owned secret retirement tombstone"));
            }
        } else if record.secret.is_empty() || record.secret.len() > MAX_SECRET_BYTES {
            return Err(invalid("invalid original owned secret shape"));
        }
        let decision = match &record.first_decision {
            Some(FirstDecision::Positive { bytes } | FirstDecision::Negative { bytes }) => {
                Some(bytes)
            }
            None => None,
        };
        if decision.is_some_and(|bytes| bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES) {
            return Err(invalid("invalid original owned secret first decision"));
        }
        Ok(())
    }
    fn read(&self, reference: &SecretAllocationReference) -> Result<LifetimeRecord, AuraError> {
        initialization::require_reference(self, reference)?;
        let path = Self::path(reference);
        let bytes = self
            .directory
            .read_bounded(&path, true, MAX_RECORD_BYTES)
            .map_err(|source| source_error("read actual allocation", source))?
            .ok_or_else(initialization::missing_live_allocation)?;
        self.decode(&path, &bytes, Some(reference))
    }
    fn decode(
        &self,
        path: &std::path::Path,
        bytes: &[u8],
        expected: Option<&SecretAllocationReference>,
    ) -> Result<LifetimeRecord, AuraError> {
        if bytes.len() > MAX_RECORD_BYTES
            || !bytes.starts_with(MAGIC)
            || bytes.len() < MAGIC.len() + 12
        {
            return Err(invalid("invalid protected owned secret record"));
        }
        let aad = [
            MAGIC,
            self.root_identity.as_slice(),
            path.as_os_str().as_encoded_bytes(),
        ]
        .concat();
        let cipher = ChaCha20Poly1305::new((&*self.key).into());
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&bytes[MAGIC.len()..MAGIC.len() + 12]),
                    Payload {
                        msg: &bytes[MAGIC.len() + 12..],
                        aad: &aad,
                    },
                )
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "authenticate original owned secret",
                        Arc::new(source),
                    )
                })?,
        );
        let record: LifetimeRecord =
            serde_json::from_slice(&plaintext).map_err(|source| AuraError::Serialization {
                message: "decode original owned secret".into(),
                source: Some(Arc::new(source)),
            })?;
        Self::validate(&record)?;
        if Self::path(&record.reference) != path
            || expected.is_some_and(|expected| expected != &record.reference)
        {
            return Err(invalid("owned secret original allocation binding mismatch"));
        }
        Ok(record)
    }
    fn publish(&self, record: &LifetimeRecord, initial: bool) -> Result<(), AuraError> {
        Self::validate(record)?;
        let path = Self::path(&record.reference);
        let plaintext = encode_protected_json(
            record,
            MAX_RECORD_BYTES - MAGIC.len() - 28,
            "encode owned secret transition",
        )?;
        let mut nonce = [0_u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|source| {
                AuraError::crypto_with_source("owned secret nonce entropy", Arc::new(source))
            })?;
        let aad = [
            MAGIC,
            self.root_identity.as_slice(),
            path.as_os_str().as_encoded_bytes(),
        ]
        .concat();
        let encrypted = ChaCha20Poly1305::new((&*self.key).into())
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source("encrypt owned secret transition", Arc::new(source))
            })?;
        let bytes = [MAGIC, &nonce, &encrypted].concat();
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(invalid("oversized protected secret transition"));
        }
        let prepared = self
            .directory
            .prepare_private(&path, &bytes)
            .map_err(|source| source_error("prepare durable transition", source))?;
        let created = prepared
            .publish(initial)
            .map_err(|source| source_error("publish durable transition", source))?;
        if !created {
            return Err(invalid(
                "fresh owned allocation collides with original birth",
            ));
        }
        // Publication alone is not an ACK. A fault here leaves the complete
        // record and requires exact authenticated recovery, never a new birth.
        #[cfg(test)]
        if with_test_faults(&ACK_FAULTS, |faults| {
            faults.remove(&record.reference.allocation)
        }) {
            return Err(source_error(
                "acknowledge durable transition",
                std::io::Error::other("injected directory fsync unavailable"),
            ));
        }
        prepared
            .acknowledge()
            .map_err(|source| source_error("acknowledge durable transition", source))
    }
}

#[async_trait::async_trait]
impl ProfileSecretLifetimeBackend for FilesystemLifetimeProvider {
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity {
        &self.provider_identity
    }
    async fn allocate(
        &self,
        scope: &[u8],
        secret: &[u8],
    ) -> Result<Arc<dyn SecretLifetimeBackend>, AuraError> {
        let _owner = self.gate.lock().await;
        initialization::complete_pending_birth(self)?;
        let mut allocation = [0_u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut allocation)
            .map_err(|source| {
                AuraError::crypto_with_source("owned allocation identity entropy", Arc::new(source))
            })?;
        let reference = SecretAllocationReference {
            allocation,
            scope: scope.to_vec(),
        };
        let record = LifetimeRecord {
            version: 1,
            reference: reference.clone(),
            first_decision: None,
            retired: false,
            secret: Zeroizing::new(secret.to_vec()),
        };
        initialization::prepare_birth(self, record)?;
        Ok(Arc::new(FilesystemLifetimeAllocation {
            root: self.0.clone(),
            reference,
        }))
    }
    async fn recover_owned_inventory(
        &self,
    ) -> Result<Vec<Arc<dyn SecretLifetimeBackend>>, AuraError> {
        let _owner = self.gate.lock().await;
        let references = initialization::recover_references(self)?;
        let mut owners: Vec<Arc<dyn SecretLifetimeBackend>> = Vec::with_capacity(references.len());
        for reference in references {
            owners.push(Arc::new(FilesystemLifetimeAllocation {
                root: self.0.clone(),
                reference,
            }));
        }
        Ok(owners)
    }
}
#[async_trait::async_trait]
impl SecretLifetimeBackend for FilesystemLifetimeAllocation {
    fn reference(&self) -> &SecretAllocationReference {
        &self.reference
    }
    async fn state(&self) -> Result<SecretLifetimeState, AuraError> {
        let _owner = self.root.gate.lock().await;
        let record = self.root.read(&self.reference)?;
        Ok(match &record.first_decision {
            None => SecretLifetimeState::Live,
            Some(FirstDecision::Positive { bytes }) => SecretLifetimeState::Positive {
                decision: bytes.clone(),
            },
            Some(FirstDecision::Negative { bytes }) if record.retired => {
                SecretLifetimeState::Retired {
                    decision: bytes.clone(),
                }
            }
            Some(FirstDecision::Negative { bytes }) => SecretLifetimeState::Negative {
                decision: bytes.clone(),
            },
        })
    }
    async fn read_live_secret(&self) -> Result<Vec<u8>, AuraError> {
        let _owner = self.root.gate.lock().await;
        let record = self.root.read(&self.reference)?;
        if matches!(record.first_decision, Some(FirstDecision::Negative { .. })) {
            return Err(AuraError::PermissionDenied {
                message: "negative secret lifetime denies further use".into(),
                source: None,
            });
        }
        Ok(record.secret.to_vec())
    }
    async fn decide_positive(&self, decision: &[u8]) -> Result<(), AuraError> {
        let _owner = self.root.gate.lock().await;
        let mut record = self.root.read(&self.reference)?;
        match &record.first_decision {
            Some(FirstDecision::Positive { bytes }) if bytes == decision => {
                return self.root.publish(&record, false);
            }
            Some(_) => {
                return Err(invalid(
                    "original secret first decision contradicts positive publication",
                ))
            }
            None => {}
        }
        record.first_decision = Some(FirstDecision::Positive {
            bytes: decision.to_vec(),
        });
        self.root.publish(&record, false)
    }
    async fn decide_negative(&self, decision: &[u8]) -> Result<(), AuraError> {
        let _owner = self.root.gate.lock().await;
        let mut record = self.root.read(&self.reference)?;
        match &record.first_decision {
            Some(FirstDecision::Negative { bytes }) if bytes == decision => {
                return self.root.publish(&record, false);
            }
            Some(_) => {
                return Err(invalid(
                    "original secret first decision contradicts negative publication",
                ))
            }
            None => {}
        }
        record.first_decision = Some(FirstDecision::Negative {
            bytes: decision.to_vec(),
        });
        self.root.publish(&record, false)
    }
    async fn acknowledge_retirement(&self, decision: &[u8]) -> Result<(), AuraError> {
        let _owner = self.root.gate.lock().await;
        let mut record = self.root.read(&self.reference)?;
        if !matches!(&record.first_decision, Some(FirstDecision::Negative { bytes }) if bytes == decision)
        {
            return Err(invalid(
                "retirement lacks exact original negative first decision",
            ));
        }
        // Even an already published tombstone is re-published/ACKed. Thus an
        // interrupted rename-before-directory-fsync is never treated as ACK.
        record.secret.zeroize();
        record.secret.clear();
        record.retired = true;
        self.root.publish(&record, false)
    }
}

/// Only the consumed selected-profile assembler calls this child-private seam.
/// Caller cannot supply a record selector, wrapping key, or alternate directory.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileSecretLifetimeRecoveryCapability",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(super) fn selected_profile_channel(
    owned: &ProfileOwnedSecureStorage,
) -> Result<ProfileSecretLifetimeRecoveryCapability, AuraError> {
    let ProductionSecureStorageHandler::FilesystemFallback(backend) = owned.backend.as_ref() else {
        return Err(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error());
    };
    let owner = owned._owner.clone();
    let selected = backend
        .owned_directory()
        .map_err(|source| source_error("required selected directory", source))?;
    let expected = owner
        .directory
        .child(
            std::path::Path::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY),
            false,
        )
        .map_err(|source| source_error("required selected provider identity", source))?;
    if !selected
        .same_directory(&expected)
        .map_err(|source| source_error("selected physical owner binding", source))?
    {
        return Err(invalid(
            "secret lifetime channel belongs to another physical profile",
        ));
    }
    owner
        .lifetime_root_claimed
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| invalid("selected profile lifetime recovery already transferred"))?;
    let initialization::InitializedLifetimeRoot {
        directory,
        root_identity,
        migration_birth_digest,
        migration_handoff_digest,
        migration_lifecycle_digest,
    } = initialization::initialize(owned)?;
    let root = Arc::new(FilesystemLifetimeRoot {
        migration_birth_digest,
        migration_handoff_digest,
        migration_lifecycle_digest,
        directory,
        provider_directory: selected.clone(),
        root_identity,
        key: Zeroizing::new(backend.wrapping_key),
        gate: owner.secure_record_gate.clone(),
        provider_identity: owner.lifetime_provider_identity.clone(),
        _profile: owner,
    });
    Ok(
        ProfileSecretLifetimeRecoveryCapability::from_trusted_provider(Box::new(
            FilesystemLifetimeProvider(root),
        )),
    )
}

mod initialization;
pub(crate) use initialization::cutover::CutoverIdentities as InitialCutoverIdentities;
pub(crate) use initialization::VerifiedOriginalInitializationSuccessor;
#[cfg(test)]
pub(crate) fn initialization_cutover_checkpoint(stage: &str) -> std::io::Result<()> {
    initialization::prelink_process_checkpoint(&format!("cutover:{stage}"))
        .map_err(std::io::Error::other)
}

#[cfg(test)]
static ACK_FAULTS: std::sync::LazyLock<tokio::sync::Mutex<std::collections::BTreeSet<[u8; 32]>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::BTreeSet::new()));

/// Run `access` on a test-only fault registry. Holders only insert or remove
/// one entry and never await, so a parallel test briefly contending for the
/// lock is retried synchronously instead of failing the test.
#[cfg(test)]
pub(super) fn with_test_faults<T: Ord, R>(
    registry: &tokio::sync::Mutex<std::collections::BTreeSet<T>>,
    access: impl FnOnce(&mut std::collections::BTreeSet<T>) -> R,
) -> R {
    loop {
        if let Ok(mut faults) = registry.try_lock() {
            return access(&mut faults);
        }
        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selected(
        path: &std::path::Path,
    ) -> Result<ProductionSecureStorageHandler, Box<dyn std::error::Error>> {
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.to_path_buf())
                .acquire_owned_native()?,
        );
        Ok(ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)?)
    }
    #[tokio::test]
    async fn allocation_lifetime_reopens_original_negative_tombstone(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (storage, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        assert!(root.recover_owned_inventory().await?.is_empty());
        let original = root
            .allocate(b"held-original-generation", b"original-secret")
            .await?;
        let original_ref = original.reference().clone();
        let negative = original
            .decide_negative(b"original-failed-first-decision")
            .await?;
        assert!(original.read_live_secret().await.is_err());
        let acknowledged = negative.retire().await?;
        assert_eq!(acknowledged.reference(), &original_ref);
        drop(acknowledged);
        drop(negative);
        drop(original);
        drop(root);
        drop(storage);
        let (storage, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        let owners = root.recover_owned_inventory().await?;
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].reference(), &original_ref);
        assert!(
            matches!(owners[0].state().await?,SecretLifetimeState::Retired {decision} if decision==b"original-failed-first-decision")
        );
        assert!(owners[0].read_live_secret().await.is_err());
        assert!(owners[0].decide_positive(b"later-positive").await.is_err());
        owners[0]
            .decide_negative(b"original-failed-first-decision")
            .await?
            .retire()
            .await?;
        let reissue = root
            .allocate(b"held-reissued-generation", b"new-secret")
            .await?;
        assert_ne!(reissue.reference().allocation, original_ref.allocation);
        assert_eq!(reissue.read_live_secret().await?, b"new-secret");
        drop(storage);
        Ok(())
    }
    #[tokio::test]
    async fn positive_secret_birth_cannot_be_reclassified_negative(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let (_storage, mut root) =
            selected(profile.path())?.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let original = root
            .allocate(b"actual-active-original", b"active-secret")
            .await?;
        original
            .decide_positive(b"verified-original-activation")
            .await?;
        original
            .decide_positive(b"verified-original-activation")
            .await?;
        assert!(original.decide_negative(b"later-cancel").await.is_err());
        assert_eq!(original.read_live_secret().await?, b"active-secret");
        Ok(())
    }
    #[tokio::test]
    async fn selected_physical_lease_issues_only_one_root_and_denies_generic_namespace(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(
                profile.path().to_path_buf(),
            )
            .acquire_owned_native()?,
        );
        let first =
            ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner.clone())?;
        let (ordinary, _root) = first.into_selected_profile_lifetime_channel()?;
        let second = ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)?;
        assert!(second.into_selected_profile_lifetime_channel().is_err());
        let location = SecureStorageLocation::new(DIRECTORY, "anything");
        assert!(ordinary
            .secure_list_keys(DIRECTORY, &[SecureStorageCapability::List])
            .await
            .is_err());
        assert!(ordinary
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .is_err());
        assert!(ordinary
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .is_err());
        assert!(ordinary
            .secure_store(&location, b"replacement", &[SecureStorageCapability::Write])
            .await
            .is_err());
        Ok(())
    }
}
#[cfg(test)]
mod ack_tests {
    use super::*;
    #[tokio::test]
    async fn required_negative_ack_failure_retains_io_and_retry_acknowledges_original(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::error::Error;
        let profile = tempfile::tempdir()?;
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(
                profile.path().to_path_buf(),
            )
            .acquire_owned_native()?,
        );
        let storage =
            ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)?;
        let (_storage, mut root) = storage.into_selected_profile_lifetime_channel()?;
        root.recover_owned_inventory().await?;
        let original = root
            .allocate(b"original-ACK-generation", b"real-secret")
            .await?;
        with_test_faults(&ACK_FAULTS, |faults| {
            faults.insert(original.reference().allocation)
        });
        let error = match original.decide_negative(b"original-negative").await {
            Err(error) => error,
            Ok(_) => panic!("unacknowledged publication must not mint proof"),
        };
        assert!(error
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .is_some());
        assert!(
            matches!(original.state().await?,SecretLifetimeState::Negative {decision} if decision==b"original-negative")
        );
        // A second ACK fault must fail even for exact already-published state.
        with_test_faults(&ACK_FAULTS, |faults| {
            faults.insert(original.reference().allocation)
        });
        assert!(original
            .decide_negative(b"original-negative")
            .await
            .is_err());
        let negative = original.decide_negative(b"original-negative").await?;
        with_test_faults(&ACK_FAULTS, |faults| {
            faults.insert(original.reference().allocation)
        });
        assert!(negative.retire().await.is_err());
        // Tombstone existence does not substitute for required ACK on replay.
        with_test_faults(&ACK_FAULTS, |faults| {
            faults.insert(original.reference().allocation)
        });
        assert!(negative.retire().await.is_err());
        negative.retire().await?;
        Ok(())
    }
}

#[cfg(test)]
#[tokio::test]
async fn original_owner_missing_leaf_retains_required_storage_absence(
) -> Result<(), Box<dyn std::error::Error>> {
    let profile = tempfile::tempdir()?;
    let owner = Arc::new(
        crate::profile_storage::FilesystemProfileStorageHandler::new(profile.path().to_path_buf())
            .acquire_owned_native()?,
    );
    let (ordinary, mut root) =
        ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(owner)?
            .into_selected_profile_lifetime_channel()?;
    root.recover_owned_inventory().await?;
    let original = root.allocate(b"actual-original", b"secret").await?;
    let ProductionSecureStorageHandler::ProfileOwned(owned) = &ordinary else {
        panic!("actual selected owner");
    };
    let ProductionSecureStorageHandler::FilesystemFallback(backend) = owned.backend.as_ref() else {
        panic!("actual descriptor provider");
    };
    let directory = backend
        .owned_directory()?
        .child(std::path::Path::new(DIRECTORY), false)?;
    directory.remove(&FilesystemLifetimeRoot::path(original.reference()))?;
    let failure = original
        .read_live_secret()
        .await
        .expect_err("actual owned allocation missing after birth");
    assert!(matches!(failure, AuraError::Storage { .. }));
    assert!(matches!(
        std::error::Error::source(&failure).and_then(|source| {
            source.downcast_ref::<initialization::AllocationLifetimeRecoveryError>()
        }),
        Some(initialization::AllocationLifetimeRecoveryError::LiveAllocation)
    ));
    Ok(())
}
