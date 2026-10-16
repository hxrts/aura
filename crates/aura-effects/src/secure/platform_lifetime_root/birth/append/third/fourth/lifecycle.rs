//! Root-authenticated fourth decision, denial, and selected-provider erasure.
//! Only the original four-entry lease can advance this private lifecycle.

use super::super::super::super::decision::{DecisionRecord, NEGATIVE, POSITIVE};
use super::*;
use aura_core::effects::secret_lifetime::MAX_DECISION_BYTES;

const DECISION_PREFIX: &str = "allocation-lifetime-fourth-decision:v11:";
const TOMBSTONE_PREFIX: &str = "allocation-lifetime-fourth-tombstone:v11:";
const TOMBSTONE_MAGIC: &[u8] = b"AURA-PLATFORM-FOURTH-TOMBSTONE-V11";
const TOMBSTONE_NONCE_DOMAIN: &[u8] = b"aura/platform-fourth-tombstone-nonce/v11";
const MAX_MARKER_LEN: usize = 256;

fn decision_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/fourth-decision-{}.v11", hex::encode(id))
}

fn tombstone_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/fourth-tombstone-{}.v11", hex::encode(id))
}

#[cfg(test)]
pub(super) fn cleanup_fixture(service: &str, id: &[u8; 32]) {
    for name in [user(DECISION_PREFIX, id), user(TOMBSTONE_PREFIX, id)] {
        if let Ok(entry) = keyring::Entry::new(service, &name) {
            let _ = entry.delete_credential();
        }
    }
}

fn marker_state(
    owned: &ProfileOwnedSecureStorage,
    path: &str,
    expected: &[u8],
) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(path), MAX_MARKER_LEN)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == expected => Ok(true),
        _ => Err(invalid("platform fourth transition marker differs")),
    }
}

fn publish_marker(
    owned: &ProfileOwnedSecureStorage,
    path: &str,
    expected: &[u8],
) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(path), expected)
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform fourth transition marker conflicted"));
    }
    staged.acknowledge().map_err(io)
}

pub(in crate::secure::platform_lifetime_root::birth) fn decision_record(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Option<DecisionRecord>, AuraError> {
    let bytes = read_entry(selected, &user(DECISION_PREFIX, &root.tail_next_id))?;
    if root.tail_phase == TAIL_APPEND_COMPLETE {
        if bytes.is_some() {
            return Err(invalid("platform fourth decision lacks root intent"));
        }
        return Ok(None);
    }
    let bytes = bytes.ok_or_else(|| invalid("platform fourth decision is missing"))?;
    if aura_core::hash::hash(&bytes) != root.tail_decision_digest {
        return Err(invalid("platform fourth decision differs from root intent"));
    }
    let record = DecisionRecord::decode(selected, &bytes)?;
    if record.root != root.id
        || record.allocation != root.tail_next_id
        || record.profile != root.profile
        || record.service != root.service
    {
        return Err(invalid("platform fourth decision has foreign identity"));
    }
    Ok(Some(record))
}

fn finish_decision(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<DecisionRecord, AuraError> {
    let record = decision_record(selected, root)?
        .ok_or_else(|| invalid("platform fourth decision has not started"))?;
    let path = decision_file(&root.tail_next_id);
    if !marker_state(owned, &path, &record.marker())? {
        if root.tail_phase == TAIL_DECISION_PENDING {
            publish_marker(owned, &path, &record.marker())?;
        } else {
            return Err(invalid(
                "completed platform fourth decision marker is missing",
            ));
        }
    }
    if root.tail_phase == TAIL_DECISION_PENDING {
        root.tail_phase = TAIL_DECISION_COMPLETE;
        write_keyring(selected, root)?;
    }
    Ok(record)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Never,
    RootIntent,
    Record,
    Marker,
    RecordDelete,
    IntentDelete,
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn decide(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(owned, four, positive, bytes, Stop::Never).await
}

async fn decide_inner(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    positive: bool,
    bytes: &[u8],
    stop: Stop,
) -> Result<(), AuraError> {
    if bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES {
        return Err(invalid("platform fourth decision size is invalid"));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = verify_four(owned, selected, four)?;
    if root.tail_phase != TAIL_APPEND_COMPLETE {
        let existing = finish_decision(owned, selected, &mut root)?;
        if existing.kind != if positive { POSITIVE } else { NEGATIVE }
            || existing.bytes.as_slice() != bytes
        {
            return Err(invalid("platform fourth decision contradicts original"));
        }
        return Ok(());
    }
    if read_entry(selected, &user(DECISION_PREFIX, &root.tail_next_id))?.is_some()
        || marker_state(owned, &decision_file(&root.tail_next_id), &[])?
    {
        return Err(invalid("platform fourth decision has unexplained evidence"));
    }
    let record = DecisionRecord {
        kind: if positive { POSITIVE } else { NEGATIVE },
        root: root.id,
        allocation: root.tail_next_id,
        profile: root.profile,
        service: root.service,
        bytes: zeroize::Zeroizing::new(bytes.to_vec()),
    };
    let encoded = record.encode(selected)?;
    root.tail_phase = TAIL_DECISION_PENDING;
    root.tail_decision_digest = aura_core::hash::hash(&encoded);
    write_keyring(selected, &root)?;
    if stop == Stop::RootIntent {
        return Err(interrupted());
    }
    write_exact(
        selected,
        &user(DECISION_PREFIX, &root.tail_next_id),
        &encoded,
    )?;
    if stop == Stop::Record {
        return Err(interrupted());
    }
    publish_marker(owned, &decision_file(&root.tail_next_id), &record.marker())?;
    if stop == Stop::Marker {
        return Err(interrupted());
    }
    finish_decision(owned, selected, &mut root)?;
    Ok(())
}

fn sealed_tombstone(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Vec<u8>, AuraError> {
    let key = Zeroizing::new(selected.record_key(false)?);
    let mut nonce_input = Vec::with_capacity(TOMBSTONE_NONCE_DOMAIN.len() + 96);
    nonce_input.extend_from_slice(TOMBSTONE_NONCE_DOMAIN);
    nonce_input.extend_from_slice(&root.id);
    nonce_input.extend_from_slice(&root.tail_next_id);
    nonce_input.extend_from_slice(&root.tail_decision_digest);
    let digest = aura_core::hash::hash(&nonce_input);
    let mut plain = Zeroizing::new(Vec::with_capacity(192));
    plain.extend_from_slice(&root.id);
    plain.extend_from_slice(&root.tail_next_id);
    plain.extend_from_slice(&root.tail_decision_digest);
    plain.extend_from_slice(&root.tail_next_intent_digest);
    plain.extend_from_slice(&root.profile);
    plain.extend_from_slice(&root.service);
    let ciphertext = ChaCha20Poly1305::new((&*key).into())
        .encrypt(
            Nonce::from_slice(&digest[..12]),
            Payload {
                msg: &plain,
                aad: TOMBSTONE_MAGIC,
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("platform fourth tombstone encryption", Arc::new(source))
        })?;
    let mut encoded = Vec::with_capacity(TOMBSTONE_MAGIC.len() + 12 + ciphertext.len());
    encoded.extend_from_slice(TOMBSTONE_MAGIC);
    encoded.extend_from_slice(&digest[..12]);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

fn tombstone_marker(root: &RootRecord, bytes: &[u8]) -> Vec<u8> {
    let mut marker = Vec::with_capacity(192);
    marker.extend_from_slice(&root.id);
    marker.extend_from_slice(&root.tail_next_id);
    marker.extend_from_slice(&root.profile);
    marker.extend_from_slice(&root.service);
    marker.extend_from_slice(&aura_core::hash::hash(bytes));
    marker
}

fn finish_tombstone(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(), AuraError> {
    let decision = decision_record(selected, root)?
        .ok_or_else(|| invalid("platform fourth denial lacks decision"))?;
    if decision.kind != NEGATIVE {
        return Err(invalid("platform fourth denial lacks negative decision"));
    }
    let bytes = sealed_tombstone(selected, root)?;
    if aura_core::hash::hash(&bytes) != root.tail_tombstone_digest {
        return Err(invalid(
            "platform fourth tombstone differs from root intent",
        ));
    }
    let user = user(TOMBSTONE_PREFIX, &root.tail_next_id);
    match read_entry(selected, &user)? {
        Some(observed) if observed == bytes => {}
        None if root.tail_phase == TAIL_TOMBSTONE_PENDING => write_exact(selected, &user, &bytes)?,
        _ => return Err(invalid("platform fourth tombstone missing or different")),
    }
    let path = tombstone_file(&root.tail_next_id);
    let marker = tombstone_marker(root, &bytes);
    if !marker_state(owned, &path, &marker)? {
        if root.tail_phase == TAIL_TOMBSTONE_PENDING {
            publish_marker(owned, &path, &marker)?;
        } else {
            return Err(invalid(
                "acknowledged platform fourth tombstone marker is missing",
            ));
        }
    }
    if root.tail_phase == TAIL_TOMBSTONE_PENDING {
        root.tail_phase = TAIL_TOMBSTONE_ACK_ERASURE_PENDING;
        write_keyring(selected, root)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_negative(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(owned, four, exact_decision, Stop::Never).await
}

async fn deny_inner(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    exact_decision: &[u8],
    stop: Stop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = verify_four(owned, selected, four)?;
    let decision = finish_decision(owned, selected, &mut root)?;
    if decision.kind != NEGATIVE || decision.bytes.as_slice() != exact_decision {
        return Err(invalid(
            "platform fourth denial lacks exact negative decision",
        ));
    }
    if root.tail_phase == TAIL_DECISION_COMPLETE {
        root.tail_phase = TAIL_TOMBSTONE_PENDING;
        root.tail_tombstone_digest = aura_core::hash::hash(&sealed_tombstone(selected, &root)?);
        write_keyring(selected, &root)?;
        if stop == Stop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.tail_phase,
        TAIL_TOMBSTONE_PENDING | TAIL_TOMBSTONE_ACK_ERASURE_PENDING
    ) {
        return Err(invalid("platform fourth denial has invalid phase"));
    }
    if stop == Stop::Record {
        write_exact(
            selected,
            &user(TOMBSTONE_PREFIX, &root.tail_next_id),
            &sealed_tombstone(selected, &root)?,
        )?;
        return Err(interrupted());
    }
    if stop == Stop::Marker {
        let bytes = sealed_tombstone(selected, &root)?;
        write_exact(
            selected,
            &user(TOMBSTONE_PREFIX, &root.tail_next_id),
            &bytes,
        )?;
        publish_marker(
            owned,
            &tombstone_file(&root.tail_next_id),
            &tombstone_marker(&root, &bytes),
        )?;
        return Err(interrupted());
    }
    finish_tombstone(owned, selected, &mut root)
}

fn survivor(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    user: &str,
) -> Result<bool, AuraError> {
    match read_entry(selected, user)? {
        Some(bytes) if aura_core::hash::hash(&bytes) == root.tail_next_intent_digest => Ok(true),
        Some(_) => Err(invalid("platform fourth erasure found changed credential")),
        None if root.tail_phase >= TAIL_ERASURE_PENDING => Ok(false),
        None => Err(invalid("platform fourth original credential is missing")),
    }
}

fn delete_and_readback(
    selected: &PlatformSecureStorageHandler,
    user: &str,
) -> Result<(), AuraError> {
    let entry = selected.entry_for_user(user)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(source) => return Err(PlatformSecureStorageHandler::map_keyring_error(source)),
    }
    if read_entry(selected, user)?.is_some() {
        return Err(invalid(
            "platform fourth credential deletion was not acknowledged",
        ));
    }
    Ok(())
}

fn finish_erasure(
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    stop: Stop,
) -> Result<(), AuraError> {
    let record_user = user(RECORD_PREFIX, &root.tail_next_id);
    let intent_user = user(INTENT_PREFIX, &root.tail_next_id);
    let record = survivor(selected, root, &record_user)?;
    let intent = survivor(selected, root, &intent_user)?;
    if root.tail_phase == TAIL_ERASURE_COMPLETE {
        if record || intent {
            return Err(invalid("completed platform fourth credential reappeared"));
        }
        return Ok(());
    }
    if record {
        delete_and_readback(selected, &record_user)?;
    }
    if stop == Stop::RecordDelete {
        return Err(interrupted());
    }
    if intent {
        delete_and_readback(selected, &intent_user)?;
    }
    if stop == Stop::IntentDelete {
        return Err(interrupted());
    }
    root.tail_phase = TAIL_ERASURE_COMPLETE;
    write_keyring(selected, root)
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn erase(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, four, Stop::Never).await
}

async fn erase_inner(
    owned: &ProfileOwnedSecureStorage,
    four: &PlatformFourRosterAdmission,
    stop: Stop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let mut root = verify_four(owned, selected, four)?;
    finish_decision(owned, selected, &mut root)?;
    finish_tombstone(owned, selected, &mut root)?;
    if root.tail_phase == TAIL_TOMBSTONE_ACK_ERASURE_PENDING {
        if !survivor(selected, &root, &user(RECORD_PREFIX, &root.tail_next_id))?
            || !survivor(selected, &root, &user(INTENT_PREFIX, &root.tail_next_id))?
        {
            return Err(invalid(
                "platform fourth erasure lacks original credentials",
            ));
        }
        root.tail_phase = TAIL_ERASURE_PENDING;
        write_keyring(selected, &root)?;
        if stop == Stop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.tail_phase,
        TAIL_ERASURE_PENDING | TAIL_ERASURE_COMPLETE
    ) {
        return Err(invalid("platform fourth erasure lacks acknowledged denial"));
    }
    finish_erasure(selected, &mut root, stop)
}

pub(super) fn recover(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(), AuraError> {
    if root.tail_phase == TAIL_APPEND_COMPLETE {
        if decision_record(selected, root)?.is_some()
            || read_entry(selected, &user(TOMBSTONE_PREFIX, &root.tail_next_id))?.is_some()
            || marker_state(owned, &decision_file(&root.tail_next_id), &[])?
            || marker_state(owned, &tombstone_file(&root.tail_next_id), &[])?
        {
            return Err(invalid("platform fourth transition lacks root intent"));
        }
        return Ok(());
    }
    finish_decision(owned, selected, root)?;
    if root.tail_phase >= TAIL_TOMBSTONE_PENDING {
        finish_tombstone(owned, selected, root)?;
    } else if read_entry(selected, &user(TOMBSTONE_PREFIX, &root.tail_next_id))?.is_some()
        || marker_state(owned, &tombstone_file(&root.tail_next_id), &[])?
    {
        return Err(invalid("platform fourth tombstone lacks root intent"));
    }
    if root.tail_phase >= TAIL_ERASURE_PENDING {
        finish_erasure(selected, root, Stop::Never)?;
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::secure::platform_lifetime_root::birth::append::third::fourth) mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    pub(in crate::secure::platform_lifetime_root::birth::append::third::fourth) async fn first_four(
        fixture: &Fixture,
    ) -> Result<(ProductionSecureStorageHandler, PlatformFourRosterAdmission), AuraError> {
        let (storage, three) = super::super::tests::first_three(fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_fourth(owned, &three, b"fourth retained scope", b"fourth secret").await?;
        let four = promote_after_fourth(owned, three)
            .await
            .map_err(|(error, _)| error)?;
        Ok((storage, four))
    }

    #[tokio::test]
    async fn fourth_positive_decision_retains_original_secret_and_refuses_denial() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, four) = first_four(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        decide(owned, &four, true, b"fourth approval").await?;
        assert_eq!(&**read_fourth_secret(owned, &four).await?, b"fourth secret");
        assert!(decide(owned, &four, false, b"fourth approval")
            .await
            .is_err());
        assert!(acknowledge_negative(owned, &four, b"fourth approval")
            .await
            .is_err());
        assert!(erase(owned, &four).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn fourth_negative_erasure_retains_scope_and_refuses_secret_after_reopen() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, four) = first_four(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        assert!(erase(owned, &four).await.is_err());
        decide(owned, &four, false, b"fourth denial").await?;
        assert!(erase(owned, &four).await.is_err());
        acknowledge_negative(owned, &four, b"fourth denial").await?;
        assert!(read_fourth_secret(owned, &four).await.is_err());
        erase(owned, &four).await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        assert_eq!(root.tail_phase, TAIL_ERASURE_COMPLETE);
        assert!(read_entry(selected, &user(INTENT_PREFIX, &root.tail_next_id))?.is_none());
        assert!(read_entry(selected, &user(RECORD_PREFIX, &root.tail_next_id))?.is_none());
        let (roster, None) = owned
            ._owner
            .directory
            .read_private_publication(Path::new(FOURTH_ROSTER_FILE), MAX_V10_ROSTER_FILE)?
            .expect("roster")
        else {
            panic!("incomplete roster")
        };
        let fourth = entries(&roster, ROSTER_MAGIC_V10, &root)?
            .into_iter()
            .find(|entry| entry[32..64] == root.tail_next_id)
            .expect("fourth scope");
        assert_eq!(&fourth[ROSTER_ENTRY_HEADER_LEN..], b"fourth retained scope");
        drop(four);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let recovered = recover_four_roster(owned).await?;
        assert!(read_fourth_secret(owned, &recovered).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn fourth_decision_and_denial_replay_only_root_anchored_records() -> TestResult {
        for stop in [Stop::Record, Stop::Marker] {
            let fixture = Fixture::new()?;
            let (storage, four) = first_four(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(decide_inner(owned, &four, false, b"fourth denial", stop)
                .await
                .is_err());
            drop(four);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_four_roster(owned).await?;
            acknowledge_negative(owned, &recovered, b"fourth denial").await?;
        }
        for stop in [Stop::RootIntent, Stop::Record, Stop::Marker] {
            let fixture = Fixture::new()?;
            let (storage, four) = first_four(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            decide(owned, &four, false, b"fourth denial").await?;
            assert!(deny_inner(owned, &four, b"fourth denial", stop)
                .await
                .is_err());
            drop(four);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_four_roster(owned).await?;
            assert!(read_fourth_secret(owned, &recovered).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn fourth_erasure_replays_each_original_credential_delete() -> TestResult {
        for stop in [Stop::RootIntent, Stop::RecordDelete, Stop::IntentDelete] {
            let fixture = Fixture::new()?;
            let (storage, four) = first_four(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            decide(owned, &four, false, b"fourth denial").await?;
            acknowledge_negative(owned, &four, b"fourth denial").await?;
            assert!(erase_inner(owned, &four, stop).await.is_err());
            drop(four);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_four_roster(owned).await?;
            let (selected, _) = selected_owner(owned)?;
            assert_eq!(
                read_root(owned, selected)?.tail_phase,
                TAIL_ERASURE_COMPLETE
            );
            assert!(read_fourth_secret(owned, &recovered).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn fourth_transition_refuses_foreign_owner_and_missing_provenance() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, four) = first_four(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        let foreign = tempfile::tempdir()?;
        let foreign_storage = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_storage else {
            unreachable!()
        };
        assert!(decide(foreign_owned, &four, true, b"wrong owner")
            .await
            .is_err());
        decide(owned, &four, false, b"fourth denial").await?;
        acknowledge_negative(owned, &four, b"fourth denial").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        selected
            .entry_for_user(&user(TOMBSTONE_PREFIX, &root.tail_next_id))?
            .delete_credential()?;
        assert!(recover_four_roster(owned).await.is_err());
        assert!(erase(owned, &four).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn fourth_tombstone_seals_identity_and_refuses_changed_ciphertext_on_reopen() -> TestResult
    {
        let fixture = Fixture::new()?;
        let (storage, four) = first_four(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        decide(owned, &four, false, b"fourth denial").await?;
        acknowledge_negative(owned, &four, b"fourth denial").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        let user = user(TOMBSTONE_PREFIX, &root.tail_next_id);
        let exact = read_entry(selected, &user)?.expect("sealed tombstone");
        assert_eq!(exact, sealed_tombstone(selected, &root)?);
        assert!(exact.starts_with(TOMBSTONE_MAGIC));
        for field in [
            root.id,
            root.tail_next_id,
            root.profile,
            root.service,
            root.tail_decision_digest,
            root.tail_next_intent_digest,
        ] {
            assert!(!exact.windows(field.len()).any(|window| window == field));
        }
        let mut foreign = root.clone();
        foreign.tail_next_id = [71; 32];
        assert_ne!(sealed_tombstone(selected, &foreign)?, exact);
        let mut changed = exact.clone();
        *changed.last_mut().expect("ciphertext") ^= 1;
        write_exact(selected, &user, &changed)?;
        drop(four);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(recover_four_roster(owned).await.is_err());
        Ok(())
    }
}
