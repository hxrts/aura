//! Private negative-denial acknowledgment under the original selected-provider root.
//! This authenticates denial but retains the original encrypted birth secret.
//! Physical erasure remains pending and this module is not wired into production.

use super::*;

const TOMBSTONE_USER: &str = "allocation-lifetime-negative-tombstone:v1";
const TOMBSTONE_FILE: &str = "platform_lifetime/tombstone.v1";
const TOMBSTONE_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-TOMBSTONE-V1";
const MARKER_MAGIC: &[u8] = b"AURA-PLATFORM-LIFETIME-TOMBSTONE-MARKER-V1";
const NONCE_DOMAIN: &[u8] = b"aura/platform-lifetime/tombstone-nonce/v1";
const MAX_MARKER_LEN: usize = 256;
mod erasure;

fn sealed_tombstone(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<Vec<u8>, AuraError> {
    let key = Zeroizing::new(selected.record_key(false)?);
    let mut nonce_input = Vec::with_capacity(NONCE_DOMAIN.len() + 64);
    nonce_input.extend_from_slice(NONCE_DOMAIN);
    nonce_input.extend_from_slice(&root.id);
    nonce_input.extend_from_slice(&root.decision_digest);
    let digest = aura_core::hash::hash(&nonce_input);
    let nonce = &digest[..12];
    let mut plain = Zeroizing::new(Vec::with_capacity(160));
    plain.extend_from_slice(&root.id);
    plain.extend_from_slice(&root.birth_id);
    plain.extend_from_slice(&root.decision_digest);
    plain.extend_from_slice(&root.profile);
    plain.extend_from_slice(&root.service);
    let ciphertext = ChaCha20Poly1305::new((&*key).into())
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &plain,
                aad: TOMBSTONE_MAGIC,
            },
        )
        .map_err(|source| {
            AuraError::crypto_with_source("platform tombstone encryption", Arc::new(source))
        })?;
    let mut bytes = Vec::with_capacity(TOMBSTONE_MAGIC.len() + 12 + ciphertext.len());
    bytes.extend_from_slice(TOMBSTONE_MAGIC);
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(&ciphertext);
    Ok(bytes)
}

fn marker(root: &RootRecord, tombstone: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MARKER_MAGIC.len() + 128);
    bytes.extend_from_slice(MARKER_MAGIC);
    bytes.extend_from_slice(&root.id);
    bytes.extend_from_slice(&root.birth_id);
    bytes.extend_from_slice(&root.decision_digest);
    bytes.extend_from_slice(&aura_core::hash::hash(tombstone));
    bytes
}

fn marker_state(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    tombstone: &[u8],
) -> Result<bool, AuraError> {
    match owned
        ._owner
        .directory
        .read_private_publication(Path::new(TOMBSTONE_FILE), MAX_MARKER_LEN)
        .map_err(io)?
    {
        None => Ok(false),
        Some((bytes, None)) if bytes == marker(root, tombstone) => Ok(true),
        _ => Err(invalid(
            "platform denial marker differs from original tombstone",
        )),
    }
}

fn publish_marker(
    owned: &ProfileOwnedSecureStorage,
    root: &RootRecord,
    tombstone: &[u8],
) -> Result<(), AuraError> {
    let staged = owned
        ._owner
        .directory
        .prepare_private(Path::new(TOMBSTONE_FILE), &marker(root, tombstone))
        .map_err(io)?;
    if !staged.publish(true).map_err(io)? {
        return Err(invalid("platform denial marker publication conflicted"));
    }
    staged.acknowledge().map_err(io)
}

/// Validate denial evidence before recovered birth custody is transferred.
/// Pending missing evidence can be reconstructed from the original negative
/// decision; acknowledged evidence loss remains a conflict.
pub(super) fn validate_recovery(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
) -> Result<(), AuraError> {
    if !matches!(
        root.phase,
        TOMBSTONE_PENDING | TOMBSTONE_ACK_ERASURE_PENDING | ERASURE_PENDING | ERASURE_COMPLETE
    ) {
        if read_entry(selected, TOMBSTONE_USER)?.is_some()
            || owned
                ._owner
                .directory
                .read_private_publication(Path::new(TOMBSTONE_FILE), MAX_MARKER_LEN)
                .map_err(io)?
                .is_some()
        {
            return Err(invalid(
                "platform denial evidence lacks original root intent",
            ));
        }
        return Ok(());
    }
    let decision = read_record(selected, root)?
        .ok_or_else(|| invalid("platform denial lacks original first decision"))?;
    if decision.kind != NEGATIVE {
        return Err(invalid("platform denial lacks original negative decision"));
    }
    let expected = sealed_tombstone(selected, root)?;
    match read_entry(selected, TOMBSTONE_USER)? {
        Some(observed) if observed == expected => {}
        None if root.phase == TOMBSTONE_PENDING => {}
        _ => return Err(invalid("platform denial tombstone is missing or different")),
    }
    if !marker_state(owned, root, &expected)? && root.phase != TOMBSTONE_PENDING {
        return Err(invalid("acknowledged platform denial marker is missing"));
    }
    Ok(())
}

pub(super) fn recover_erasure(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(), AuraError> {
    erasure::recover(owned, selected, root)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootIntent,
    Tombstone,
    Marker,
}

fn finish_original(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    mut root: RootRecord,
    stop: StopAfter,
) -> Result<(), AuraError> {
    let expected = sealed_tombstone(selected, &root)?;
    match read_entry(selected, TOMBSTONE_USER)? {
        Some(observed) if observed == expected => {}
        None if root.phase == TOMBSTONE_PENDING => {
            write_exact(selected, TOMBSTONE_USER, &expected)?;
        }
        _ => return Err(invalid("platform denial tombstone is missing or different")),
    }
    if stop == StopAfter::Tombstone {
        return Err(interrupted());
    }
    match marker_state(owned, &root, &expected)? {
        true => {}
        false if root.phase == TOMBSTONE_PENDING => publish_marker(owned, &root, &expected)?,
        false => return Err(invalid("acknowledged platform denial marker is missing")),
    }
    if stop == StopAfter::Marker {
        return Err(interrupted());
    }
    if root.phase == TOMBSTONE_PENDING {
        root.phase = TOMBSTONE_ACK_ERASURE_PENDING;
        write_keyring(selected, &root)?;
    }
    Ok(())
}

/// Acknowledge the exact original negative decision under the selected owner.
/// This denies live reads; original birth credentials remain pending erasure.
#[allow(dead_code)]
pub(super) async fn acknowledge_negative_denial(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
    negative_decision: &[u8],
) -> Result<(), AuraError> {
    acknowledge_inner(owned, birth, negative_decision, StopAfter::Never).await
}

async fn acknowledge_inner(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
    negative_decision: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _intent) = verify_birth(owned, selected, birth)?;
    acknowledge_locked(owned, selected, root, negative_decision, stop)
}

/// The same first-entry denial, authorized by complete roster custody after
/// append. The earlier single-birth token cannot authorize this path.
#[allow(dead_code)]
pub(super) async fn acknowledge_roster(
    owned: &ProfileOwnedSecureStorage,
    roster: &append::PlatformRosterAdmission,
    negative_decision: &[u8],
) -> Result<(), AuraError> {
    acknowledge_roster_inner(owned, roster, negative_decision, StopAfter::Never).await
}

async fn acknowledge_roster_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: &append::PlatformRosterAdmission,
    negative_decision: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let root = append::verify_roster_owner(owned, selected, roster)?;
    acknowledge_locked(owned, selected, root, negative_decision, stop)
}

fn acknowledge_locked(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    mut root: RootRecord,
    negative_decision: &[u8],
    stop: StopAfter,
) -> Result<(), AuraError> {
    super::validate_recovery(owned, selected, &root)?;
    let decision = read_record(selected, &root)?
        .ok_or_else(|| invalid("platform denial lacks original first decision"))?;
    if decision.kind != NEGATIVE || decision.bytes.as_slice() != negative_decision {
        return Err(invalid(
            "platform denial lacks exact original negative decision",
        ));
    }
    if root.phase == DECISION_COMPLETE {
        root.phase = TOMBSTONE_PENDING;
        write_keyring(selected, &root)?;
        if stop == StopAfter::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(
        root.phase,
        TOMBSTONE_PENDING | TOMBSTONE_ACK_ERASURE_PENDING
    ) {
        return Err(invalid(
            "platform denial requires acknowledged first decision",
        ));
    }
    finish_original(owned, selected, root, stop)
}

/// Reopen only the original pending or acknowledged negative tombstone.
#[allow(dead_code)]
pub(super) async fn reconcile_tombstone(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
) -> Result<bool, AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _intent) = verify_birth(owned, selected, birth)?;
    super::validate_recovery(owned, selected, &root)?;
    if matches!(
        root.phase,
        TOMBSTONE_PENDING | TOMBSTONE_ACK_ERASURE_PENDING
    ) {
        finish_original(owned, selected, root, StopAfter::Never)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    pub(super) async fn denied_roster() -> Result<
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
        let birth = allocate_one(owned, &empty, b"first", b"first secret").await?;
        append::append_second(owned, &birth, b"second", b"second secret").await?;
        drop(birth);
        drop(empty);
        drop(first);
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let roster = append::recover_roster(owned).await?;
        decide_roster(owned, &roster, false, b"first refusal").await?;
        Ok((fixture, selected, roster))
    }

    #[tokio::test]
    async fn first_roster_denial_preserves_second_and_retains_erasure_pending() -> TestResult {
        let (fixture, selected, roster) = denied_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        assert!(acknowledge_roster(owned, &roster, b"wrong refusal")
            .await
            .is_err());
        acknowledge_roster(owned, &roster, b"first refusal").await?;
        assert!(read_roster_first_secret(owned, &roster).await.is_err());
        assert_eq!(
            &**append::read_roster_second_secret(owned, &roster).await?,
            b"second secret"
        );
        let platform = selected_owner(owned)?.0;
        assert_eq!(
            read_root(owned, platform)?.phase,
            TOMBSTONE_ACK_ERASURE_PENDING
        );
        assert!(read_entry(platform, INTENT_USER)?.is_some());
        assert!(read_entry(platform, RECORD_USER)?.is_some());
        drop(roster);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let roster = append::recover_roster(owned).await?;
        assert!(read_roster_first_secret(owned, &roster).await.is_err());
        assert_eq!(
            &**append::read_roster_second_secret(owned, &roster).await?,
            b"second secret"
        );
        Ok(())
    }

    #[tokio::test]
    async fn first_roster_denial_interruptions_recover_exact_tombstone() -> TestResult {
        for stop in [
            StopAfter::RootIntent,
            StopAfter::Tombstone,
            StopAfter::Marker,
        ] {
            let (fixture, selected, roster) = denied_roster().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            assert!(
                acknowledge_roster_inner(owned, &roster, b"first refusal", stop)
                    .await
                    .is_err()
            );
            drop(roster);
            drop(selected);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let roster = append::recover_roster(owned).await?;
            acknowledge_roster(owned, &roster, b"first refusal").await?;
            assert_eq!(
                read_root(owned, selected_owner(owned)?.0)?.phase,
                TOMBSTONE_ACK_ERASURE_PENDING
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn original_negative_denial_is_exact_and_reopens_with_erasure_pending() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, false, b"original refusal").await?;
        acknowledge_negative_denial(owned, &birth, b"original refusal").await?;
        acknowledge_negative_denial(owned, &birth, b"original refusal").await?;
        assert!(read_live_secret(owned, &birth).await.is_err());
        let (platform, _) = selected_owner(owned)?;
        assert_eq!(
            read_root(owned, platform)?.phase,
            TOMBSTONE_ACK_ERASURE_PENDING
        );
        // A logical denial cannot be mistaken for physical deletion.
        assert!(read_entry(platform, INTENT_USER)?.is_some());
        assert!(read_entry(platform, RECORD_USER)?.is_some());
        drop(birth);
        drop(empty);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let birth = recover_one(owned).await?;
        assert!(reconcile_tombstone(owned, &birth).await?);
        assert!(read_live_secret(owned, &birth).await.is_err());
        let (platform, _) = selected_owner(owned)?;
        assert!(read_entry(platform, INTENT_USER)?.is_some());
        assert!(read_entry(platform, RECORD_USER)?.is_some());
        let (positive, original) = super::super::recover(owned, &birth)
            .await?
            .expect("original first decision");
        assert!(!positive);
        assert_eq!(&**original, b"original refusal");
        Ok(())
    }

    #[tokio::test]
    async fn denial_interruptions_reconcile_only_original_tombstone() -> TestResult {
        for stop in [
            StopAfter::RootIntent,
            StopAfter::Tombstone,
            StopAfter::Marker,
        ] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
            decide(owned, &birth, false, b"refusal").await?;
            assert!(acknowledge_inner(owned, &birth, b"refusal", stop)
                .await
                .is_err());
            assert!(read_live_secret(owned, &birth).await.is_err());
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let birth = recover_one(owned).await?;
            assert!(reconcile_tombstone(owned, &birth).await?);
            assert!(acknowledge_negative_denial(owned, &birth, b"refusal")
                .await
                .is_ok());
            assert!(read_live_secret(owned, &birth).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn positive_wrong_foreign_and_unexplained_denial_refuse() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, true, b"approval").await?;
        assert!(acknowledge_negative_denial(owned, &birth, b"approval")
            .await
            .is_err());
        let (platform, _) = selected_owner(owned)?;
        assert_eq!(read_root(owned, platform)?.phase, DECISION_COMPLETE);
        drop(birth);
        drop(empty);
        drop(selected);

        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, false, b"refusal").await?;
        assert!(
            acknowledge_negative_denial(owned, &birth, b"different refusal")
                .await
                .is_err()
        );
        let foreign = tempfile::tempdir()?;
        let foreign_selected = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(
            acknowledge_negative_denial(foreign_owned, &birth, b"refusal")
                .await
                .is_err()
        );
        let (platform, _) = selected_owner(owned)?;
        write_exact(platform, TOMBSTONE_USER, b"unexplained tombstone")?;
        assert!(acknowledge_negative_denial(owned, &birth, b"refusal")
            .await
            .is_err());
        assert_eq!(read_root(owned, platform)?.phase, DECISION_COMPLETE);
        Ok(())
    }

    #[tokio::test]
    async fn acknowledged_tombstone_or_marker_loss_refuses_recovered_owner() -> TestResult {
        for remove_marker in [false, true] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let empty = admit_empty_root(owned).await?;
            let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
            decide(owned, &birth, false, b"refusal").await?;
            acknowledge_negative_denial(owned, &birth, b"refusal").await?;
            let (platform, _) = selected_owner(owned)?;
            if remove_marker {
                std::fs::remove_file(fixture.profile.path().join(TOMBSTONE_FILE))?;
            } else {
                platform
                    .entry_for_user(TOMBSTONE_USER)?
                    .delete_credential()?;
            }
            drop(birth);
            drop(empty);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(recover_one(owned).await.is_err());
        }
        Ok(())
    }
}
