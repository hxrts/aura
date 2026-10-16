//! Private selected-provider deletion of the original birth credentials.
//! The authenticated root, decision and tombstone remain as recovery evidence.
//! Completion promises only the selected provider's local delete/readback result.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopAfter {
    Never,
    RootIntent,
    RecordDelete,
    IntentDelete,
}

fn original_or_absent(
    selected: &PlatformSecureStorageHandler,
    root: &RootRecord,
    user: &str,
) -> Result<bool, AuraError> {
    match read_entry(selected, user)? {
        Some(bytes) if aura_core::hash::hash(&bytes) == root.intent_digest => Ok(true),
        Some(_) => Err(invalid(
            "platform erasure found different original credential",
        )),
        None if root.phase == ERASURE_PENDING || root.phase == ERASURE_COMPLETE => Ok(false),
        None => Err(invalid("platform erasure original credential is missing")),
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
            "platform erasure credential delete was not acknowledged",
        ));
    }
    Ok(())
}

fn finish_original(
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
    stop: StopAfter,
) -> Result<(), AuraError> {
    // Validate both survivors before mutating either. A missing survivor is
    // explained only by the authenticated erasure-pending root transition.
    let record_present = original_or_absent(selected, root, RECORD_USER)?;
    let intent_present = original_or_absent(selected, root, INTENT_USER)?;
    if root.phase == ERASURE_COMPLETE {
        if record_present || intent_present {
            return Err(invalid("completed platform erasure credential reappeared"));
        }
        return Ok(());
    }
    if record_present {
        delete_and_readback(selected, RECORD_USER)?;
    }
    if stop == StopAfter::RecordDelete {
        return Err(interrupted());
    }
    if intent_present {
        delete_and_readback(selected, INTENT_USER)?;
    }
    if stop == StopAfter::IntentDelete {
        return Err(interrupted());
    }
    root.phase = ERASURE_COMPLETE;
    write_keyring(selected, root)
}

pub(super) fn recover(
    _owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    root: &mut RootRecord,
) -> Result<(), AuraError> {
    if !matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE) {
        return Err(invalid("platform erasure lacks authenticated root intent"));
    }
    finish_original(selected, root, StopAfter::Never)
}

/// Transition from acknowledged negative denial to local credential erasure.
/// No caller supplies record addresses or replacement bytes.
#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn erase_original(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
) -> Result<(), AuraError> {
    erase_inner(owned, birth, StopAfter::Never).await
}

async fn erase_inner(
    owned: &ProfileOwnedSecureStorage,
    birth: &PlatformBirthAdmission,
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    if !Arc::ptr_eq(&birth._owner, &owned._owner) {
        return Err(invalid("platform erasure belongs to another profile owner"));
    }
    let root = read_root(owned, selected)?;
    if root.id != birth.root || root.birth_id != birth.allocation {
        return Err(invalid("platform erasure differs from original allocation"));
    }
    if verify_inventory(owned, &root)?.is_none() {
        return Err(invalid("platform erasure original inventory is missing"));
    }
    erase_locked(owned, selected, root, stop)
}

/// After append, only the recovered complete-roster owner may erase the first
/// allocation. The selected provider and namespace lock remain unchanged.
#[allow(dead_code)]
pub(in crate::secure::platform_lifetime_root::birth) async fn erase_roster(
    owned: &ProfileOwnedSecureStorage,
    roster: &append::PlatformRosterAdmission,
) -> Result<(), AuraError> {
    erase_roster_inner(
        owned,
        append::OriginalRosterOwner::Two(roster),
        StopAfter::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_three(
    owned: &ProfileOwnedSecureStorage,
    three: &append::third::PlatformThreeRosterAdmission,
) -> Result<(), AuraError> {
    erase_roster_inner(
        owned,
        append::OriginalRosterOwner::Three(three),
        StopAfter::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_four(
    owned: &ProfileOwnedSecureStorage,
    four: &append::third::fourth::PlatformFourRosterAdmission,
) -> Result<(), AuraError> {
    erase_roster_inner(
        owned,
        append::OriginalRosterOwner::Four(four),
        StopAfter::Never,
    )
    .await
}

pub(in crate::secure::platform_lifetime_root::birth) async fn erase_generic(
    owned: &ProfileOwnedSecureStorage,
    generic: &append::third::fourth::generic::PlatformGenericRosterAdmission,
) -> Result<(), AuraError> {
    erase_roster_inner(
        owned,
        append::OriginalRosterOwner::Generic(generic),
        StopAfter::Never,
    )
    .await
}

async fn erase_roster_inner(
    owned: &ProfileOwnedSecureStorage,
    roster: append::OriginalRosterOwner<'_>,
    stop: StopAfter,
) -> Result<(), AuraError> {
    let (selected, _) = selected_owner(owned)?;
    let _guard = selected.namespace_owner()?.mutation_guard().await;
    let (root, _) = append::verify_original_roster(owned, selected, roster)?;
    erase_locked(owned, selected, root, stop)
}

fn erase_locked(
    owned: &ProfileOwnedSecureStorage,
    selected: &PlatformSecureStorageHandler,
    mut root: RootRecord,
    stop: StopAfter,
) -> Result<(), AuraError> {
    super::super::validate_recovery(owned, selected, &root)?;
    if root.phase == TOMBSTONE_ACK_ERASURE_PENDING {
        // The original birth record must still be complete before an intent
        // authorizes deletion of either credential.
        let (bytes, _) = read_intent(selected, &root)?;
        if read_entry(selected, RECORD_USER)?.as_deref() != Some(bytes.as_slice()) {
            return Err(invalid(
                "platform erasure original record is missing or different",
            ));
        }
        root.phase = ERASURE_PENDING;
        write_keyring(selected, &root)?;
        if stop == StopAfter::RootIntent {
            return Err(interrupted());
        }
    } else if !matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE) {
        return Err(invalid(
            "platform erasure requires acknowledged negative denial",
        ));
    }
    finish_original(selected, &mut root, stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn first_roster_erasure_preserves_second_and_reopens() -> TestResult {
        let (fixture, selected, roster) = super::super::tests::denied_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        acknowledge_roster(owned, &roster, b"first refusal").await?;
        erase_roster(owned, &roster).await?;
        erase_roster(owned, &roster).await?;
        let platform = selected_owner(owned)?.0;
        let root = read_root(owned, platform)?;
        assert_eq!(root.phase, ERASURE_COMPLETE);
        assert_eq!(root.first_scope, b"first");
        assert_eq!(root.second_scope, b"second");
        assert!(read_entry(platform, INTENT_USER)?.is_none());
        assert!(read_entry(platform, RECORD_USER)?.is_none());
        assert!(read_roster_first_secret(owned, &roster).await.is_err());
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
        let root = read_root(owned, selected_owner(owned)?.0)?;
        assert_eq!(root.first_scope, b"first");
        assert_eq!(root.second_scope, b"second");
        assert!(read_roster_first_secret(owned, &roster).await.is_err());
        assert_eq!(
            &**append::read_roster_second_secret(owned, &roster).await?,
            b"second secret"
        );
        Ok(())
    }

    #[tokio::test]
    async fn erased_first_scope_refuses_missing_root_on_reopen() -> TestResult {
        let (fixture, selected, roster) = super::super::tests::denied_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        acknowledge_roster(owned, &roster, b"first refusal").await?;
        erase_roster(owned, &roster).await?;
        let platform = selected_owner(owned)?.0;
        platform.entry_for_user(KEYRING_USER)?.delete_credential()?;
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

    #[tokio::test]
    async fn first_roster_erasure_interruptions_recover_before_handoff() -> TestResult {
        for stop in [
            StopAfter::RootIntent,
            StopAfter::RecordDelete,
            StopAfter::IntentDelete,
        ] {
            let (fixture, selected, roster) = super::super::tests::denied_roster().await?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
                unreachable!()
            };
            acknowledge_roster(owned, &roster, b"first refusal").await?;
            assert!(
                erase_roster_inner(owned, append::OriginalRosterOwner::Two(&roster), stop)
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
            let platform = selected_owner(owned)?.0;
            assert_eq!(read_root(owned, platform)?.phase, ERASURE_COMPLETE);
            assert!(read_entry(platform, INTENT_USER)?.is_none());
            assert!(read_entry(platform, RECORD_USER)?.is_none());
            assert_eq!(
                &**append::read_roster_second_secret(owned, &roster).await?,
                b"second secret"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn first_roster_erasure_refuses_foreign_owner_and_lost_second_marker() -> TestResult {
        let (fixture, selected, roster) = super::super::tests::denied_roster().await?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        acknowledge_roster(owned, &roster, b"first refusal").await?;
        let foreign = tempfile::tempdir()?;
        let foreign_selected = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(erase_roster(foreign_owned, &roster).await.is_err());
        let platform = selected_owner(owned)?.0;
        let original_intent = read_entry(platform, INTENT_USER)?;
        let original_record = read_entry(platform, RECORD_USER)?;
        std::fs::remove_file(fixture.profile.path().join(append::SECOND_ROSTER_FILE))?;
        assert!(erase_roster(owned, &roster).await.is_err());
        assert_eq!(
            read_root(owned, platform)?.phase,
            TOMBSTONE_ACK_ERASURE_PENDING
        );
        assert_eq!(read_entry(platform, INTENT_USER)?, original_intent);
        assert_eq!(read_entry(platform, RECORD_USER)?, original_record);
        Ok(())
    }

    async fn denied_birth(
        owned: &ProfileOwnedSecureStorage,
    ) -> Result<PlatformBirthAdmission, AuraError> {
        let empty = admit_empty_root(owned).await?;
        let birth = allocate_one(owned, &empty, b"scope", b"secret").await?;
        decide(owned, &birth, false, b"refusal").await?;
        acknowledge_negative_denial(owned, &birth, b"refusal").await?;
        Ok(birth)
    }

    #[tokio::test]
    async fn selected_provider_erases_original_credentials_and_reopens() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let birth = denied_birth(owned).await?;
        erase_original(owned, &birth).await?;
        erase_original(owned, &birth).await?;
        let (selected, _) = selected_owner(owned)?;
        assert_eq!(read_root(owned, selected)?.phase, ERASURE_COMPLETE);
        assert!(read_entry(selected, INTENT_USER)?.is_none());
        assert!(read_entry(selected, RECORD_USER)?.is_none());
        assert!(matches!(
            read_live_secret(owned, &birth).await,
            Err(AuraError::Invalid { message, .. }) if message == "platform live read denied by original erasure"
        ));
        drop(birth);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let birth = recover_one(owned).await?;
        assert!(matches!(
            read_live_secret(owned, &birth).await,
            Err(AuraError::Invalid { message, .. }) if message == "platform live read denied by original erasure"
        ));
        let (selected, _) = selected_owner(owned)?;
        assert_eq!(read_root(owned, selected)?.phase, ERASURE_COMPLETE);
        assert!(read_entry(selected, INTENT_USER)?.is_none());
        assert!(read_entry(selected, RECORD_USER)?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn every_erasure_interruption_replays_original_only() -> TestResult {
        for stop in [
            StopAfter::RootIntent,
            StopAfter::RecordDelete,
            StopAfter::IntentDelete,
        ] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let birth = denied_birth(owned).await?;
            assert!(erase_inner(owned, &birth, stop).await.is_err());
            assert!(read_live_secret(owned, &birth).await.is_err());
            drop(birth);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            let birth = recover_one(owned).await?;
            let (selected, _) = selected_owner(owned)?;
            assert_eq!(read_root(owned, selected)?.phase, ERASURE_COMPLETE);
            assert!(read_entry(selected, INTENT_USER)?.is_none());
            assert!(read_entry(selected, RECORD_USER)?.is_none());
            assert!(read_live_secret(owned, &birth).await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn foreign_and_wrong_surviving_credentials_refuse_without_deletion() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let birth = denied_birth(owned).await?;
        let foreign = tempfile::tempdir()?;
        let foreign_selected = crate::secure::platform_lifetime_root::tests::selected(
            foreign.path(),
            &fixture.service,
        )?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        assert!(erase_original(foreign_owned, &birth).await.is_err());
        let (selected, _) = selected_owner(owned)?;
        write_exact(selected, RECORD_USER, b"wrong record")?;
        assert!(erase_original(owned, &birth).await.is_err());
        assert_eq!(
            read_root(owned, selected)?.phase,
            TOMBSTONE_ACK_ERASURE_PENDING
        );
        assert_eq!(
            read_entry(selected, RECORD_USER)?.as_deref(),
            Some(b"wrong record".as_slice())
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_tombstone_and_completed_reappearance_refuse() -> TestResult {
        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let birth = denied_birth(owned).await?;
        let (selected, _) = selected_owner(owned)?;
        selected
            .entry_for_user(TOMBSTONE_USER)?
            .delete_credential()?;
        assert!(erase_original(owned, &birth).await.is_err());
        assert_eq!(
            read_root(owned, selected)?.phase,
            TOMBSTONE_ACK_ERASURE_PENDING
        );
        drop(birth);
        drop(first);

        let fixture = Fixture::new()?;
        let first = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
            unreachable!()
        };
        let birth = denied_birth(owned).await?;
        erase_original(owned, &birth).await?;
        let (selected, _) = selected_owner(owned)?;
        write_exact(selected, RECORD_USER, b"reappeared")?;
        assert!(erase_original(owned, &birth).await.is_err());
        drop(birth);
        drop(first);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        assert!(recover_one(owned).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn pending_wrong_survivor_refuses_before_another_delete() -> TestResult {
        for stop in [StopAfter::RootIntent, StopAfter::RecordDelete] {
            let fixture = Fixture::new()?;
            let first = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &first else {
                unreachable!()
            };
            let birth = denied_birth(owned).await?;
            assert!(erase_inner(owned, &birth, stop).await.is_err());
            let (selected, _) = selected_owner(owned)?;
            let wrong_user = if stop == StopAfter::RootIntent {
                RECORD_USER
            } else {
                INTENT_USER
            };
            write_exact(selected, wrong_user, b"different surviving bytes")?;
            let other_user = if stop == StopAfter::RootIntent {
                INTENT_USER
            } else {
                RECORD_USER
            };
            let before = read_entry(selected, other_user)?;
            drop(birth);
            drop(first);
            let reopened = fixture.selected()?;
            let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
                unreachable!()
            };
            assert!(recover_one(owned).await.is_err());
            let (selected, _) = selected_owner(owned)?;
            assert_eq!(read_root(owned, selected)?.phase, ERASURE_PENDING);
            assert_eq!(read_entry(selected, other_user)?, before);
            assert_eq!(
                read_entry(selected, wrong_user)?.as_deref(),
                Some(b"different surviving bytes".as_slice())
            );
        }
        Ok(())
    }
}
