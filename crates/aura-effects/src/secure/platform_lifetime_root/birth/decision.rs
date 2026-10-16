//! Original first decision for the private, single-allocation platform birth.
//! No retirement authority or production lifetime facade is issued here.

use super::*;
use aura_core::effects::secret_lifetime::MAX_DECISION_BYTES;

const DECISION_USER: &str = "allocation-lifetime-first-decision:v1";
const DECISION_FILE: &str = "platform_lifetime/decision.v1";
const DECISION_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-DECISION-V1";
const MARKER_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-DECISION-MARKER-V1";
pub(super) const POSITIVE: u8 = 1;
pub(super) const NEGATIVE: u8 = 2;
const HEADER_LEN: usize = 1 + 32 + 32 + 32 + 32 + 4;
const MAX_RECORD_LEN: usize = DECISION_MAGIC.len() + 12 + HEADER_LEN + MAX_DECISION_BYTES + 16;
const MAX_MARKER_LEN: usize = 256;

pub(super) mod negative_denial;

pub(super) struct DecisionRecord {
    pub(super) kind: u8,
    pub(super) root: [u8; 32],
    pub(super) allocation: [u8; 32],
    pub(super) profile: [u8; 32],
    pub(super) service: [u8; 32],
    pub(super) bytes: Zeroizing<Vec<u8>>,
}

impl DecisionRecord {
    pub(super) fn marker(&self) -> Vec<u8> {
        let mut marker = Vec::with_capacity(MARKER_MAGIC.len() + 129);
        marker.extend_from_slice(MARKER_MAGIC);
        marker.extend_from_slice(&self.root);
        marker.extend_from_slice(&self.allocation);
        marker.extend_from_slice(&self.profile);
        marker.extend_from_slice(&self.service);
        marker.push(self.kind);
        marker.extend_from_slice(&aura_core::hash::hash(&self.bytes));
        marker
    }

    pub(super) fn encode(
        &self,
        selected: &PlatformSecureStorageHandler,
    ) -> Result<Vec<u8>, AuraError> {
        let key = Zeroizing::new(selected.record_key(false)?);
        let mut plain = Zeroizing::new(Vec::with_capacity(HEADER_LEN + self.bytes.len()));
        plain.push(self.kind);
        plain.extend_from_slice(&self.root);
        plain.extend_from_slice(&self.allocation);
        plain.extend_from_slice(&self.profile);
        plain.extend_from_slice(&self.service);
        plain.extend_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        plain.extend_from_slice(&self.bytes);
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|source| {
                AuraError::crypto_with_source("platform decision entropy", Arc::new(source))
            })?;
        let ciphertext = ChaCha20Poly1305::new((&*key).into())
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: DECISION_MAGIC,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source("platform decision encryption", Arc::new(source))
            })?;
        let mut encoded = Vec::with_capacity(DECISION_MAGIC.len() + 12 + ciphertext.len());
        encoded.extend_from_slice(DECISION_MAGIC);
        encoded.extend_from_slice(&nonce);
        encoded.extend_from_slice(&ciphertext);
        Ok(encoded)
    }

    pub(super) fn decode(
        selected: &PlatformSecureStorageHandler,
        encoded: &[u8],
    ) -> Result<Self, AuraError> {
        if encoded.len() > MAX_RECORD_LEN
            || encoded.len() < DECISION_MAGIC.len() + 12 + HEADER_LEN + 16
            || !encoded.starts_with(DECISION_MAGIC)
        {
            return Err(invalid("platform decision encoding is invalid"));
        }
        let key = Zeroizing::new(selected.record_key(false)?);
        let mut plain = Zeroizing::new(
            ChaCha20Poly1305::new((&*key).into())
                .decrypt(
                    Nonce::from_slice(&encoded[DECISION_MAGIC.len()..DECISION_MAGIC.len() + 12]),
                    Payload {
                        msg: &encoded[DECISION_MAGIC.len() + 12..],
                        aad: DECISION_MAGIC,
                    },
                )
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "platform decision authentication failed",
                        Arc::new(source),
                    )
                })?,
        );
        if plain.len() < HEADER_LEN || !matches!(plain[0], POSITIVE | NEGATIVE) {
            return Err(invalid("platform decision kind is invalid"));
        }
        let len = u32::from_le_bytes(fixed_bytes(&plain[129..133])?) as usize;
        if len == 0 || len > MAX_DECISION_BYTES || plain.len() != HEADER_LEN + len {
            return Err(invalid("platform decision length is invalid"));
        }
        let bytes = Zeroizing::new(plain.split_off(HEADER_LEN));
        Ok(Self {
            kind: plain[0],
            root: fixed_bytes(&plain[1..33])?,
            allocation: fixed_bytes(&plain[33..65])?,
            profile: fixed_bytes(&plain[65..97])?,
            service: fixed_bytes(&plain[97..129])?,
            bytes,
        })
    }

    fn bound_to(&self, root: &RootRecord) -> Result<(), AuraError> {
        if self.root != root.id
            || self.allocation != root.birth_id
            || self.profile != root.profile
            || self.service != root.service
        {
            return Err(invalid(
                "platform decision belongs to another original birth",
            ));
        }
        Ok(())
    }
}

fn marker_state(
    owned: &ProfileOwnedSecureStorage,
    record: &DecisionRecord,
) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(DECISION_FILE), MAX_MARKER_LEN)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == record.marker() => Ok(true),
        _ => Err(invalid(
            "platform decision marker differs from original intent",
        )),
    }
}

fn publish_marker(
    owned: &ProfileOwnedSecureStorage,
    record: &DecisionRecord,
) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(DECISION_FILE), &record.marker())
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform decision marker publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

pub(super) fn read_record(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Option<DecisionRecord>, AuraError> {
    match (root.phase, read_entry(selected, DECISION_USER)?) {
        (BIRTH_COMPLETE, None) => Ok(None),
        (BIRTH_COMPLETE, Some(_)) => Err(invalid("platform decision lacks original root intent")),
        (
            DECISION_PENDING
            | DECISION_COMPLETE
            | TOMBSTONE_PENDING
            | TOMBSTONE_ACK_ERASURE_PENDING
            | ERASURE_PENDING
            | ERASURE_COMPLETE,
            Some(bytes),
        ) => {
            if aura_core::hash::hash(&bytes) != root.decision_digest {
                return Err(invalid(
                    "platform decision differs from original root intent",
                ));
            }
            let record = DecisionRecord::decode(selected, &bytes)?;
            record.bound_to(root)?;
            Ok(Some(record))
        }
        (
            DECISION_PENDING
            | DECISION_COMPLETE
            | TOMBSTONE_PENDING
            | TOMBSTONE_ACK_ERASURE_PENDING
            | ERASURE_PENDING
            | ERASURE_COMPLETE,
            None,
        ) => Err(invalid("platform decision original intent is missing")),
        _ => Err(invalid("platform decision root phase is invalid")),
    }
}

/// Authenticate decision evidence before recovered birth custody is handed out.
/// A pending original intent is recoverable; a completed marker loss is not.
pub(super) fn validate_recovery(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<(), AuraError> {
    if let Some(record) = read_record(selected, root)? {
        let marker = marker_state(owned, &record)?;
        if matches!(
            root.phase,
            DECISION_COMPLETE
                | TOMBSTONE_PENDING
                | TOMBSTONE_ACK_ERASURE_PENDING
                | ERASURE_PENDING
                | ERASURE_COMPLETE
        ) && !marker
        {
            return Err(invalid("completed platform decision marker is missing"));
        }
    } else if owned
        ._owner
        .directory
        .read_private_publication(Path::new(DECISION_FILE), MAX_MARKER_LEN)
        .map_err(io)?
        .is_some()
    {
        return Err(invalid("platform decision marker lacks original intent"));
    }
    negative_denial::validate_recovery(owned, selected, root)
}

pub(super) fn recover_erasure(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(), AuraError> {
    validate_recovery(owned, selected, root)?;
    negative_denial::recover_erasure(owned, selected, root)
}

fn finish_original(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    mut root: RootRecord,
    record: DecisionRecord,
) -> Result<DecisionRecord, AuraError> {
    match marker_state(owned, &record)? {
        true => {}
        false if root.phase == DECISION_PENDING => publish_marker(owned, &record)?,
        false => return Err(invalid("completed platform decision marker is missing")),
    }
    if root.phase == DECISION_PENDING {
        root.phase = DECISION_COMPLETE;
        write_keyring(selected, &root)?;
    }
    Ok(record)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootIntent,
    Intent,
    Marker,
}

/// Private first-decision transaction. Only original birth custody can call it;
/// an exact replay returns the authentic winner without replacing it.
#[allow(dead_code)]
pub(super) async fn decide(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(owned, birth, positive, bytes, StopAfter::Never).await
}

async fn decide_inner(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
    positive: bool,
    bytes: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _intent) = verify_birth(owned, selected, birth)?;
    decide_locked(owned, selected, root, positive, bytes, stop)
}

/// First decision after a second allocation exists. The original first-birth
/// token is deliberately insufficient; only the recovered whole-roster owner
/// can authorize this same immutable first decision transaction.
#[allow(dead_code)]
pub(super) async fn decide_roster(
    owned: &ProfileOwnedSecureStorage,
    roster: &append::PlatformRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_roster_inner(
        owned,
        append::OriginalRosterOwner::Two(roster),
        positive,
        bytes,
        StopAfter::Never,
    )
    .await
}

pub(super) async fn decide_three(
    owned: &ProfileOwnedSecureStorage,
    three: &append::third::PlatformThreeRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_roster_inner(
        owned,
        append::OriginalRosterOwner::Three(three),
        positive,
        bytes,
        StopAfter::Never,
    )
    .await
}

async fn decide_roster_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: append::OriginalRosterOwner<'_>,
    positive: bool,
    bytes: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _) = append::verify_original_roster(owned, selected, roster)?;
    decide_locked(owned, selected, root, positive, bytes, stop)
}

fn decide_locked(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    mut root: RootRecord,
    positive: bool,
    bytes: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    if bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES {
        return Err(invalid("platform decision size is invalid"));
    }
    let kind = if positive { POSITIVE } else { NEGATIVE };
    if let Some(existing) = read_record(selected, &root)? {
        if existing.kind != kind || existing.bytes.as_slice() != bytes {
            return Err(invalid(
                "platform first decision contradicts original decision",
            ));
        }
        finish_original(owned, selected, root, existing)?;
        return Ok(());
    }
    if owned
        ._owner
        .directory
        .read_private_publication(Path::new(DECISION_FILE), MAX_MARKER_LEN)
        .map_err(io)?
        .is_some()
    {
        return Err(invalid("platform decision marker lacks original intent"));
    }
    let record = DecisionRecord {
        kind,
        root: root.id,
        allocation: root.birth_id,
        profile: root.profile,
        service: root.service,
        bytes: Zeroizing::new(bytes.to_vec()),
    };
    let encoded = record.encode(selected)?;
    root.phase = DECISION_PENDING;
    root.decision_digest = aura_core::hash::hash(&encoded);
    write_keyring(selected, &root)?;
    if stop == StopAfter::RootIntent {
        return Err(interrupted());
    }
    write_exact(selected, DECISION_USER, &encoded)?;
    if stop == StopAfter::Intent {
        return Err(interrupted());
    }
    publish_marker(owned, &record)?;
    if stop == StopAfter::Marker {
        return Err(interrupted());
    }
    finish_original(owned, selected, root, record)?;
    Ok(())
}

/// Reopen completes only the authenticated original first decision.
#[allow(dead_code)]
pub(super) async fn recover(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
) -> Result<Option<(bool, Zeroizing<Vec<u8>>)>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _intent) = verify_birth(owned, selected, birth)?;
    if let Some(record) = read_record(selected, &root)? {
        let record = finish_original(owned, selected, root, record)?;
        Ok(Some((record.kind == POSITIVE, record.bytes)))
    } else if owned
        ._owner
        .directory
        .read_private_publication(Path::new(DECISION_FILE), MAX_MARKER_LEN)
        .map_err(io)?
        .is_some()
    {
        Err(invalid("platform decision marker lacks original intent"))
    } else {
        Ok(None)
    }
}

pub(super) fn require_live_read(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<(), AuraError> {
    if let Some(record) = read_record(selected, root)? {
        let marker = marker_state(owned, &record)?;
        if matches!(
            root.phase,
            DECISION_COMPLETE
                | TOMBSTONE_PENDING
                | TOMBSTONE_ACK_ERASURE_PENDING
                | ERASURE_PENDING
                | ERASURE_COMPLETE
        ) && !marker
        {
            return Err(invalid("completed platform decision marker is missing"));
        }
        if matches!(
            root.phase,
            TOMBSTONE_PENDING | TOMBSTONE_ACK_ERASURE_PENDING
        ) && record.kind != NEGATIVE
        {
            return Err(invalid("platform denial lacks original negative decision"));
        }
        if record.kind == NEGATIVE || root.phase == DECISION_PENDING {
            return Err(invalid("platform live read denied by first decision"));
        }
    } else if owned
        ._owner
        .directory
        .read_private_publication(Path::new(DECISION_FILE), MAX_MARKER_LEN)
        .map_err(io)?
        .is_some()
    {
        return Err(invalid("platform decision marker lacks original intent"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn positive_first_decision_is_exact_and_reopens() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, true, b"original approval").await?;
        decide(owned, &birth, true, b"original approval").await?;
        assert!(decide(owned, &birth, false, b"original approval")
            .await
            .is_err());
        assert!(decide(owned, &birth, true, b"different approval")
            .await
            .is_err());
        assert_eq!(&**read_live_secret(owned, &birth).await?, b"secret");
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let birth = recover_one(owned).await?;
        let (positive, original) = recover(owned, &birth).await?.expect("original decision");
        assert!(positive);
        assert_eq!(&**original, b"original approval");
        assert_eq!(&**read_live_secret(owned, &birth).await?, b"secret");
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_negative_first_decision_recovers_original_and_denies_read() -> TestResult {
        for stop in [StopAfter::Intent, StopAfter::Marker] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
            assert!(
                decide_inner(owned, &birth, false, b"original refusal", stop)
                    .await
                    .is_err()
            );
            assert!(read_live_secret(owned, &birth).await.is_err());
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let birth = recover_one(owned).await?;
            let (positive, original) = recover(owned, &birth).await?.expect("original decision");
            assert!(!positive);
            assert_eq!(&**original, b"original refusal");
            assert!(decide(owned, &birth, false, b"replacement refusal")
                .await
                .is_err());
            assert!(decide(owned, &birth, true, b"original refusal")
                .await
                .is_err());
            assert!(read_live_secret(owned, &birth).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn missing_or_unexplained_decision_evidence_refuses_without_repair() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, true, b"approval").await?;
        std::fs::remove_file(fixture.profile.path().join(DECISION_FILE))?;
        assert!(recover(owned, &birth).await.is_err());
        assert!(read_live_secret(owned, &birth).await.is_err());
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_one(owned).await.is_err(),
            "corrupt decision evidence cannot transfer recovered birth custody"
        );
        drop(reopened);

        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        let staged = owned
            ._owner
            .directory
            .prepare_private(Path::new(DECISION_FILE), b"unexplained marker")?;
        assert!(staged.publish(true)?);
        staged.acknowledge()?;
        assert!(decide(owned, &birth, true, b"approval").await.is_err());
        assert!(recover(owned, &birth).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn root_anchor_refuses_missing_intent_and_total_decision_loss() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        assert!(
            decide_inner(owned, &birth, false, b"refusal", StopAfter::RootIntent)
                .await
                .is_err()
        );
        assert!(read_live_secret(owned, &birth).await.is_err());
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_one(owned).await.is_err(),
            "missing original intent cannot become live"
        );
        drop(reopened);

        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, true, b"approval").await?;
        let ProductionSecureStorageHandler::Platform(platform) = owned.backend.as_ref() else {
            unreachable!()
        };
        platform
            .entry_for_user(DECISION_USER)?
            .delete_credential()?;
        std::fs::remove_file(fixture.profile.path().join(DECISION_FILE))?;
        assert!(read_live_secret(owned, &birth).await.is_err());
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(
            recover_one(owned).await.is_err(),
            "root anchor remembers the lost first decision"
        );
        Ok(())
    }

    #[tokio::test]
    async fn foreign_birth_oversized_decision_and_concurrent_contender_refuse() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        assert!(
            decide(owned, &birth, true, &vec![1; MAX_DECISION_BYTES + 1])
                .await
                .is_err()
        );
        let foreign = tempfile::tempdir()?;
        let foreign_selected =
            super::super::super::tests::selected(foreign.path(), &fixture.service)?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(decide(foreign_owned, &birth, true, b"approval")
            .await
            .is_err());
        let (first, second) = tokio::join!(
            decide(owned, &birth, true, b"first approval"),
            decide(owned, &birth, false, b"second refusal")
        );
        assert_ne!(first.is_ok(), second.is_ok(), "one original first decision");
        let (positive, original) = recover(owned, &birth).await?.expect("winner retained");
        assert_eq!(positive, first.is_ok());
        assert_eq!(
            &**original,
            if positive {
                &b"first approval"[..]
            } else {
                &b"second refusal"[..]
            }
        );
        Ok(())
    }

    async fn two_birth_roster() -> Result<
        (
            Fixture,
            ProductionSecureStorageHandler,
            append::PlatformRosterAdmission,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"first scope", b"first secret").await?;
        append::append_second(owned, &birth, b"second scope", b"second secret").await?;
        assert!(decide(owned, &birth, true, b"stale owner").await.is_err());
        drop(birth);
        drop(empty);
        drop(first);
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let roster = append::recover_roster(owned).await?;
        Ok((fixture, selected, roster))
    }

    #[tokio::test]
    async fn first_roster_decision_is_exact_and_reopens_with_both_entries() -> TestResult {
        let (fixture, selected, roster) = two_birth_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        decide_roster(owned, &roster, true, b"original approval").await?;
        decide_roster(owned, &roster, true, b"original approval").await?;
        assert!(decide_roster(owned, &roster, false, b"original approval")
            .await
            .is_err());
        assert!(decide_roster(owned, &roster, true, b"replacement approval")
            .await
            .is_err());
        assert_eq!(
            &**read_roster_first_secret(owned, &roster).await?,
            b"first secret"
        );
        assert_eq!(
            &**append::read_roster_second_secret(owned, &roster).await?,
            b"second secret"
        );
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = append::recover_roster(owned).await?;
        assert_eq!(
            &**read_roster_first_secret(owned, &roster).await?,
            b"first secret"
        );
        Ok(())
    }

    #[tokio::test]
    async fn first_roster_decision_interruptions_retain_original_intent() -> TestResult {
        for stop in [StopAfter::RootIntent, StopAfter::Intent, StopAfter::Marker] {
            let (fixture, selected, roster) = two_birth_roster().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            assert!(decide_roster_inner(
                owned,
                append::OriginalRosterOwner::Two(&roster),
                false,
                b"refusal",
                stop
            )
            .await
            .is_err());
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let result = append::recover_roster(owned).await;
            assert_eq!(result.is_ok(), stop != StopAfter::RootIntent, "{stop:?}");
            if let Ok(roster) = result {
                decide_roster(owned, &roster, false, b"refusal").await?;
                assert!(read_roster_first_secret(owned, &roster).await.is_err());
                assert_eq!(
                    read_root(owned, selected_owner(owned)?.0)?.phase,
                    DECISION_COMPLETE
                );
            } else {
                assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn first_roster_decision_refuses_foreign_owner_and_lost_evidence() -> TestResult {
        let (fixture, selected, roster) = two_birth_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let foreign = tempfile::tempdir()?;
        let foreign_selected =
            super::super::super::tests::selected(foreign.path(), &fixture.service)?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(decide_roster(foreign_owned, &roster, true, b"approval")
            .await
            .is_err());
        decide_roster(owned, &roster, true, b"approval").await?;
        let platform = selected_owner(owned)?.0;
        platform
            .entry_for_user(DECISION_USER)?
            .delete_credential()?;
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(append::recover_roster(owned).await.is_err());
        assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        Ok(())
    }
}
