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
const KEYRING_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-ROOT-V4";
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
const PLAINTEXT_LEN: usize = 1 + 32 * 7;
const MAX_FILE_LEN: usize = 256;

struct RootRecord {
    phase: u8,
    id: [u8; 32],
    profile: [u8; 32],
    service: [u8; 32],
    birth_id: [u8; 32],
    intent_digest: [u8; 32],
    decision_digest: [u8; 32],
    roster_digest: [u8; 32],
}

fn invalid(message: &'static str) -> AuraError {
    AuraError::invalid(message)
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
        let mut plain = Vec::with_capacity(PLAINTEXT_LEN);
        plain.push(self.phase);
        plain.extend_from_slice(&self.id);
        plain.extend_from_slice(&self.profile);
        plain.extend_from_slice(&self.service);
        plain.extend_from_slice(&self.birth_id);
        plain.extend_from_slice(&self.intent_digest);
        plain.extend_from_slice(&self.decision_digest);
        plain.extend_from_slice(&self.roster_digest);
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
            || bytes.len() != KEYRING_MAGIC.len() + 12 + PLAINTEXT_LEN + 16
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
        if plain.len() != PLAINTEXT_LEN
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
        {
            return Err(invalid("selected platform lifetime root phase is invalid"));
        }
        Ok(Self {
            phase: plain[0],
            id: plain[1..33].try_into().expect("bounded root id"),
            profile: plain[33..65].try_into().expect("bounded profile binding"),
            service: plain[65..97].try_into().expect("bounded service binding"),
            birth_id: plain[97..129].try_into().expect("bounded birth identity"),
            intent_digest: plain[129..161]
                .try_into()
                .expect("bounded birth intent digest"),
            decision_digest: plain[161..193]
                .try_into()
                .expect("bounded decision intent digest"),
            roster_digest: plain[193..225]
                .try_into()
                .expect("bounded original roster digest"),
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
    async fn v3_root_cannot_be_reinterpreted_as_v4_roster_authority() -> TestResult {
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
        let mut encoded = root.encode(platform)?;
        encoded[..KEYRING_MAGIC.len()].copy_from_slice(b"AURA-PLATFORM-LIFETIME-ROOT-V3");
        assert!(RootRecord::decode(platform, &encoded).is_err());
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
