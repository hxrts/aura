//! Original third-entry decision, negative denial and selected-provider erasure.
//! The V9 authenticated root anchors every mutable step before per-ID writes.

use super::super::super::decision::{DecisionRecord, NEGATIVE, POSITIVE};
use super::*;
use aura_core::effects::secret_lifetime::MAX_DECISION_BYTES;

const DECISION_PREFIX: &str = "allocation-lifetime-third-decision:v9:";
const TOMBSTONE_PREFIX: &str = "allocation-lifetime-third-tombstone:v9:";
const TOMBSTONE_MAGIC: &[u8] = b"AURA-PLATFORM-THIRD-TOMBSTONE-V9";
const TOMBSTONE_NONCE_DOMAIN: &[u8] = b"aura/platform-third-tombstone-nonce/v9";
const TOMBSTONE_MARKER_MAGIC: &[u8] = b"AURA-PLATFORM-THIRD-TOMBSTONE-MARKER-V9";
const MAX_MARKER_LEN: usize = 256;

fn decision_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/third-decision-{}.v9", hex::encode(id))
}

fn tombstone_file(id: &[u8; 32]) -> String {
    format!("platform_lifetime/third-tombstone-{}.v9", hex::encode(id))
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
        _ => Err(invalid("platform third transition marker differs")),
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
        return Err(invalid("platform third transition marker conflicted"));
    }
    staged.acknowledge().map_err(io)
}

pub(super) fn decision_record(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Option<DecisionRecord>, AuraError> {
    let bytes = read_entry(selected, &user(DECISION_PREFIX, &root.third_id))?;
    if root.third_phase == THIRD_BIRTH_COMPLETE {
        if bytes.is_some() {
            return Err(invalid("platform third decision lacks root intent"));
        }
        return Ok(None);
    }
    let bytes = bytes.ok_or_else(|| invalid("platform third decision record is missing"))?;
    if aura_core::hash::hash(&bytes) != root.third_decision_digest {
        return Err(invalid("platform third decision differs from root intent"));
    }
    let record = DecisionRecord::decode(selected, &bytes)?;
    if record.root != root.id
        || record.allocation != root.third_id
        || record.profile != root.profile
        || record.service != root.service
    {
        return Err(invalid("platform third decision has foreign identity"));
    }
    Ok(Some(record))
}

fn finish_decision(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<DecisionRecord, AuraError> {
    let record = decision_record(selected, root)?
        .ok_or_else(|| invalid("platform third decision has not started"))?;
    let path = decision_file(&root.third_id);
    if !marker_state(owned, &path, &record.marker())? {
        if root.third_phase == THIRD_DECISION_PENDING {
            publish_marker(owned, &path, &record.marker())?;
        } else {
            return Err(invalid(
                "completed platform third decision marker is missing",
            ));
        }
    }
    if root.third_phase == THIRD_DECISION_PENDING {
        root.third_phase = THIRD_DECISION_COMPLETE;
        write_keyring(selected, root)?;
    }
    Ok(record)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecisionStop {
    Never,
    RootIntent,
    Record,
    Marker,
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn decide(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalThirdOwner::Three(three),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn decide_four(
    owned: &ProfileOwnedSecureStorage,
    four: &fourth::PlatformFourRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalThirdOwner::Four(four),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn decide_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &fourth::generic::PlatformGenericRosterAdmission,
    positive: bool,
    bytes: &[u8],
) -> Result<(), AuraError> {
    decide_inner(
        owned,
        OriginalThirdOwner::Generic(generic),
        positive,
        bytes,
        DecisionStop::Never,
    )
    .await
}

async fn decide_inner(
    owned: &ProfileOwnedSecureStorage,
    three: OriginalThirdOwner<'_>,
    positive: bool,
    bytes: &[u8],
    stop: DecisionStop,
) -> Result<(), AuraError> {
    if bytes.is_empty() || bytes.len() > MAX_DECISION_BYTES {
        return Err(invalid("platform third decision size is invalid"));
    }
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, _, _) = verify_original_third(owned, selected, three)?;
    if root.third_phase != THIRD_BIRTH_COMPLETE {
        let existing = finish_decision(owned, selected, &mut root)?;
        if existing.kind != if positive { POSITIVE } else { NEGATIVE }
            || existing.bytes.as_slice() != bytes
        {
            return Err(invalid("platform third decision contradicts original"));
        }
        return Ok(());
    }
    if read_entry(selected, &user(DECISION_PREFIX, &root.third_id))?.is_some()
        || marker_state(owned, &decision_file(&root.third_id), &[])?
    {
        return Err(invalid("platform third decision has unexplained evidence"));
    }
    let record = DecisionRecord {
        kind: if positive { POSITIVE } else { NEGATIVE },
        root: root.id,
        allocation: root.third_id,
        profile: root.profile,
        service: root.service,
        bytes: Zeroizing::new(bytes.to_vec()),
    };
    let encoded = record.encode(selected)?;
    root.third_phase = THIRD_DECISION_PENDING;
    root.third_decision_digest = aura_core::hash::hash(&encoded);
    write_keyring(selected, &root)?;
    if stop == DecisionStop::RootIntent {
        return Err(interrupted());
    }
    write_exact(selected, &user(DECISION_PREFIX, &root.third_id), &encoded)?;
    if stop == DecisionStop::Record {
        return Err(interrupted());
    }
    publish_marker(owned, &decision_file(&root.third_id), &record.marker())?;
    if stop == DecisionStop::Marker {
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
    nonce_input.extend_from_slice(&root.third_id);
    nonce_input.extend_from_slice(&root.third_decision_digest);
    let digest = aura_core::hash::hash(&nonce_input);
    let mut plain = Zeroizing::new(Vec::with_capacity(192));
    plain.extend_from_slice(&root.id);
    plain.extend_from_slice(&root.third_id);
    plain.extend_from_slice(&root.third_decision_digest);
    plain.extend_from_slice(&root.third_intent_digest);
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
            AuraError::crypto_with_source("platform third tombstone encryption", Arc::new(source))
        })?;
    let mut encoded = Vec::with_capacity(TOMBSTONE_MAGIC.len() + 12 + ciphertext.len());
    encoded.extend_from_slice(TOMBSTONE_MAGIC);
    encoded.extend_from_slice(&digest[..12]);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

fn tombstone_marker(root: &RootRecord, bytes: &[u8]) -> Vec<u8> {
    let mut marker = Vec::with_capacity(TOMBSTONE_MARKER_MAGIC.len() + 160);
    marker.extend_from_slice(TOMBSTONE_MARKER_MAGIC);
    marker.extend_from_slice(&root.id);
    marker.extend_from_slice(&root.third_id);
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
        .ok_or_else(|| invalid("platform third denial lacks decision"))?;
    if decision.kind != NEGATIVE {
        return Err(invalid("platform third denial lacks negative decision"));
    }
    let bytes = sealed_tombstone(selected, root)?;
    if aura_core::hash::hash(&bytes) != root.third_tombstone_digest {
        return Err(invalid("platform third tombstone differs from root intent"));
    }
    let user = user(TOMBSTONE_PREFIX, &root.third_id);
    match read_entry(selected, &user)? {
        Some(observed) if observed == bytes => {}
        None if root.third_phase == THIRD_TOMBSTONE_PENDING => {
            write_exact(selected, &user, &bytes)?;
        }
        _ => return Err(invalid("platform third tombstone is missing or different")),
    }
    let path = tombstone_file(&root.third_id);
    let marker = tombstone_marker(root, &bytes);
    if !marker_state(owned, &path, &marker)? {
        if root.third_phase == THIRD_TOMBSTONE_PENDING {
            publish_marker(owned, &path, &marker)?;
        } else {
            return Err(invalid(
                "acknowledged platform third tombstone marker is missing",
            ));
        }
    }
    if root.third_phase == THIRD_TOMBSTONE_PENDING {
        root.third_phase = THIRD_TOMBSTONE_ACK_ERASURE_PENDING;
        write_keyring(selected, root)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DenialStop {
    Never,
    RootIntent,
    Tombstone,
    Marker,
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_negative(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalThirdOwner::Three(three),
        exact_decision,
        DenialStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_four(
    owned: &ProfileOwnedSecureStorage,
    four: &fourth::PlatformFourRosterAdmission,
    exact_decision: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalThirdOwner::Four(four),
        exact_decision,
        DenialStop::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn acknowledge_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &fourth::generic::PlatformGenericRosterAdmission,
    bytes: &[u8],
) -> Result<(), AuraError> {
    deny_inner(
        owned,
        OriginalThirdOwner::Generic(generic),
        bytes,
        DenialStop::Never,
    )
    .await
}

async fn deny_inner(
    owned: &ProfileOwnedSecureStorage,
    three: OriginalThirdOwner<'_>,
    exact_decision: &[u8],
    stop: DenialStop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, _, _) = verify_original_third(owned, selected, three)?;
    let decision = finish_decision(owned, selected, &mut root)?;
    if decision.kind != NEGATIVE || decision.bytes.as_slice() != exact_decision {
        return Err(invalid(
            "platform third denial lacks exact negative decision",
        ));
    }
    if root.third_phase == THIRD_DECISION_COMPLETE {
        let bytes = sealed_tombstone(selected, &root)?;
        root.third_phase = THIRD_TOMBSTONE_PENDING;
        root.third_tombstone_digest = aura_core::hash::hash(&bytes);
        write_keyring(selected, &root)?;
        if stop == DenialStop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.third_phase,
        THIRD_TOMBSTONE_PENDING | THIRD_TOMBSTONE_ACK_ERASURE_PENDING
    ) {
        return Err(invalid("platform third denial has invalid phase"));
    }
    if stop == DenialStop::Tombstone {
        let bytes = sealed_tombstone(selected, &root)?;
        write_exact(selected, &user(TOMBSTONE_PREFIX, &root.third_id), &bytes)?;
        return Err(interrupted());
    }
    if stop == DenialStop::Marker {
        let bytes = sealed_tombstone(selected, &root)?;
        write_exact(selected, &user(TOMBSTONE_PREFIX, &root.third_id), &bytes)?;
        publish_marker(
            owned,
            &tombstone_file(&root.third_id),
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
        Some(bytes) if aura_core::hash::hash(&bytes) == root.third_intent_digest => Ok(true),
        Some(_) => Err(invalid("platform third erasure found changed credential")),
        None if root.third_phase >= THIRD_ERASURE_PENDING => Ok(false),
        None => Err(invalid(
            "platform third erasure original credential is missing",
        )),
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
            "platform third credential deletion was not acknowledged",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErasureStop {
    Never,
    RootIntent,
    RecordDelete,
    IntentDelete,
}

fn finish_erasure(
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    stop: ErasureStop,
) -> Result<(), AuraError> {
    let record_user = user(RECORD_PREFIX, &root.third_id);
    let intent_user = user(INTENT_PREFIX, &root.third_id);
    let record = survivor(selected, root, &record_user)?;
    let intent = survivor(selected, root, &intent_user)?;
    if root.third_phase == THIRD_ERASURE_COMPLETE {
        if record || intent {
            return Err(invalid("completed platform third credential reappeared"));
        }
        return Ok(());
    }
    if record {
        delete_and_readback(selected, &record_user)?;
    }
    if stop == ErasureStop::RecordDelete {
        return Err(interrupted());
    }
    if intent {
        delete_and_readback(selected, &intent_user)?;
    }
    if stop == ErasureStop::IntentDelete {
        return Err(interrupted());
    }
    root.third_phase = THIRD_ERASURE_COMPLETE;
    write_keyring(selected, root)
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn erase(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, OriginalThirdOwner::Three(three), ErasureStop::Never).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_four(
    owned: &ProfileOwnedSecureStorage,
    four: &fourth::PlatformFourRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, OriginalThirdOwner::Four(four), ErasureStop::Never).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &fourth::generic::PlatformGenericRosterAdmission,
) -> Result<(), AuraError> {
    erase_inner(
        owned,
        OriginalThirdOwner::Generic(generic),
        ErasureStop::Never,
    )
    .await
}

async fn erase_inner(
    owned: &ProfileOwnedSecureStorage,
    three: OriginalThirdOwner<'_>,
    stop: ErasureStop,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (mut root, _, _) = verify_original_third(owned, selected, three)?;
    finish_decision(owned, selected, &mut root)?;
    finish_tombstone(owned, selected, &mut root)?;
    if root.third_phase == THIRD_TOMBSTONE_ACK_ERASURE_PENDING {
        if !survivor(selected, &root, &user(RECORD_PREFIX, &root.third_id))?
            || !survivor(selected, &root, &user(INTENT_PREFIX, &root.third_id))?
        {
            return Err(invalid("platform third erasure lacks original credentials"));
        }
        root.third_phase = THIRD_ERASURE_PENDING;
        write_keyring(selected, &root)?;
        if stop == ErasureStop::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.third_phase,
        THIRD_ERASURE_PENDING | THIRD_ERASURE_COMPLETE
    ) {
        return Err(invalid("platform third erasure lacks acknowledged denial"));
    }
    finish_erasure(selected, &mut root, stop)
}

pub(super) fn recover(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    _scope: &[u8],
) -> Result<(), AuraError> {
    if root.third_phase == THIRD_BIRTH_COMPLETE {
        if decision_record(selected, root)?.is_some()
            || read_entry(selected, &user(TOMBSTONE_PREFIX, &root.third_id))?.is_some()
            || marker_state(owned, &decision_file(&root.third_id), &[])?
            || marker_state(owned, &tombstone_file(&root.third_id), &[])?
        {
            return Err(invalid(
                "platform third transition evidence lacks root intent",
            ));
        }
        return Ok(());
    }
    finish_decision(owned, selected, root)?;
    if root.third_phase >= THIRD_TOMBSTONE_PENDING {
        finish_tombstone(owned, selected, root)?;
    } else if read_entry(selected, &user(TOMBSTONE_PREFIX, &root.third_id))?.is_some()
        || marker_state(owned, &tombstone_file(&root.third_id), &[])?
    {
        return Err(invalid("platform third tombstone lacks root intent"));
    }
    if root.third_phase >= THIRD_ERASURE_PENDING {
        finish_erasure(selected, root, ErasureStop::Never)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn read_live_secret(
    owned: &ProfileOwnedSecureStorage,
    three: &PlatformThreeRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalThirdOwner::Three(three)).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn read_four_secret(
    owned: &ProfileOwnedSecureStorage,
    four: &fourth::PlatformFourRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalThirdOwner::Four(four)).await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn read_generic_secret(
    owned: &ProfileOwnedSecureStorage,
    generic: &fourth::generic::PlatformGenericRosterAdmission,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    read_owned_secret(owned, OriginalThirdOwner::Generic(generic)).await
}

async fn read_owned_secret(
    owned: &ProfileOwnedSecureStorage,
    three: OriginalThirdOwner<'_>,
) -> Result<Zeroizing<Vec<u8>>, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _, scope) = verify_original_third(owned, selected, three)?;
    if root.third_phase >= THIRD_TOMBSTONE_PENDING {
        return Err(invalid(
            "platform third live read denied by original tombstone",
        ));
    }
    let bytes = read_entry(selected, &user(INTENT_PREFIX, &root.third_id))?
        .ok_or_else(|| invalid("platform third original intent is missing"))?;
    if aura_core::hash::hash(&bytes) != root.third_intent_digest
        || read_entry(selected, &user(RECORD_PREFIX, &root.third_id))?.as_deref()
            != Some(bytes.as_slice())
    {
        return Err(invalid("platform third live credential differs"));
    }
    let intent = BirthIntent::decode(selected, &bytes)?;
    if intent.scope != scope {
        return Err(invalid("platform third live scope differs from original"));
    }
    Ok(intent.secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn first_three(
        fixture: &Fixture,
    ) -> Result<(ProductionSecureStorageHandler, PlatformThreeRosterAdmission), AuraError> {
        let (storage, two) = super::super::tests::first_two(fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        append_third(owned, &two, b"third retained scope", b"third secret").await?;
        let three = promote_after_third(owned, two)
            .await
            .map_err(|(error, _)| error)?;
        Ok((storage, three))
    }

    #[tokio::test]
    async fn third_positive_decision_preserves_live_secret_and_exact_owner() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        decide(owned, &three, true, b"third approval").await?;
        assert_eq!(&**read_live_secret(owned, &three).await?, b"third secret");
        assert!(decide(owned, &three, false, b"third approval")
            .await
            .is_err());
        assert!(acknowledge_negative(owned, &three, b"third approval")
            .await
            .is_err());
        assert!(
            erase(owned, &three).await.is_err(),
            "positive decision cannot erase"
        );
        let (selected, _) = selected_owner(owned)?;
        assert_eq!(
            read_root(owned, selected)?.third_phase,
            THIRD_DECISION_COMPLETE
        );
        Ok(())
    }

    #[tokio::test]
    async fn third_negative_erasure_retains_scope_but_refuses_secret_after_reopen() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        assert!(
            erase(owned, &three).await.is_err(),
            "birth alone cannot erase"
        );
        decide(owned, &three, false, b"third denial").await?;
        assert!(
            erase(owned, &three).await.is_err(),
            "denial needs tombstone ACK"
        );
        acknowledge_negative(owned, &three, b"third denial").await?;
        assert!(read_live_secret(owned, &three).await.is_err());
        let (selected, _) = selected_owner(owned)?;
        let before = read_root(owned, selected)?;
        assert!(read_entry(selected, &user(INTENT_PREFIX, &before.third_id))?.is_some());
        erase(owned, &three).await?;
        let mut root = read_root(owned, selected)?;
        assert_eq!(root.third_phase, THIRD_ERASURE_COMPLETE);
        assert!(read_entry(selected, &user(INTENT_PREFIX, &root.third_id))?.is_none());
        assert!(read_entry(selected, &user(RECORD_PREFIX, &root.third_id))?.is_none());
        let claim = prior_claim(owned, selected, &mut root)?;
        assert_eq!(
            retained_scope(owned, &root, &claim)?,
            b"third retained scope"
        );
        drop(three);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let recovered = recover_three_roster(owned).await?;
        assert!(read_live_secret(owned, &recovered).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn third_decision_and_tombstone_replay_only_anchored_records() -> TestResult {
        for decision_stop in [DecisionStop::Record, DecisionStop::Marker] {
            let fixture = Fixture::new()?;
            let (storage, three) = first_three(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            assert!(decide_inner(
                owned,
                OriginalThirdOwner::Three(&three),
                false,
                b"third denial",
                decision_stop
            )
            .await
            .is_err());
            drop(three);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_three_roster(owned).await?;
            acknowledge_negative(owned, &recovered, b"third denial").await?;
        }
        for denial_stop in [
            DenialStop::RootIntent,
            DenialStop::Tombstone,
            DenialStop::Marker,
        ] {
            let fixture = Fixture::new()?;
            let (storage, three) = first_three(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            decide(owned, &three, false, b"third denial").await?;
            assert!(deny_inner(
                owned,
                OriginalThirdOwner::Three(&three),
                b"third denial",
                denial_stop
            )
            .await
            .is_err());
            drop(three);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_three_roster(owned).await?;
            assert!(read_live_secret(owned, &recovered).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn third_erasure_replays_record_and_intent_deletion() -> TestResult {
        for stop in [
            ErasureStop::RootIntent,
            ErasureStop::RecordDelete,
            ErasureStop::IntentDelete,
        ] {
            let fixture = Fixture::new()?;
            let (storage, three) = first_three(&fixture).await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
                unreachable!()
            };
            decide(owned, &three, false, b"third denial").await?;
            acknowledge_negative(owned, &three, b"third denial").await?;
            assert!(erase_inner(owned, OriginalThirdOwner::Three(&three), stop)
                .await
                .is_err());
            drop(three);
            drop(storage);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let recovered = recover_three_roster(owned).await?;
            let (selected, _) = selected_owner(owned)?;
            let root = read_root(owned, selected)?;
            assert_eq!(root.third_phase, THIRD_ERASURE_COMPLETE);
            assert!(read_live_secret(owned, &recovered).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn third_transition_refuses_foreign_owner_and_missing_provenance() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, three) = first_three(&fixture).await?;
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
        assert!(decide(foreign_owned, &three, true, b"wrong owner")
            .await
            .is_err());
        decide(owned, &three, false, b"third denial").await?;
        acknowledge_negative(owned, &three, b"third denial").await?;
        let (selected, _) = selected_owner(owned)?;
        let root = read_root(owned, selected)?;
        selected
            .entry_for_user(&user(TOMBSTONE_PREFIX, &root.third_id))?
            .delete_credential()?;
        assert!(recover_three_roster(owned).await.is_err());
        assert!(erase(owned, &three).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn third_recovery_preserves_reconciled_second_phase() -> TestResult {
        let fixture = Fixture::new()?;
        let (storage, two) = super::super::tests::first_two(&fixture).await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &storage else {
            unreachable!()
        };
        super::super::super::lifecycle::decide(owned, &two, true, b"second approval").await?;
        append_third(owned, &two, b"third retained scope", b"third secret").await?;
        let three = promote_after_third(owned, two)
            .await
            .map_err(|(error, _)| error)?;
        decide(owned, &three, true, b"third approval").await?;
        let (selected, _) = selected_owner(owned)?;
        let mut root = read_root(owned, selected)?;
        // Both markers and records exist. Model a crash before each root
        // acknowledgement, then verify neither recovered phase is lost.
        root.second_phase = SECOND_DECISION_PENDING;
        root.third_phase = THIRD_DECISION_PENDING;
        write_keyring(selected, &root)?;
        drop(three);
        drop(storage);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        recover_three_roster(owned).await?;
        let (selected, _) = selected_owner(owned)?;
        let recovered = read_root(owned, selected)?;
        assert_eq!(recovered.second_phase, SECOND_DECISION_COMPLETE);
        assert_eq!(recovered.third_phase, THIRD_DECISION_COMPLETE);
        Ok(())
    }
}
