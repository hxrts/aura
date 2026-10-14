//! One original platform allocation birth under the selected namespace.
//! This remains private until decision, retirement and whole-profile inventory
//! support are complete. In particular a second birth is refused.

use super::*;
use aura_core::effects::secret_lifetime::{MAX_SCOPE_BYTES, MAX_SECRET_BYTES};
use zeroize::Zeroizing;

pub(super) const INTENT_USER: &str = "allocation-lifetime-birth-intent:v1";
pub(super) const RECORD_USER: &str = "allocation-lifetime-birth-record:v1";
mod decision;
const INVENTORY_FILE: &str = "platform_lifetime/inventory.v1";
const INVENTORY_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-INVENTORY-V1";
const INTENT_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-BIRTH-V1";
const MAX_INVENTORY_FILE: usize = 256;
const MAX_INTENT: usize =
    INTENT_MAGIC.len() + 12 + 16 + 64 + 4 + MAX_SCOPE_BYTES + MAX_SECRET_BYTES;

struct BirthIntent {
    root: [u8; 32],
    allocation: [u8; 32],
    scope: Vec<u8>,
    secret: Zeroizing<Vec<u8>>,
}

impl BirthIntent {
    fn encode(&self, selected: &PlatformSecureStorageHandler) -> Result<Vec<u8>, AuraError> {
        let key = Zeroizing::new(selected.record_key(false)?);
        let mut plain = Zeroizing::new(Vec::with_capacity(
            68 + self.scope.len() + self.secret.len(),
        ));
        plain.extend_from_slice(&self.root);
        plain.extend_from_slice(&self.allocation);
        plain.extend_from_slice(&(self.scope.len() as u16).to_le_bytes());
        plain.extend_from_slice(&(self.secret.len() as u16).to_le_bytes());
        plain.extend_from_slice(&self.scope);
        plain.extend_from_slice(&self.secret);
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|source| {
                AuraError::crypto_with_source("platform birth entropy", Arc::new(source))
            })?;
        let ciphertext = ChaCha20Poly1305::new((&*key).into())
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: INTENT_MAGIC,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source("platform birth encryption", Arc::new(source))
            })?;
        let mut bytes = Vec::with_capacity(INTENT_MAGIC.len() + 12 + ciphertext.len());
        bytes.extend_from_slice(INTENT_MAGIC);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&ciphertext);
        Ok(bytes)
    }

    fn decode(selected: &PlatformSecureStorageHandler, bytes: &[u8]) -> Result<Self, AuraError> {
        if bytes.len() > MAX_INTENT
            || !bytes.starts_with(INTENT_MAGIC)
            || bytes.len() < INTENT_MAGIC.len() + 12 + 16 + 68
        {
            return Err(invalid("platform birth intent encoding is invalid"));
        }
        let key = Zeroizing::new(selected.record_key(false)?);
        let nonce = &bytes[INTENT_MAGIC.len()..INTENT_MAGIC.len() + 12];
        let mut plain = Zeroizing::new(
            ChaCha20Poly1305::new((&*key).into())
                .decrypt(
                    Nonce::from_slice(nonce),
                    Payload {
                        msg: &bytes[INTENT_MAGIC.len() + 12..],
                        aad: INTENT_MAGIC,
                    },
                )
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "platform birth intent authentication failed",
                        Arc::new(source),
                    )
                })?,
        );
        if plain.len() < 68 {
            return Err(invalid("platform birth intent is truncated"));
        }
        let scope_len = u16::from_le_bytes([plain[64], plain[65]]) as usize;
        let secret_len = u16::from_le_bytes([plain[66], plain[67]]) as usize;
        if scope_len == 0
            || scope_len > MAX_SCOPE_BYTES
            || secret_len == 0
            || secret_len > MAX_SECRET_BYTES
            || plain.len() != 68 + scope_len + secret_len
        {
            return Err(invalid("platform birth intent lengths are invalid"));
        }
        let secret = Zeroizing::new(plain.split_off(68 + scope_len));
        Ok(Self {
            root: plain[0..32].try_into().expect("bounded root"),
            allocation: plain[32..64].try_into().expect("bounded allocation"),
            scope: plain[68..68 + scope_len].to_vec(),
            secret,
        })
    }
}

fn inventory_marker(root: &RootRecord) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INVENTORY_MAGIC.len() + 160);
    bytes.extend_from_slice(INVENTORY_MAGIC);
    bytes.extend_from_slice(&root.id);
    bytes.extend_from_slice(&root.birth_id);
    bytes.extend_from_slice(&root.intent_digest);
    bytes.extend_from_slice(&root.profile);
    bytes.extend_from_slice(&root.service);
    bytes
}

fn read_entry(
    selected: &PlatformSecureStorageHandler,
    user: &str,
) -> Result<Option<Vec<u8>>, AuraError> {
    match selected.entry_for_user(user)?.get_secret() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(source) => Err(PlatformSecureStorageHandler::map_keyring_error(source)),
    }
}

fn write_exact(
    selected: &PlatformSecureStorageHandler,
    user: &str,
    bytes: &[u8],
) -> Result<(), AuraError> {
    let entry = selected.entry_for_user(user)?;
    entry
        .set_secret(bytes)
        .map_err(PlatformSecureStorageHandler::map_keyring_error)?;
    if entry
        .get_secret()
        .map_err(PlatformSecureStorageHandler::map_keyring_error)?
        != bytes
    {
        return Err(invalid(
            "platform birth credential write was not acknowledged",
        ));
    }
    Ok(())
}

fn selected_owner(
    owned: &ProfileOwnedSecureStorage,
) -> Result<
    (
        &PlatformSecureStorageHandler,
        &Arc<crate::profile_storage::OwnedProfileLease>,
    ),
    AuraError,
> {
    let ProductionSecureStorageHandler::Platform(selected) = owned.backend.as_ref() else {
        return Err(invalid(
            "platform birth requires original selected credential provider",
        ));
    };
    let namespace = selected
        .namespace_owner
        .as_ref()
        .ok_or_else(|| invalid("platform birth lacks namespace owner"))?;
    if !owned
        ._owner
        .keyring_namespace
        .get()
        .is_some_and(|original| Arc::ptr_eq(original, namespace))
    {
        return Err(invalid("platform birth belongs to another namespace owner"));
    }
    Ok((selected, &owned._owner))
}

fn read_root(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
) -> Result<RootRecord, AuraError> {
    let bytes = read_entry(selected, KEYRING_USER)?
        .ok_or_else(|| invalid("platform birth root is missing"))?;
    let root = RootRecord::decode(selected, &bytes)?;
    if root.profile != aura_core::hash::hash(owned._owner.profile_identity().as_bytes())
        || root.service != aura_core::hash::hash(selected.service.as_bytes())
    {
        return Err(invalid(
            "platform birth root belongs to another profile or provider",
        ));
    }
    let marker = owned
        ._owner
        .directory
        .read_private_publication(Path::new(FILE_PATH), MAX_FILE_LEN)
        .map_err(io)?;
    if !matches!(marker, Some((ref bytes, None)) if *bytes == root.file_marker()) {
        return Err(invalid(
            "platform birth original root marker is not acknowledged",
        ));
    }
    Ok(root)
}

fn read_intent(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<(Vec<u8>, BirthIntent), AuraError> {
    let bytes = read_entry(selected, INTENT_USER)?
        .ok_or_else(|| invalid("platform birth original intent is missing"))?;
    if aura_core::hash::hash(&bytes) != root.intent_digest {
        return Err(invalid("platform birth intent differs from original root"));
    }
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.root != root.id || intent.allocation != root.birth_id {
        return Err(invalid(
            "platform birth intent has foreign original identity",
        ));
    }
    Ok((bytes, intent))
}

fn verify_inventory(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
) -> Result<Option<()>, AuraError> {
    let observed = owned
        ._owner
        .directory
        .read_private_publication(Path::new(INVENTORY_FILE), MAX_INVENTORY_FILE)
        .map_err(io)?;
    match observed {
        None => Ok(None),
        Some((bytes, None)) if bytes == inventory_marker(root) => Ok(Some(())),
        _ => Err(invalid(
            "platform birth inventory differs from original intent",
        )),
    }
}

fn publish_inventory(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(INVENTORY_FILE), &inventory_marker(root))
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform birth inventory publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

/// Private original birth evidence. No first-decision or retirement methods
/// are issued by this dependency.
#[allow(dead_code)]
pub(super) struct PlatformBirthAdmission {
    root: [u8; 32],
    allocation: [u8; 32],
    _owner: Arc<crate::profile_storage::OwnedProfileLease>,
}

fn verify_birth<'a>(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    birth: &PlatformBirthAdmission,
) -> Result<(RootRecord, BirthIntent), AuraError> {
    if !Arc::ptr_eq(&birth._owner, &owned._owner) {
        return Err(invalid("platform birth belongs to another profile owner"));
    }
    let root = read_root(owned, selected)?;
    if !matches!(
        root.phase,
        BIRTH_COMPLETE | DECISION_PENDING | DECISION_COMPLETE
    ) || root.id != birth.root
        || root.birth_id != birth.allocation
    {
        return Err(invalid(
            "platform birth owner differs from original allocation",
        ));
    }
    if verify_inventory(owned, &root)?.is_none() {
        return Err(invalid("platform live inventory is missing"));
    }
    let (bytes, intent) = read_intent(selected, &root)?;
    if read_entry(selected, RECORD_USER)?.as_deref() != Some(bytes.as_slice()) {
        return Err(invalid(
            "platform live credential differs from original birth",
        ));
    }
    Ok((root, intent))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootIntent,
    IntentCredential,
    Inventory,
    SecretCredential,
}

fn interrupted() -> AuraError {
    invalid("platform birth interrupted at fixture boundary")
}

#[allow(dead_code)]
pub(super) async fn allocate_one(
    owned: &ProfileOwnedSecureStorage,
    admission: &EmptyPlatformLifetimeRootAdmission,
    scope: &[u8],
    secret: &[u8],
) -> Result<PlatformBirthAdmission, AuraError> {
    allocate_one_inner(owned, admission, scope, secret, StopAfter::Never).await
}

async fn allocate_one_inner(
    owned: &ProfileOwnedSecureStorage,
    admission: &EmptyPlatformLifetimeRootAdmission,
    scope: &[u8],
    secret: &[u8],
    stop: StopAfter,
) -> Result<PlatformBirthAdmission, AuraError> {
    if scope.is_empty()
        || scope.len() > MAX_SCOPE_BYTES
        || secret.is_empty()
        || secret.len() > MAX_SECRET_BYTES
    {
        return Err(invalid(
            "platform birth scope or secret exceeds original bound",
        ));
    }
    let (selected, _) = selected_owner(owned)?;
    if !Arc::ptr_eq(&admission._owner, &owned._owner) {
        return Err(invalid(
            "platform birth token belongs to another profile owner",
        ));
    }
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = read_root(owned, selected)?;
    if root.id != admission._id || root.phase != COMPLETE {
        return Err(invalid("platform birth lacks original empty root"));
    }
    if verify_inventory(owned, &root)?.is_some()
        || read_entry(selected, INTENT_USER)?.is_some()
        || read_entry(selected, RECORD_USER)?.is_some()
    {
        return Err(invalid(
            "platform birth found an unexplained existing allocation",
        ));
    }
    let mut allocation = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut allocation)
        .map_err(|source| {
            AuraError::crypto_with_source("platform allocation identity entropy", Arc::new(source))
        })?;
    let intent = BirthIntent {
        root: root.id,
        allocation,
        scope: scope.to_vec(),
        secret: Zeroizing::new(secret.to_vec()),
    };
    let bytes = intent.encode(selected)?;
    root.phase = BIRTH_PENDING;
    root.birth_id = allocation;
    root.intent_digest = aura_core::hash::hash(&bytes);
    write_keyring(selected, &root)?;
    if stop == StopAfter::RootIntent {
        return Err(interrupted());
    }
    write_exact(selected, INTENT_USER, &bytes)?;
    if stop == StopAfter::IntentCredential {
        return Err(interrupted());
    }
    publish_inventory(owned, &root)?;
    if stop == StopAfter::Inventory {
        return Err(interrupted());
    }
    write_exact(selected, RECORD_USER, &bytes)?;
    if stop == StopAfter::SecretCredential {
        return Err(interrupted());
    }
    root.phase = BIRTH_COMPLETE;
    write_keyring(selected, &root)?;
    admission
        .birth_handed_out
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("platform birth already transferred from original root"))?;
    Ok(PlatformBirthAdmission {
        root: root.id,
        allocation,
        _owner: owned._owner.clone(),
    })
}

/// Recover the one original birth from the same profile and credential owner.
/// A root intent without its exact credential is a typed conflict, never a
/// license to generate replacement identity or secret bytes.
#[allow(dead_code)]
pub(super) async fn recover_one(
    owned: &ProfileOwnedSecureStorage,
) -> Result<PlatformBirthAdmission, AuraError> {
    recover_one_inner(owned, None).await
}

/// Reconcile a failed birth while the original, already transferred empty-root
/// owner is still alive. This is not a second root handoff.
#[allow(dead_code)]
pub(super) async fn reconcile_one(
    owned: &ProfileOwnedSecureStorage,
    original: &EmptyPlatformLifetimeRootAdmission,
) -> Result<PlatformBirthAdmission, AuraError> {
    recover_one_inner(owned, Some(original)).await
}

async fn recover_one_inner(
    owned: &ProfileOwnedSecureStorage,
    original_root: Option<&EmptyPlatformLifetimeRootAdmission>,
) -> Result<PlatformBirthAdmission, AuraError> {
    let (selected, owner) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = read_root(owned, selected)?;
    if !matches!(
        root.phase,
        BIRTH_PENDING | BIRTH_COMPLETE | DECISION_PENDING | DECISION_COMPLETE
    ) {
        return Err(invalid("platform allocation birth has not started"));
    }
    if let Some(original) = original_root {
        if root.id != original._id
            || !Arc::ptr_eq(&original._owner, owner)
            || !owner.lifetime_root_claimed.load(Ordering::Acquire)
            || original.birth_handed_out.load(Ordering::Acquire)
        {
            return Err(invalid(
                "platform birth reconciliation lacks original root owner",
            ));
        }
    }
    let (bytes, _intent) = read_intent(selected, &root)?;
    match verify_inventory(owned, &root)? {
        Some(()) => {}
        None if root.phase == BIRTH_PENDING => publish_inventory(owned, &root)?,
        None => return Err(invalid("completed platform birth inventory is missing")),
    }
    match read_entry(selected, RECORD_USER)? {
        Some(observed) if observed == bytes => {}
        None if root.phase == BIRTH_PENDING => write_exact(selected, RECORD_USER, &bytes)?,
        _ => return Err(invalid("platform birth credential differs or is missing")),
    }
    if root.phase == BIRTH_PENDING {
        root.phase = BIRTH_COMPLETE;
        write_keyring(selected, &root)?;
    }
    decision::validate_recovery(owned, selected, &root)?;
    if original_root.is_none() {
        owner
            .lifetime_root_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| invalid("platform lifetime root already transferred"))?;
    } else if let Some(original) = original_root {
        original
            .birth_handed_out
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| invalid("platform birth already transferred from original root"))?;
    }
    Ok(PlatformBirthAdmission {
        root: root.id,
        allocation: root.birth_id,
        _owner: owner.clone(),
    })
}

#[allow(dead_code)]
pub(super) async fn read_live_secret(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, intent) = verify_birth(owned, selected, birth)?;
    decision::require_live_read(owned, selected, &root)?;
    Ok(intent.secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn one_birth_reads_exact_secret_and_reopens_under_selected_owner() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"original scope", b"original secret").await?;
        assert_eq!(
            &**read_live_secret(owned, &birth).await?,
            b"original secret"
        );
        assert!(allocate_one(owned, &empty, b"second", b"secret")
            .await
            .is_err());
        let original_allocation = birth.allocation;
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let recovered = recover_one(owned).await?;
        assert_eq!(recovered.allocation, original_allocation);
        assert_eq!(
            &**read_live_secret(owned, &recovered).await?,
            b"original secret"
        );
        assert!(recover_one(owned).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_birth_recovers_only_original_intent() -> TestResult {
        for stop in [
            StopAfter::IntentCredential,
            StopAfter::Inventory,
            StopAfter::SecretCredential,
        ] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            assert!(allocate_one_inner(owned, &empty, b"scope", b"secret", stop)
                .await
                .is_err());
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let birth = recover_one(owned).await?;
            assert_eq!(&**read_live_secret(owned, &birth).await?, b"secret");
        }
        Ok(())
    }

    #[tokio::test]
    async fn same_owner_reconciles_without_second_birth_handoff() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        assert!(allocate_one_inner(
            owned,
            &empty,
            b"scope",
            b"original secret",
            StopAfter::Inventory
        )
        .await
        .is_err());
        let birth = reconcile_one(owned, &empty).await?;
        assert_eq!(
            &**read_live_secret(owned, &birth).await?,
            b"original secret"
        );
        assert!(
            reconcile_one(owned, &empty).await.is_err(),
            "birth owner transfers once"
        );
        assert!(allocate_one(owned, &empty, b"scope", b"other secret")
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn missing_intent_and_completed_inventory_loss_refuse_recovery() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        assert!(
            allocate_one_inner(owned, &empty, b"scope", b"secret", StopAfter::RootIntent)
                .await
                .is_err()
        );
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_one(owned).await.is_err(),
            "missing intent cannot be reconstructed"
        );
        drop(reopened);

        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        drop(birth);
        drop(empty);
        drop(first);
        std::fs::remove_file(fixture.profile.path().join(INVENTORY_FILE))?;
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_one(owned).await.is_err(),
            "committed inventory loss is not fresh"
        );
        Ok(())
    }

    #[tokio::test]
    async fn foreign_profile_and_unexplained_credential_refuse_birth() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        write_exact(platform, RECORD_USER, b"unexplained credential")?;
        assert!(allocate_one(owned, &empty, b"scope", b"secret")
            .await
            .is_err());
        let root = read_root(owned, platform)?;
        assert_eq!(root.phase, COMPLETE, "refusal precedes root mutation");
        drop(empty);
        drop(first);
        let foreign = tempfile::tempdir()?;
        let foreign_selected = super::super::tests::selected(foreign.path(), &fixture.service)?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(recover_one(foreign_owned).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn changed_credential_and_oversized_birth_refuse_without_repair() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        assert!(
            allocate_one(owned, &empty, &vec![1; MAX_SCOPE_BYTES + 1], b"secret")
                .await
                .is_err()
        );
        assert!(
            allocate_one(owned, &empty, b"scope", &vec![2; MAX_SECRET_BYTES + 1])
                .await
                .is_err()
        );
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        write_exact(platform, RECORD_USER, b"different ciphertext")?;
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(recover_one(owned).await.is_err());
        Ok(())
    }
}
