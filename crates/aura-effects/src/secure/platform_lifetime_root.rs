//! Selected platform lifetime root and private birth transaction dependencies.
//! Neither issues decision or retirement custody. L6 continues to reject
//! platform lifetime selection until the complete provider is validated.
use super::{
    PlatformSecureStorageHandler, ProductionSecureStorageHandler, ProfileOwnedSecureStorage,
};
use aura_core::{effects::profile_storage::ProfileStorageLease, AuraError};
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use rand::RngCore;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};

mod birth;

const KEYRING_USER: &str = "allocation-lifetime-empty-root:v1";
const FILE_PATH: &str = "platform_lifetime/root.v1";
const KEYRING_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-ROOT-V9";
const FILE_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-FILE-V1";
const PENDING: u8 = 0;
const COMPLETE: u8 = 1;
const BIRTH_PENDING: u8 = 2;
const BIRTH_COMPLETE: u8 = 3;
const DECISION_PENDING: u8 = 4;
const DECISION_COMPLETE: u8 = 5;
const TOMBSTONE_PENDING: u8 = 6;
const TOMBSTONE_ACK_ERASURE_PENDING: u8 = 7;
const ERASURE_PENDING: u8 = 8;
const ERASURE_COMPLETE: u8 = 9;
const PLAINTEXT_HEADER_LEN: usize = 1 + 32 * 14 + 2 + 2 + 2;
const MAX_PLAINTEXT_LEN: usize =
    PLAINTEXT_HEADER_LEN + 2 * aura_core::effects::secret_lifetime::MAX_SCOPE_BYTES;
const SECOND_BIRTH_COMPLETE: u8 = 0;
const SECOND_DECISION_PENDING: u8 = 1;
const SECOND_DECISION_COMPLETE: u8 = 2;
const SECOND_TOMBSTONE_PENDING: u8 = 3;
const SECOND_TOMBSTONE_ACK_ERASURE_PENDING: u8 = 4;
const SECOND_ERASURE_PENDING: u8 = 5;
const SECOND_ERASURE_COMPLETE: u8 = 6;
const THIRD_UNCLAIMED: u8 = 0;
const THIRD_BIRTH_PENDING: u8 = 1;
const THIRD_BIRTH_COMPLETE: u8 = 2;
const THIRD_DECISION_PENDING: u8 = 3;
const THIRD_DECISION_COMPLETE: u8 = 4;
const THIRD_TOMBSTONE_PENDING: u8 = 5;
const THIRD_TOMBSTONE_ACK_ERASURE_PENDING: u8 = 6;
const THIRD_ERASURE_PENDING: u8 = 7;
const THIRD_ERASURE_COMPLETE: u8 = 8;
const MAX_FILE_LEN: usize = 256;

#[derive(Clone)]
struct RootRecord {
    phase: u8,
    id: [u8; 32],
    profile: [u8; 32],
    service: [u8; 32],
    birth_id: [u8; 32],
    intent_digest: [u8; 32],
    decision_digest: [u8; 32],
    roster_digest: [u8; 32],
    second_phase: u8,
    second_decision_digest: [u8; 32],
    second_tombstone_digest: [u8; 32],
    third_phase: u8,
    third_id: [u8; 32],
    third_intent_digest: [u8; 32],
    third_roster_digest: [u8; 32],
    third_decision_digest: [u8; 32],
    third_tombstone_digest: [u8; 32],
    // Retained nonsecret birth scopes survive selected-provider credential
    // erasure. Their bytes are authenticated inside the original root.
    first_scope: Vec<u8>,
    second_scope: Vec<u8>,
}

fn invalid(message: &'static str) -> AuraError {
    AuraError::invalid(message)
}

fn fixed_bytes<const N: usize>(bytes: &[u8]) -> Result<[u8; N], AuraError> {
    bytes
        .try_into()
        .map_err(|_| invalid("selected platform lifetime field length is invalid"))
}
fn io(source: std::io::Error) -> AuraError {
    AuraError::Storage {
        message: "selected platform lifetime root publication failed".into(),
        source: Some(Arc::new(source)),
    }
}

impl RootRecord {
    fn file_marker(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(FILE_MAGIC.len() + 96);
        bytes.extend_from_slice(FILE_MAGIC);
        bytes.extend_from_slice(&self.id);
        bytes.extend_from_slice(&self.profile);
        bytes.extend_from_slice(&self.service);
        bytes
    }

    fn encode(&self, selected: &PlatformSecureStorageHandler) -> Result<Vec<u8>, AuraError> {
        let key = selected.record_key(true)?;
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|source| {
                AuraError::crypto_with_source("platform lifetime root entropy", Arc::new(source))
            })?;
        if self.first_scope.len() > aura_core::effects::secret_lifetime::MAX_SCOPE_BYTES
            || self.second_scope.len() > aura_core::effects::secret_lifetime::MAX_SCOPE_BYTES
        {
            return Err(invalid("platform lifetime scope exceeds root bound"));
        }
        let mut plain = Vec::with_capacity(
            PLAINTEXT_HEADER_LEN + self.first_scope.len() + self.second_scope.len(),
        );
        plain.push(self.phase);
        plain.extend_from_slice(&self.id);
        plain.extend_from_slice(&self.profile);
        plain.extend_from_slice(&self.service);
        plain.extend_from_slice(&self.birth_id);
        plain.extend_from_slice(&self.intent_digest);
        plain.extend_from_slice(&self.decision_digest);
        plain.extend_from_slice(&self.roster_digest);
        plain.push(self.second_phase);
        plain.extend_from_slice(&self.second_decision_digest);
        plain.extend_from_slice(&self.second_tombstone_digest);
        plain.push(self.third_phase);
        plain.extend_from_slice(&self.third_id);
        plain.extend_from_slice(&self.third_intent_digest);
        plain.extend_from_slice(&self.third_roster_digest);
        plain.extend_from_slice(&self.third_decision_digest);
        plain.extend_from_slice(&self.third_tombstone_digest);
        plain.extend_from_slice(&(self.first_scope.len() as u16).to_le_bytes());
        plain.extend_from_slice(&(self.second_scope.len() as u16).to_le_bytes());
        plain.extend_from_slice(&self.first_scope);
        plain.extend_from_slice(&self.second_scope);
        let ciphertext = ChaCha20Poly1305::new((&key).into())
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: KEYRING_MAGIC,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source("platform lifetime root encryption", Arc::new(source))
            })?;
        let mut bytes = Vec::with_capacity(KEYRING_MAGIC.len() + 12 + ciphertext.len());
        bytes.extend_from_slice(KEYRING_MAGIC);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&ciphertext);
        Ok(bytes)
    }

    fn decode(selected: &PlatformSecureStorageHandler, bytes: &[u8]) -> Result<Self, AuraError> {
        if !bytes.starts_with(KEYRING_MAGIC)
            || bytes.len() < KEYRING_MAGIC.len() + 12 + PLAINTEXT_HEADER_LEN + 16
            || bytes.len() > KEYRING_MAGIC.len() + 12 + MAX_PLAINTEXT_LEN + 16
        {
            return Err(invalid(
                "selected platform lifetime root encoding is invalid",
            ));
        }
        let key = selected.record_key(false)?;
        let nonce = &bytes[KEYRING_MAGIC.len()..KEYRING_MAGIC.len() + 12];
        let ciphertext = &bytes[KEYRING_MAGIC.len() + 12..];
        let plain = ChaCha20Poly1305::new((&key).into())
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: KEYRING_MAGIC,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "selected platform lifetime root authentication failed",
                    Arc::new(source),
                )
            })?;
        let first_scope_len = u16::from_le_bytes([plain[451], plain[452]]) as usize;
        let second_scope_len = u16::from_le_bytes([plain[453], plain[454]]) as usize;
        if plain.len() != PLAINTEXT_HEADER_LEN + first_scope_len + second_scope_len
            || first_scope_len > aura_core::effects::secret_lifetime::MAX_SCOPE_BYTES
            || second_scope_len > aura_core::effects::secret_lifetime::MAX_SCOPE_BYTES
            || (matches!(plain[0], PENDING | COMPLETE)
                && (first_scope_len != 0 || second_scope_len != 0))
            || (!matches!(plain[0], PENDING | COMPLETE) && first_scope_len == 0)
            || !matches!(
                plain[0],
                PENDING
                    | COMPLETE
                    | BIRTH_PENDING
                    | BIRTH_COMPLETE
                    | DECISION_PENDING
                    | DECISION_COMPLETE
                    | TOMBSTONE_PENDING
                    | TOMBSTONE_ACK_ERASURE_PENDING
                    | ERASURE_PENDING
                    | ERASURE_COMPLETE
            )
            || (matches!(plain[0], PENDING | COMPLETE)
                && (plain[97..129] != [0; 32]
                    || plain[129..161] != [0; 32]
                    || plain[161..193] != [0; 32]
                    || plain[193..225] != [0; 32]))
            || (matches!(plain[0], BIRTH_PENDING | BIRTH_COMPLETE)
                && (plain[97..129] == [0; 32]
                    || plain[129..161] == [0; 32]
                    || plain[161..193] != [0; 32]
                    || plain[193..225] == [0; 32]))
            || (matches!(
                plain[0],
                DECISION_PENDING
                    | DECISION_COMPLETE
                    | TOMBSTONE_PENDING
                    | TOMBSTONE_ACK_ERASURE_PENDING
                    | ERASURE_PENDING
                    | ERASURE_COMPLETE
            ) && (plain[97..129] == [0; 32]
                || plain[129..161] == [0; 32]
                || plain[161..193] == [0; 32]
                || plain[193..225] == [0; 32]))
            || !matches!(plain[225], 0..=SECOND_ERASURE_COMPLETE)
            || (plain[225] == SECOND_BIRTH_COMPLETE
                && (plain[226..258] != [0; 32] || plain[258..290] != [0; 32]))
            || (matches!(
                plain[225],
                SECOND_DECISION_PENDING | SECOND_DECISION_COMPLETE
            ) && (plain[226..258] == [0; 32] || plain[258..290] != [0; 32]))
            || (plain[225] >= SECOND_TOMBSTONE_PENDING
                && (plain[226..258] == [0; 32] || plain[258..290] == [0; 32]))
            || !matches!(plain[290], THIRD_UNCLAIMED..=THIRD_ERASURE_COMPLETE)
            || (plain[290] == THIRD_UNCLAIMED && plain[291..451] != [0; 160])
            || (plain[290] != THIRD_UNCLAIMED
                && (plain[291..323] == [0; 32]
                    || plain[323..355] == [0; 32]
                    || plain[355..387] == [0; 32]
                    || plain[291..323] == plain[97..129]))
            || (plain[290] <= THIRD_BIRTH_COMPLETE
                && (plain[387..419] != [0; 32] || plain[419..451] != [0; 32]))
            || (matches!(plain[290], THIRD_DECISION_PENDING | THIRD_DECISION_COMPLETE)
                && (plain[387..419] == [0; 32] || plain[419..451] != [0; 32]))
            || (plain[290] >= THIRD_TOMBSTONE_PENDING
                && (plain[387..419] == [0; 32] || plain[419..451] == [0; 32]))
        {
            return Err(invalid("selected platform lifetime root phase is invalid"));
        }
        Ok(Self {
            phase: plain[0],
            id: fixed_bytes(&plain[1..33])?,
            profile: fixed_bytes(&plain[33..65])?,
            service: fixed_bytes(&plain[65..97])?,
            birth_id: fixed_bytes(&plain[97..129])?,
            intent_digest: fixed_bytes(&plain[129..161])?,
            decision_digest: fixed_bytes(&plain[161..193])?,
            roster_digest: fixed_bytes(&plain[193..225])?,
            second_phase: plain[225],
            second_decision_digest: fixed_bytes(&plain[226..258])?,
            second_tombstone_digest: fixed_bytes(&plain[258..290])?,
            third_phase: plain[290],
            third_id: fixed_bytes(&plain[291..323])?,
            third_intent_digest: fixed_bytes(&plain[323..355])?,
            third_roster_digest: fixed_bytes(&plain[355..387])?,
            third_decision_digest: fixed_bytes(&plain[387..419])?,
            third_tombstone_digest: fixed_bytes(&plain[419..451])?,
            first_scope: plain[455..455 + first_scope_len].to_vec(),
            second_scope: plain[455 + first_scope_len..].to_vec(),
        })
    }
}

fn write_keyring(
    selected: &PlatformSecureStorageHandler,
    record: &RootRecord,
) -> Result<(), AuraError> {
    let entry = selected.entry_for_user(KEYRING_USER)?;
    let bytes = record.encode(selected)?;
    entry
        .set_secret(&bytes)
        .map_err(PlatformSecureStorageHandler::map_keyring_error)?;
    let observed = entry
        .get_secret()
        .map_err(PlatformSecureStorageHandler::map_keyring_error)?;
    if observed != bytes {
        return Err(invalid(
            "selected platform lifetime root write was not acknowledged",
        ));
    }
    Ok(())
}

/// Private proof that the selected platform provider admitted one empty root.
/// It cannot allocate secrets or authorize a lifetime transition.
#[allow(dead_code)]
pub(super) struct EmptyPlatformLifetimeRootAdmission {
    _id: [u8; 32],
    _owner: Arc<crate::profile_storage::OwnedProfileLease>,
    birth_handed_out: std::sync::atomic::AtomicBool,
}

/// Admit/recover only the original empty root. The keyring Pending record is
/// durable before the profile marker; Complete is written only after its exact
/// descriptor-relative publication is acknowledged. A missing marker after
/// Complete is loss, not an interrupted initialization.
#[allow(dead_code)]
pub(super) async fn admit_empty_root(
    owned: &ProfileOwnedSecureStorage,
) -> Result<EmptyPlatformLifetimeRootAdmission, AuraError> {
    let ProductionSecureStorageHandler::Platform(selected) = owned.backend.as_ref() else {
        return Err(invalid(
            "platform lifetime root requires selected platform provider",
        ));
    };
    let owner = &owned._owner;
    let namespace = selected
        .namespace_owner
        .as_ref()
        .ok_or_else(|| invalid("selected platform provider lacks original namespace owner"))?;
    if !owner
        .keyring_namespace
        .get()
        .is_some_and(|original| Arc::ptr_eq(original, namespace))
    {
        return Err(invalid(
            "selected platform namespace differs from original profile",
        ));
    }
    let _guard = namespace.mutation_guard().await;
    let file = Path::new(FILE_PATH);
    let existing_file = owner
        .directory
        .read_private_publication(file, MAX_FILE_LEN)
        .map_err(io)?;
    let entry = selected.entry_for_user(KEYRING_USER)?;
    let profile = aura_core::hash::hash(owner.profile_identity().as_bytes());
    let service = aura_core::hash::hash(selected.service.as_bytes());
    let mut record = match entry.get_secret() {
        Ok(bytes) => RootRecord::decode(selected, &bytes)?,
        Err(keyring::Error::NoEntry) if existing_file.is_none() => {
            let mut id = [0u8; 32];
            rand::rngs::OsRng
                .try_fill_bytes(&mut id)
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "platform lifetime root identity entropy",
                        Arc::new(source),
                    )
                })?;
            let record = RootRecord {
                phase: PENDING,
                id,
                profile,
                service,
                birth_id: [0; 32],
                intent_digest: [0; 32],
                decision_digest: [0; 32],
                roster_digest: [0; 32],
                second_phase: SECOND_BIRTH_COMPLETE,
                second_decision_digest: [0; 32],
                second_tombstone_digest: [0; 32],
                third_phase: THIRD_UNCLAIMED,
                third_id: [0; 32],
                third_intent_digest: [0; 32],
                third_roster_digest: [0; 32],
                third_decision_digest: [0; 32],
                third_tombstone_digest: [0; 32],
                first_scope: Vec::new(),
                second_scope: Vec::new(),
            };
            write_keyring(selected, &record)?;
            record
        }
        Err(keyring::Error::NoEntry) => {
            return Err(invalid(
                "selected platform lifetime credential is missing after profile publication",
            ))
        }
        Err(source) => return Err(PlatformSecureStorageHandler::map_keyring_error(source)),
    };
    if record.profile != profile || record.service != service {
        return Err(invalid(
            "selected platform lifetime root belongs to another owner",
        ));
    }
    if !matches!(record.phase, PENDING | COMPLETE) {
        return Err(invalid(
            "selected platform root already has allocation birth",
        ));
    }
    let expected = record.file_marker();
    match existing_file {
        Some((observed, stage)) => {
            if observed != expected {
                return Err(invalid("selected platform lifetime profile marker differs"));
            }
            if let Some(stage) = stage {
                stage.publish(true).map_err(io)?;
                stage.acknowledge().map_err(io)?;
            }
        }
        None if record.phase == PENDING => {
            let stage = owner
                .directory
                .prepare_private(file, &expected)
                .map_err(io)?;
            if !stage.publish(true).map_err(io)? {
                return Err(invalid(
                    "selected platform lifetime root publication conflicted",
                ));
            }
            stage.acknowledge().map_err(io)?;
        }
        None => {
            return Err(invalid(
                "completed selected platform lifetime profile marker is missing",
            ))
        }
    }
    if record.phase == PENDING {
        record.phase = COMPLETE;
        write_keyring(selected, &record)?;
    }
    owner
        .lifetime_root_claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("selected platform lifetime root already transferred"))?;
    Ok(EmptyPlatformLifetimeRootAdmission {
        _id: record.id,
        _owner: owner.clone(),
        birth_handed_out: std::sync::atomic::AtomicBool::new(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

    pub(super) fn selected(
        path: &Path,
        service: &str,
    ) -> Result<ProductionSecureStorageHandler, AuraError> {
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.to_path_buf())
                .acquire_owned_native()
                .map_err(|source| AuraError::Storage {
                    message: "acquire selected platform fixture".into(),
                    source: Some(Arc::new(source)),
                })?,
        );
        let mut platform = PlatformSecureStorageHandler::new();
        platform.service = service.to_owned();
        ProductionSecureStorageHandler::Platform(platform)
            .retain_profile_owner(owner)
            .map_err(|source| AuraError::Storage {
                message: "bind selected platform fixture".into(),
                source: Some(Arc::new(source)),
            })
    }

    pub(super) struct Fixture {
        pub(super) profile: tempfile::TempDir,
        pub(super) service: String,
    }
    impl Fixture {
        pub(super) fn new() -> TestResult<Self> {
            let profile = tempfile::tempdir()?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(profile.path(), std::fs::Permissions::from_mode(0o700))?;
            let service = format!(
                "aura-platform-lifetime-root:{}",
                hex::encode(aura_core::hash::hash(
                    profile.path().as_os_str().as_encoded_bytes()
                ))
            );
            Ok(Self { profile, service })
        }
        pub(super) fn selected(&self) -> Result<ProductionSecureStorageHandler, AuraError> {
            selected(self.profile.path(), &self.service)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Ok(ProductionSecureStorageHandler::ProfileOwned(owned)) = self.selected() {
                if let ProductionSecureStorageHandler::Platform(selected) = owned.backend.as_ref() {
                    if let Ok(root) = selected
                        .entry_for_user(KEYRING_USER)
                        .and_then(|entry| {
                            entry
                                .get_secret()
                                .map_err(PlatformSecureStorageHandler::map_keyring_error)
                        })
                        .and_then(|bytes| RootRecord::decode(selected, &bytes))
                    {
                        if root.third_phase != THIRD_UNCLAIMED {
                            birth::cleanup_third_fixture(&self.service, &root.third_id);
                            for name in [
                                format!(
                                    "allocation-lifetime-birth-intent:v2:{}",
                                    hex::encode(root.third_id)
                                ),
                                format!(
                                    "allocation-lifetime-birth-record:v2:{}",
                                    hex::encode(root.third_id)
                                ),
                            ] {
                                if let Ok(entry) = keyring::Entry::new(&self.service, &name) {
                                    let _ = entry.delete_credential();
                                }
                            }
                        }
                    }
                }
            }
            birth::cleanup_append_fixture(&self.service);
            for user in [
                KEYRING_USER,
                PlatformSecureStorageHandler::RECORD_KEY_USER,
                PlatformSecureStorageHandler::RECORD_INVENTORY_USER,
                birth::INTENT_USER,
                birth::RECORD_USER,
                "allocation-lifetime-first-decision:v1",
                "allocation-lifetime-negative-tombstone:v1",
            ] {
                if let Ok(entry) = keyring::Entry::new(&self.service, user) {
                    let _ = entry.delete_credential();
                }
            }
        }
    }

    #[tokio::test]
    async fn selected_platform_handoff_refuses_bounded_private_adapter_without_mutation(
    ) -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let error = match selected.into_selected_profile_lifetime_channel() {
            Err(error) => error,
            Ok(_) => panic!("two-entry private adapter must not enter production runtime"),
        };
        assert!(matches!(
            std::error::Error::source(&error).and_then(|source| source
                .downcast_ref::<aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable>()),
            Some(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider)
        ));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        assert!(matches!(
            platform.entry_for_user(KEYRING_USER)?.get_secret(),
            Err(keyring::Error::NoEntry)
        ));
        assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        Ok(())
    }

    #[tokio::test]
    async fn selected_empty_root_reopens_once_under_original_provider() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let original = admit_empty_root(owned).await?;
        assert!(
            admit_empty_root(owned).await.is_err(),
            "one root per original owner"
        );
        let original_id = original._id;
        drop(original);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert_eq!(admit_empty_root(owned).await?._id, original_id);
        Ok(())
    }

    #[tokio::test]
    async fn pending_keyring_root_replays_exact_profile_marker() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        let pending = RootRecord {
            phase: PENDING,
            id: [73; 32],
            profile: aura_core::hash::hash(owned._owner.profile_identity().as_bytes()),
            service: aura_core::hash::hash(platform.service.as_bytes()),
            birth_id: [0; 32],
            intent_digest: [0; 32],
            decision_digest: [0; 32],
            roster_digest: [0; 32],
            second_phase: SECOND_BIRTH_COMPLETE,
            second_decision_digest: [0; 32],
            second_tombstone_digest: [0; 32],
            third_phase: THIRD_UNCLAIMED,
            third_id: [0; 32],
            third_intent_digest: [0; 32],
            third_roster_digest: [0; 32],
            third_decision_digest: [0; 32],
            third_tombstone_digest: [0; 32],
            first_scope: Vec::new(),
            second_scope: Vec::new(),
        };
        {
            let _guard = platform.namespace_owner()?.mutation_guard().await;
            write_keyring(platform, &pending)?;
        }
        let admitted = admit_empty_root(owned).await?;
        assert_eq!(admitted._id, pending.id);
        let observed = platform.entry_for_user(KEYRING_USER)?.get_secret()?;
        assert_eq!(RootRecord::decode(platform, &observed)?.phase, COMPLETE);
        assert_eq!(
            owned
                ._owner
                .directory
                .read_private_publication(Path::new(FILE_PATH), MAX_FILE_LEN)?
                .expect("replayed marker")
                .0,
            pending.file_marker()
        );
        Ok(())
    }

    #[tokio::test]
    async fn completed_root_refuses_missing_marker_and_foreign_profile() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        admit_empty_root(owned).await?;
        drop(first);
        let foreign = tempfile::tempdir()?;
        let foreign_selected = selected(foreign.path(), &fixture.service)?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(admit_empty_root(foreign_owned).await.is_err());
        drop(foreign_selected);
        std::fs::remove_file(fixture.profile.path().join(FILE_PATH))?;
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            admit_empty_root(owned).await.is_err(),
            "completed marker loss is not fresh initialization"
        );
        Ok(())
    }

    #[tokio::test]
    async fn earlier_roots_cannot_be_reinterpreted_as_v9_lifetime_authority() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        let original = admit_empty_root(owned).await?;
        let root = RootRecord::decode(
            platform,
            &platform.entry_for_user(KEYRING_USER)?.get_secret()?,
        )?;
        for old_magic in [
            b"AURA-PLATFORM-LIFETIME-ROOT-V3".as_slice(),
            b"AURA-PLATFORM-LIFETIME-ROOT-V4".as_slice(),
            b"AURA-PLATFORM-LIFETIME-ROOT-V5".as_slice(),
            b"AURA-PLATFORM-LIFETIME-ROOT-V6".as_slice(),
            b"AURA-PLATFORM-LIFETIME-ROOT-V7".as_slice(),
            b"AURA-PLATFORM-LIFETIME-ROOT-V8".as_slice(),
        ] {
            let mut encoded = root.encode(platform)?;
            encoded[..KEYRING_MAGIC.len()].copy_from_slice(old_magic);
            assert!(RootRecord::decode(platform, &encoded).is_err());
        }
        let mut encoded = root.encode(platform)?;
        encoded[..KEYRING_MAGIC.len()].copy_from_slice(b"AURA-PLATFORM-LIFETIME-ROOT-V4");
        platform
            .entry_for_user(KEYRING_USER)?
            .set_secret(&encoded)?;
        drop(original);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(admit_empty_root(owned).await.is_err());
        Ok(())
    }
}
