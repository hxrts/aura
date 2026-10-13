//! Browser selected-profile lifetime custody. One protected IndexedDB record is
//! committed with strict, raw-ciphertext CAS before any allocation is handed out.
use super::*;
use aura_core::effects::secret_lifetime::*;
use aura_core::AuraError;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

const NAMESPACE: &str = ".allocation-lifetimes-v1";
const INVENTORY_KEY: &str = "selected-profile-inventory";
const MAX_INVENTORY_BYTES: usize = 24 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    version: u16,
    records: Vec<Record>,
}
// aura-security: secret-derive-justified owner=allocation-lifetime-provider expires=before-release remediation=docs/100_crypto.md private browser persistence codec only; plaintext and encoded buffers are Zeroizing, IndexedDB stores only authenticated ciphertext, Record Drop zeroizes secret bytes, and no Debug/Clone or public export is provided.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    reference: SecretAllocationReference,
    decision: Option<Decision>,
    retired: bool,
    #[serde(with = "secret_codec")]
    secret: Zeroizing<Vec<u8>>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Decision {
    Positive { bytes: Vec<u8> },
    Negative { bytes: Vec<u8> },
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
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Zeroizing<Vec<u8>>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bounded secret bytes")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_SECRET_BYTES));
                while let Some(byte) = seq.next_element::<u8>()? {
                    if bytes.len() == MAX_SECRET_BYTES {
                        return Err(serde::de::Error::custom("secret limit exceeded"));
                    }
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

fn invalid(message: &'static str) -> AuraError {
    AuraError::invalid(message)
}
fn encode(inventory: &Inventory) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    // Count before allocating, then serialize into a fixed zeroizing buffer.
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= MAX_INVENTORY_BYTES)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "inventory limit exceeded",
                    )
                })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, inventory).map_err(|source| AuraError::Serialization {
        message: "count browser secret inventory".into(),
        source: Some(Arc::new(source)),
    })?;
    let mut bytes = Zeroizing::new(vec![0; count.0]);
    let mut writer = std::io::Cursor::new(bytes.as_mut_slice());
    serde_json::to_writer(&mut writer, inventory).map_err(|source| AuraError::Serialization {
        message: "encode browser secret inventory".into(),
        source: Some(Arc::new(source)),
    })?;
    if writer.position() != count.0 as u64 {
        return Err(invalid("browser secret inventory encoding changed length"));
    }
    Ok(bytes)
}
fn validate(inventory: &Inventory) -> Result<(), AuraError> {
    if inventory.version != 1 || inventory.records.len() > MAX_PROFILE_ALLOCATION_COUNT {
        return Err(invalid("invalid browser secret inventory header"));
    }
    let mut identities = BTreeSet::new();
    for record in &inventory.records {
        if !identities.insert(record.reference.allocation)
            || record.reference.scope.is_empty()
            || record.reference.scope.len() > MAX_SCOPE_BYTES
            || (record.retired
                && (!record.secret.is_empty()
                    || !matches!(record.decision, Some(Decision::Negative { .. }))))
            || (!record.retired
                && (record.secret.is_empty() || record.secret.len() > MAX_SECRET_BYTES))
        {
            return Err(invalid("invalid browser secret allocation"));
        }
        let decision = match &record.decision {
            Some(Decision::Positive { bytes } | Decision::Negative { bytes }) => Some(bytes),
            None => None,
        };
        if decision.is_some_and(|bytes| bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES) {
            return Err(invalid("invalid browser secret first decision"));
        }
    }
    Ok(())
}

struct Root {
    backend: Arc<ProductionSecureStorageHandler>,
    owner: Arc<crate::profile_storage::OwnedProfileLease>,
}
impl Root {
    fn selected_backend(&self) -> &FilesystemFallbackSecureStorageHandler {
        // Checked once at construction; the private enum cannot mutate.
        let ProductionSecureStorageHandler::FilesystemFallback(backend) = self.backend.as_ref()
        else {
            unreachable!("selected browser backend changed variant")
        };
        backend
    }
    fn location() -> SecureStorageLocation {
        SecureStorageLocation::new(NAMESPACE, INVENTORY_KEY)
    }
    async fn read(&self) -> Result<(Option<Vec<u8>>, Inventory), AuraError> {
        let location = Self::location();
        let original = self.selected_backend().wasm_get_record(&location).await?;
        let inventory = if let Some(bytes) = original.as_deref() {
            if bytes.len() > MAX_INVENTORY_BYTES + 1024 {
                return Err(invalid("oversized protected browser secret inventory"));
            }
            let (plaintext, protected) = self
                .selected_backend()
                .wasm_decrypt_record_with_protection(&location, bytes)
                .await?;
            let plaintext = Zeroizing::new(plaintext);
            if !protected || plaintext.len() > MAX_INVENTORY_BYTES {
                return Err(invalid("unprotected browser secret inventory"));
            }
            serde_json::from_slice(&plaintext).map_err(|source| AuraError::Serialization {
                message: "decode browser secret inventory".into(),
                source: Some(Arc::new(source)),
            })?
        } else {
            Inventory {
                version: 1,
                records: Vec::new(),
            }
        };
        validate(&inventory)?;
        Ok((original, inventory))
    }
    async fn publish(
        &self,
        original: Option<&[u8]>,
        inventory: &Inventory,
    ) -> Result<(), AuraError> {
        validate(inventory)?;
        let location = Self::location();
        let plaintext = encode(inventory)?;
        let sealed = self
            .selected_backend()
            .wasm_encrypt_record_with_protection(&location, &plaintext, true)
            .await?;
        self.selected_backend()
            .wasm_compare_exchange_record(&location, original, Some(&sealed))
            .await
    }
    fn find<'a>(
        inventory: &'a Inventory,
        reference: &SecretAllocationReference,
    ) -> Result<&'a Record, AuraError> {
        inventory
            .records
            .iter()
            .find(|record| record.reference == *reference)
            .ok_or_else(|| invalid("original browser secret allocation missing"))
    }
    fn find_mut<'a>(
        inventory: &'a mut Inventory,
        reference: &SecretAllocationReference,
    ) -> Result<&'a mut Record, AuraError> {
        inventory
            .records
            .iter_mut()
            .find(|record| record.reference == *reference)
            .ok_or_else(|| invalid("original browser secret allocation missing"))
    }
}
struct Provider(Arc<Root>);
struct Allocation {
    root: Arc<Root>,
    reference: SecretAllocationReference,
}
#[async_trait(?Send)]
impl ProfileSecretLifetimeBackend for Provider {
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity {
        &self.0.owner.lifetime_provider_identity
    }
    async fn allocate(
        &self,
        scope: &[u8],
        secret: &[u8],
    ) -> Result<Arc<dyn SecretLifetimeBackend>, AuraError> {
        if scope.is_empty()
            || scope.len() > MAX_SCOPE_BYTES
            || secret.is_empty()
            || secret.len() > MAX_SECRET_BYTES
        {
            return Err(invalid("invalid browser secret birth shape"));
        }
        let _gate = self.0.owner.secure_record_gate.lock().await;
        let (original, mut inventory) = self.0.read().await?;
        if inventory.records.len() == MAX_PROFILE_ALLOCATION_COUNT {
            return Err(invalid("browser secret inventory full"));
        }
        let mut identity = [0u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut identity)
            .map_err(|source| {
                AuraError::crypto_with_source("browser secret allocation entropy", Arc::new(source))
            })?;
        if inventory
            .records
            .iter()
            .any(|record| record.reference.allocation == identity)
        {
            return Err(invalid("duplicate browser secret allocation"));
        }
        let reference = SecretAllocationReference {
            allocation: identity,
            scope: scope.to_vec(),
        };
        inventory.records.push(Record {
            reference: reference.clone(),
            decision: None,
            retired: false,
            secret: Zeroizing::new(secret.to_vec()),
        });
        self.0.publish(original.as_deref(), &inventory).await?;
        Ok(Arc::new(Allocation {
            root: self.0.clone(),
            reference,
        }))
    }
    async fn recover_owned_inventory(
        &self,
    ) -> Result<Vec<Arc<dyn SecretLifetimeBackend>>, AuraError> {
        let _gate = self.0.owner.secure_record_gate.lock().await;
        let (_, inventory) = self.0.read().await?;
        Ok(inventory
            .records
            .into_iter()
            .map(|record| {
                Arc::new(Allocation {
                    root: self.0.clone(),
                    reference: record.reference.clone(),
                }) as Arc<dyn SecretLifetimeBackend>
            })
            .collect())
    }
}
#[async_trait(?Send)]
impl SecretLifetimeBackend for Allocation {
    fn reference(&self) -> &SecretAllocationReference {
        &self.reference
    }
    async fn state(&self) -> Result<SecretLifetimeState, AuraError> {
        let _gate = self.root.owner.secure_record_gate.lock().await;
        let (_, inventory) = self.root.read().await?;
        let record = Root::find(&inventory, &self.reference)?;
        Ok(match &record.decision {
            None => SecretLifetimeState::Live,
            Some(Decision::Positive { bytes }) => SecretLifetimeState::Positive {
                decision: bytes.clone(),
            },
            Some(Decision::Negative { bytes }) if record.retired => SecretLifetimeState::Retired {
                decision: bytes.clone(),
            },
            Some(Decision::Negative { bytes }) => SecretLifetimeState::Negative {
                decision: bytes.clone(),
            },
        })
    }
    async fn read_live_secret(&self) -> Result<Vec<u8>, AuraError> {
        let _gate = self.root.owner.secure_record_gate.lock().await;
        let (_, inventory) = self.root.read().await?;
        let record = Root::find(&inventory, &self.reference)?;
        if matches!(record.decision, Some(Decision::Negative { .. })) {
            return Err(AuraError::PermissionDenied {
                message: "negative browser secret lifetime denies use".into(),
                source: None,
            });
        }
        Ok(record.secret.to_vec())
    }
    async fn decide_positive(&self, decision: &[u8]) -> Result<(), AuraError> {
        self.decide(decision, true).await
    }
    async fn decide_negative(&self, decision: &[u8]) -> Result<(), AuraError> {
        self.decide(decision, false).await
    }
    async fn acknowledge_retirement(&self, decision: &[u8]) -> Result<(), AuraError> {
        let _gate = self.root.owner.secure_record_gate.lock().await;
        let (original, mut inventory) = self.root.read().await?;
        let record = Root::find_mut(&mut inventory, &self.reference)?;
        if !matches!(&record.decision, Some(Decision::Negative { bytes }) if bytes == decision) {
            return Err(invalid(
                "browser retirement lacks original negative decision",
            ));
        }
        record.secret.zeroize();
        record.secret.clear();
        record.retired = true;
        self.root.publish(original.as_deref(), &inventory).await
    }
}
impl Allocation {
    async fn decide(&self, decision: &[u8], positive: bool) -> Result<(), AuraError> {
        if decision.is_empty() || decision.len() > MAX_DECISION_BYTES {
            return Err(invalid("invalid browser first decision"));
        }
        let _gate = self.root.owner.secure_record_gate.lock().await;
        let (original, mut inventory) = self.root.read().await?;
        let record = Root::find_mut(&mut inventory, &self.reference)?;
        match &record.decision {
            Some(Decision::Positive { bytes }) if positive && bytes == decision => {}
            Some(Decision::Negative { bytes }) if !positive && bytes == decision => {}
            Some(_) => return Err(invalid("contradictory browser secret first decision")),
            None => {
                record.decision = Some(if positive {
                    Decision::Positive {
                        bytes: decision.to_vec(),
                    }
                } else {
                    Decision::Negative {
                        bytes: decision.to_vec(),
                    }
                })
            }
        }
        self.root.publish(original.as_deref(), &inventory).await
    }
}

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
        return Err(
            SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error(),
        );
    };
    if !backend
        .base_path
        .parent()
        .is_some_and(|profile| owned._owner.matches_profile(profile).unwrap_or(false))
    {
        return Err(invalid(
            "browser secret lifetime provider differs from selected profile",
        ));
    }
    FilesystemFallbackSecureStorageHandler::require_strict_idb_durability()?;
    owned
        ._owner
        .lifetime_root_claimed
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| invalid("selected browser lifetime recovery already transferred"))?;
    Ok(
        ProfileSecretLifetimeRecoveryCapability::from_trusted_provider(Box::new(Provider(
            Arc::new(Root {
                backend: owned.backend.clone(),
                owner: owned._owner.clone(),
            }),
        ))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_storage::FilesystemProfileStorageHandler;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test(async)]
    async fn stale_authenticated_inventory_cannot_overwrite_committed_birth() {
        let profile = format!("test/browser-lifetime-cas-{}", js_sys::Math::random());
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(profile.clone().into())
                .acquire_owned_browser()
                .await
                .expect("actual Web Lock"),
        );
        let secure =
            ProductionSecureStorageHandler::for_production(std::path::PathBuf::from(&profile))
                .retain_profile_owner(owner.clone())
                .expect("selected profile");
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &secure else {
            unreachable!()
        };
        let root = Root {
            backend: owned.backend.clone(),
            owner,
        };
        let (original, mut first) = root.read().await.expect("initial inventory");
        let mut second = Inventory {
            version: 1,
            records: Vec::new(),
        };
        let record = |byte: u8| Record {
            reference: SecretAllocationReference {
                allocation: [byte; 32],
                scope: b"rotation".to_vec(),
            },
            decision: None,
            retired: false,
            secret: Zeroizing::new(vec![byte]),
        };
        first.records.push(record(1));
        second.records.push(record(2));
        root.publish(original.as_deref(), &first)
            .await
            .expect("first durable CAS");
        assert!(
            root.publish(original.as_deref(), &second).await.is_err(),
            "stale encrypted inventory must conflict"
        );
        let (_, committed) = root.read().await.expect("committed inventory");
        assert_eq!(committed.records.len(), 1);
        assert_eq!(committed.records[0].reference.allocation, [1; 32]);
    }
}
