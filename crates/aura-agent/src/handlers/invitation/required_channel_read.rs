//! Required persisted channel-invitation reads for canonical membership queries.
//! Unlike observed invitation listing, failures cannot imply an empty roster.
use super::*;
use aura_core::AuraError;

#[derive(Debug, thiserror::Error)]
enum RequiredInvitationIdentityError {
    #[error("retained sender enrollment payload differs from its regular record")]
    EnrollmentPayloadBinding,
    #[error("retained sender enrollment exceeds the 4 MiB secret record bound")]
    OversizedSecret,
    #[error("persisted sender invitation exceeds the 1 MiB record bound")]
    Oversized,
    #[error("persisted invitation key does not match its authority and invitation identifier")]
    StorageKey,
    #[error("created invitation sender differs from its storage authority")]
    CreatedSender,
    #[error("cached and persisted channel invitation identities disagree")]
    CachedIdentity,
}

fn identity_failure(cause: RequiredInvitationIdentityError) -> AuraError {
    AuraError::Invalid {
        message: cause.to_string(),
        source: Some(Arc::new(cause)),
    }
}

fn validate_created_identity(
    own_id: AuthorityId,
    key: &str,
    invitation: &Invitation,
) -> Result<(), AuraError> {
    if key != InvitationCacheHandler::created_invitation_key(own_id, &invitation.invitation_id) {
        return Err(identity_failure(
            RequiredInvitationIdentityError::StorageKey,
        ));
    }
    if invitation.sender_id != own_id {
        return Err(identity_failure(
            RequiredInvitationIdentityError::CreatedSender,
        ));
    }
    Ok(())
}

fn validate_cached_identity(
    existing: &Invitation,
    persisted: &Invitation,
) -> Result<(), AuraError> {
    let channel_id = |invitation: &Invitation| match invitation.invitation_type {
        InvitationType::Channel { home_id, .. } => Some(home_id),
        _ => None,
    };
    if existing.invitation_id != persisted.invitation_id
        || existing.context_id != persisted.context_id
        || existing.sender_id != persisted.sender_id
        || existing.receiver_id != persisted.receiver_id
        || channel_id(existing) != channel_id(persisted)
    {
        return Err(identity_failure(
            RequiredInvitationIdentityError::CachedIdentity,
        ));
    }
    Ok(())
}

/// Required sender-owned record; observed cache and best-effort loaders are not
/// evidence of presence, absence, or authorization for cancellation.
pub(super) async fn created_required<E: StorageCoreEffects + ?Sized>(
    effects: &E,
    own_id: AuthorityId,
    invitation_id: &InvitationId,
) -> Result<Invitation, AuraError> {
    let key = InvitationCacheHandler::created_invitation_key(own_id, invitation_id);
    let bytes = effects
        .retrieve(&key)
        .await
        .map_err(|source| AuraError::Storage {
            message: "read required sender invitation".into(),
            source: Some(Arc::new(source)),
        })?
        .ok_or_else(|| AuraError::not_found("sender invitation"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(identity_failure(RequiredInvitationIdentityError::Oversized));
    }
    let invitation: Invitation =
        serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
            message: "decode required sender invitation".into(),
            source: Some(Arc::new(source)),
        })?;
    validate_created_identity(own_id, &key, &invitation)?;
    Ok(invitation)
}

/// Restore only the separately retained secret payload of this exact required
/// sender record. Missing, inaccessible and corrupt secure records are required
/// failures; none may return the redacted value as usable enrollment authority.
pub(super) async fn hydrate_created_enrollment_required<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    own_id: AuthorityId,
    regular: Invitation,
) -> Result<Invitation, AuraError> {
    if !matches!(
        regular.invitation_type,
        InvitationType::DeviceEnrollment { .. }
    ) {
        return Ok(regular);
    }
    let location =
        InvitationCacheHandler::secret_payload_location(own_id, &regular.invitation_id, "created");
    let bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(identity_failure(
            RequiredInvitationIdentityError::OversizedSecret,
        ));
    }
    let mut retained: Invitation =
        serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
            message: "decode required retained sender enrollment".into(),
            source: Some(Arc::new(source)),
        })?;
    validate_created_identity(
        own_id,
        &InvitationCacheHandler::created_invitation_key(own_id, &regular.invitation_id),
        &retained,
    )?;
    let redacted = InvitationCacheHandler::redact_device_enrollment_payload(&retained);
    if redacted.invitation_id != regular.invitation_id
        || redacted.context_id != regular.context_id
        || redacted.sender_id != regular.sender_id
        || redacted.receiver_id != regular.receiver_id
        || redacted.invitation_type != regular.invitation_type
        || redacted.created_at != regular.created_at
        || redacted.expires_at != regular.expires_at
        || redacted.message != regular.message
        || redacted.receiver_nickname != regular.receiver_nickname
    {
        return Err(identity_failure(
            RequiredInvitationIdentityError::EnrollmentPayloadBinding,
        ));
    }
    // Lifecycle status is mutable in the regular record; it never rewrites the
    // retained original cryptographic payload or supplies cryptographic evidence.
    retained.status = regular.status;
    Ok(retained)
}

pub(super) async fn list_required<E>(
    effects: &E,
    own_id: AuthorityId,
    cached: Vec<Invitation>,
) -> Result<Vec<Invitation>, AuraError>
where
    E: StorageCoreEffects + PhysicalTimeEffects,
{
    let mut invitations = HashMap::new();
    for invitation in cached {
        if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
            InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
        }
    }
    for (prefix, imported) in [
        (
            InvitationCacheHandler::created_invitation_prefix(own_id),
            false,
        ),
        (
            InvitationCacheHandler::imported_invitation_prefix(own_id),
            true,
        ),
    ] {
        let keys = effects
            .list_keys(Some(&prefix))
            .await
            .map_err(|error| AuraError::Storage {
                message: format!("list required channel invitation records under {prefix}"),
                source: Some(Arc::new(error)),
            })?;
        for key in keys {
            let bytes = effects
                .retrieve(&key)
                .await
                .map_err(|error| AuraError::Storage {
                    message: format!("read required invitation record {key}"),
                    source: Some(Arc::new(error)),
                })?
                .ok_or_else(|| {
                    AuraError::not_found(format!("listed invitation record disappeared: {key}"))
                })?;
            // Decode before filtering: corrupt records have unknown type and
            // cannot silently become an absent channel invitation.
            let invitation = if imported {
                let stored: StoredImportedInvitation =
                    serde_json::from_slice(&bytes).map_err(|error| AuraError::Serialization {
                        message: format!("decode required imported invitation record {key}"),
                        source: Some(Arc::new(error)),
                    })?;
                if key
                    != InvitationCacheHandler::imported_invitation_key(
                        own_id,
                        &stored.shareable.invitation_id,
                    )
                {
                    return Err(identity_failure(
                        RequiredInvitationIdentityError::StorageKey,
                    ));
                }
                if !matches!(
                    stored.shareable.invitation_type,
                    InvitationType::Channel { .. }
                ) {
                    continue;
                }
                let context_id = require_channel_invitation_context(
                    &stored.invitation_id,
                    stored.sender_id,
                    stored.context_id,
                )
                .map_err(|error| AuraError::Invalid {
                    message: format!("required imported channel invitation lacks context: {key}"),
                    source: Some(Arc::new(error)),
                })?;
                let created_at = if stored.created_at == 0 {
                    effects
                        .physical_time()
                        .await
                        .map_err(|error| AuraError::Internal {
                            message: "required invitation creation timestamp unavailable"
                                .to_string(),
                            source: Some(Arc::new(error)),
                        })?
                        .ts_ms
                } else {
                    stored.created_at
                };
                let status = stored.status;
                let shareable = stored.shareable;
                Invitation {
                    invitation_id: shareable.invitation_id,
                    context_id,
                    sender_id: shareable.sender_id,
                    receiver_id: imported_invitation_receiver(&shareable.invitation_type, own_id),
                    invitation_type: shareable.invitation_type,
                    status,
                    created_at,
                    expires_at: shareable.expires_at,
                    message: shareable.message,
                    receiver_nickname: None,
                }
            } else {
                let invitation: Invitation =
                    serde_json::from_slice(&bytes).map_err(|error| AuraError::Serialization {
                        message: format!("decode required created invitation record {key}"),
                        source: Some(Arc::new(error)),
                    })?;
                validate_created_identity(own_id, &key, &invitation)?;
                if !matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                    continue;
                }
                invitation
            };
            if let Some(existing) = invitations.get(&invitation.invitation_id) {
                validate_cached_identity(existing, &invitation)?;
            }
            InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
        }
    }
    Ok(invitations.into_values().collect())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use aura_core::effects::storage::StorageError;
    use aura_core::effects::time::TimeError;
    use std::error::Error as _;

    enum Fault {
        List,
        Retrieve,
        Missing,
        Codec,
        Empty,
    }
    struct FaultStorage(Fault);
    #[async_trait::async_trait]
    impl StorageCoreEffects for FaultStorage {
        async fn store(&self, _: &str, _: Vec<u8>) -> Result<(), StorageError> {
            unreachable!("reader must not write")
        }
        async fn remove(&self, _: &str) -> Result<bool, StorageError> {
            unreachable!("reader must not remove")
        }
        async fn retrieve(&self, _: &str) -> Result<Option<Vec<u8>>, StorageError> {
            match self.0 {
                Fault::Retrieve => Err(StorageError::ReadFailed(
                    "actual required record failure".to_string(),
                )),
                Fault::Missing => Ok(None),
                Fault::Codec => Ok(Some(b"{broken record".to_vec())),
                _ => unreachable!(),
            }
        }
        async fn list_keys(&self, _: Option<&str>) -> Result<Vec<String>, StorageError> {
            match self.0 {
                Fault::List => Err(StorageError::ReadFailed(
                    "actual required list failure".to_string(),
                )),
                Fault::Empty => Ok(vec![]),
                _ => Ok(vec!["listed-record".to_string()]),
            }
        }
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for FaultStorage {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            Err(TimeError::ServiceUnavailable)
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), TimeError> {
            unreachable!("reader must not sleep")
        }
    }

    #[tokio::test]
    async fn required_sender_read_retains_storage_codec_and_absence_without_mutation() {
        let own_id = AuthorityId::new_from_entropy([0x79; 32]);
        let id = InvitationId::new("required-sender-read");
        let storage = created_required(&FaultStorage(Fault::Retrieve), own_id, &id)
            .await
            .expect_err("required read must retain actual storage failure");
        assert!(matches!(
            storage
                .source()
                .and_then(|source| source.downcast_ref::<StorageError>()),
            Some(StorageError::ReadFailed(_))
        ));
        let codec = created_required(&FaultStorage(Fault::Codec), own_id, &id)
            .await
            .expect_err("malformed sender bytes cannot become absence");
        assert!(codec
            .source()
            .expect("actual codec source")
            .is::<serde_json::Error>());
        let absent = created_required(&FaultStorage(Fault::Missing), own_id, &id)
            .await
            .expect_err("missing sender record is typed absence");
        assert!(matches!(absent, AuraError::NotFound { .. }));
        // FaultStorage store/remove panic: all three required failures are read-only.
    }

    #[tokio::test]
    async fn required_channel_listing_preserves_list_and_record_storage_causes() {
        for fault in [Fault::List, Fault::Retrieve] {
            let error = list_required(
                &FaultStorage(fault),
                AuthorityId::new_from_entropy([0x61; 32]),
                vec![],
            )
            .await
            .expect_err("required read or identity validation must fail");
            assert!(matches!(error, AuraError::Storage { .. }));
            assert!(matches!(
                error
                    .source()
                    .expect("valid required-read fixture must succeed")
                    .downcast_ref::<StorageError>(),
                Some(StorageError::ReadFailed(_))
            ));
        }
    }
    #[tokio::test]
    async fn corrupt_unknown_type_and_disappeared_record_cannot_be_empty_roster() {
        let own_id = AuthorityId::new_from_entropy([0x62; 32]);
        let codec = list_required(&FaultStorage(Fault::Codec), own_id, vec![])
            .await
            .expect_err("required read or identity validation must fail");
        assert!(codec
            .source()
            .expect("valid required-read fixture must succeed")
            .is::<serde_json::Error>());
        let missing = list_required(&FaultStorage(Fault::Missing), own_id, vec![])
            .await
            .expect_err("required read or identity validation must fail");
        assert!(matches!(missing, AuraError::NotFound { .. }));
        // Empty stores need no fabricated timestamp or unavailable time service.
        assert!(list_required(&FaultStorage(Fault::Empty), own_id, vec![])
            .await
            .expect("valid required-read fixture must succeed")
            .is_empty());
    }
    #[tokio::test]
    async fn required_sender_record_size_is_checked_before_json_decode() {
        let source = RecordStorage {
            bytes: vec![b' '; 1024 * 1024 + 1],
            fail_clock: false,
        };
        let error = created_required(
            &source,
            AuthorityId::new_from_entropy([0x7a; 32]),
            &InvitationId::new("oversized-sender"),
        )
        .await
        .expect_err("bounded reader rejects oversized record");
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<RequiredInvitationIdentityError>()),
            Some(RequiredInvitationIdentityError::Oversized)
        ));
        assert!(
            !error
                .source()
                .expect("size failure source")
                .is::<serde_json::Error>(),
            "oversized data must not reach the JSON decoder"
        );
    }

    struct RecordStorage {
        bytes: Vec<u8>,
        fail_clock: bool,
    }
    #[async_trait::async_trait]
    impl StorageCoreEffects for RecordStorage {
        async fn store(&self, _: &str, _: Vec<u8>) -> Result<(), StorageError> {
            unreachable!()
        }
        async fn remove(&self, _: &str) -> Result<bool, StorageError> {
            unreachable!()
        }
        async fn retrieve(&self, _: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(Some(self.bytes.clone()))
        }
        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            if prefix.is_some_and(|prefix| prefix.contains("imported")) {
                Ok(vec![format!(
                    "{}required-channel-record",
                    prefix.expect("matched imported prefix")
                )])
            } else {
                Ok(vec![])
            }
        }
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for RecordStorage {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            if self.fail_clock {
                Err(TimeError::ServiceUnavailable)
            } else {
                Ok(PhysicalTime::exact(55))
            }
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), TimeError> {
            unreachable!()
        }
    }
    fn channel_record(context: Option<ContextId>) -> StoredImportedInvitation {
        let shareable = ShareableInvitation {
            version: 1,
            invitation_id: InvitationId::new("required-channel-record"),
            sender_id: AuthorityId::new_from_entropy([0x63; 32]),
            context_id: context,
            invitation_type: InvitationType::Channel {
                home_id: ChannelId::from_bytes([0x64; 32]),
                nickname_suggestion: None,
                bootstrap: None,
                home: false,
            },
            expires_at: None,
            message: None,
        };
        let mut stored =
            StoredImportedInvitation::pending(shareable, 0, ImportedSenderTrust::SelfCertified);
        stored.status = InvitationStatus::Accepted;
        stored
    }
    #[tokio::test]
    async fn required_channel_context_and_clock_failures_retain_sources() {
        let own_id = AuthorityId::new_from_entropy([0x65; 32]);
        let context = ContextId::new_from_entropy([0x66; 32]);
        let missing_context = RecordStorage {
            bytes: serde_json::to_vec(&channel_record(None))
                .expect("valid required-read fixture must succeed"),
            fail_clock: false,
        };
        let error = list_required(&missing_context, own_id, vec![])
            .await
            .expect_err("required read or identity validation must fail");
        assert!(matches!(error, AuraError::Invalid { .. }));
        assert!(error
            .source()
            .expect("valid required-read fixture must succeed")
            .is::<AgentError>());
        let unavailable_clock = RecordStorage {
            bytes: serde_json::to_vec(&channel_record(Some(context)))
                .expect("valid required-read fixture must succeed"),
            fail_clock: true,
        };
        let error = list_required(&unavailable_clock, own_id, vec![])
            .await
            .expect_err("required read or identity validation must fail");
        assert!(matches!(
            error
                .source()
                .expect("valid required-read fixture must succeed")
                .downcast_ref::<TimeError>(),
            Some(TimeError::ServiceUnavailable)
        ));
        let valid = RecordStorage {
            fail_clock: false,
            ..unavailable_clock
        };
        let invitations = list_required(&valid, own_id, vec![])
            .await
            .expect("valid required-read fixture must succeed");
        assert_eq!(invitations.len(), 1);
        assert_eq!(invitations[0].context_id, context);
        assert_eq!(invitations[0].created_at, 55);
        assert_eq!(invitations[0].status, InvitationStatus::Accepted);
        let failed = list_required(&FaultStorage(Fault::List), own_id, invitations)
            .await
            .expect_err("required read or identity validation must fail");
        assert!(
            failed
                .source()
                .expect("valid required-read fixture must succeed")
                .is::<StorageError>(),
            "cached accepted membership cannot hide required store failure"
        );
    }
    #[tokio::test]
    async fn substituted_record_identifier_fails_before_context_or_timestamp() {
        let own_id = AuthorityId::new_from_entropy([0x65; 32]);
        let mut record = channel_record(None);
        record.shareable.invitation_id = InvitationId::new("substituted-record");
        let storage = RecordStorage {
            bytes: serde_json::to_vec(&record).expect("valid required-read fixture must succeed"),
            fail_clock: true,
        };
        let error = list_required(&storage, own_id, vec![])
            .await
            .expect_err("required read or identity validation must fail");
        assert!(matches!(
            error
                .source()
                .expect("identity cause")
                .downcast_ref::<RequiredInvitationIdentityError>(),
            Some(RequiredInvitationIdentityError::StorageKey)
        ));
    }

    #[test]
    fn creator_and_cached_identity_cannot_be_reassigned_by_persisted_records() {
        let own_id = AuthorityId::new_from_entropy([0x65; 32]);
        let context = ContextId::new_from_entropy([0x66; 32]);
        let record = channel_record(Some(context));
        let mut invitation = Invitation {
            invitation_id: record.invitation_id.clone(),
            context_id: context,
            sender_id: record.sender_id,
            receiver_id: own_id,
            invitation_type: record.shareable.invitation_type,
            status: InvitationStatus::Accepted,
            created_at: 1,
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };
        let key = InvitationCacheHandler::created_invitation_key(own_id, &invitation.invitation_id);
        let error = validate_created_identity(own_id, &key, &invitation)
            .expect_err("required read or identity validation must fail");
        assert!(matches!(
            error
                .source()
                .expect("creator cause")
                .downcast_ref::<RequiredInvitationIdentityError>(),
            Some(RequiredInvitationIdentityError::CreatedSender)
        ));
        invitation.sender_id = own_id;
        validate_created_identity(own_id, &key, &invitation)
            .expect("valid required-read fixture must succeed");
        let mut altered = invitation.clone();
        altered.context_id = ContextId::new_from_entropy([0x67; 32]);
        assert!(validate_cached_identity(&invitation, &altered).is_err());
        let mut altered = invitation.clone();
        altered.sender_id = AuthorityId::new_from_entropy([0x68; 32]);
        assert!(validate_cached_identity(&invitation, &altered).is_err());
        altered = invitation.clone();
        altered.receiver_id = AuthorityId::new_from_entropy([0x69; 32]);
        assert!(validate_cached_identity(&invitation, &altered).is_err());
        altered = invitation.clone();
        altered.invitation_type = InvitationType::Contact { nickname: None };
        assert!(validate_cached_identity(&invitation, &altered).is_err());
        altered = invitation.clone();
        altered.status = InvitationStatus::Declined;
        validate_cached_identity(&invitation, &altered)
            .expect("valid required-read fixture must succeed");
    }
}
