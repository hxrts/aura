use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use aura_agent::core::default_context_id_for_authority;
use aura_app::ui::types::{
    AccountBackup, AccountConfig, BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity,
    BootstrapSurface, PendingAccountBootstrap, PENDING_ACCOUNT_BOOTSTRAP_FILENAME,
};
use aura_app::ui::workflows::account::{
    derive_recovered_context_id, parse_backup_code, prepare_pending_account_bootstrap,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{StorageCoreEffects, StorageExtendedEffects};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use aura_effects::time::PhysicalTimeHandler;
use aura_effects::{
    identifiers::{new_authority_id, new_context_id},
    EncryptedStorage, EncryptedStorageConfig, FilesystemFallbackSecureStorageHandler,
    FilesystemStorageHandler, RealCryptoHandler,
};

use super::{AccountLoadResult, ACCOUNT_FILENAME, JOURNAL_FILENAME};

const SELECTED_RUNTIME_IDENTITY_FILENAME: &str = "selected-runtime-identity.json";
const PREPARED_DEVICE_ENROLLMENT_INVITEE_AUTHORITY_FILENAME: &str =
    ".harness-device-enrollment-invitee-authority";

pub(super) type BootstrapStorage = EncryptedStorage<
    FilesystemStorageHandler,
    RealCryptoHandler,
    FilesystemFallbackSecureStorageHandler,
>;

pub(super) fn open_bootstrap_storage(base_path: &Path) -> BootstrapStorage {
    let crypto = Arc::new(RealCryptoHandler::new());
    let secure = Arc::new(FilesystemFallbackSecureStorageHandler::with_base_path(
        base_path.to_path_buf(),
    ));
    EncryptedStorage::new(
        FilesystemStorageHandler::from_path(base_path.to_path_buf()),
        crypto,
        secure,
        EncryptedStorageConfig::default(),
    )
}

pub(super) async fn cleanup_demo_storage(storage: &impl StorageExtendedEffects, base_path: &Path) {
    match storage.clear_all().await {
        Ok(()) => tracing::info!(path = %base_path.display(), "Cleaned up demo storage"),
        Err(error) => tracing::warn!(
            path = %base_path.display(),
            err = %error,
            "Failed to clean up demo storage"
        ),
    }
}

pub(super) async fn try_load_account(
    storage: &impl StorageCoreEffects,
) -> Result<AccountLoadResult, AuraError> {
    let Some(bytes) = storage
        .retrieve(ACCOUNT_FILENAME)
        .await
        .map_err(|error| AuraError::internal(format!("Failed to read account config: {error}")))?
    else {
        return Ok(AccountLoadResult::NotFound);
    };

    let config: AccountConfig = serde_json::from_slice(&bytes)
        .map_err(|error| AuraError::internal(format!("Failed to parse account config: {error}")))?;

    Ok(AccountLoadResult::Loaded {
        authority: config.authority_id,
        context: config.context_id,
        nickname_suggestion: config.nickname_suggestion,
    })
}

pub(super) async fn wait_for_persisted_account(
    storage: &impl StorageCoreEffects,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<AccountLoadResult, AuraError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let loaded = try_load_account(storage).await?;
        if matches!(loaded, AccountLoadResult::Loaded { .. }) {
            return Ok(loaded);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(loaded);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Load persisted account state for a terminal storage root.
pub async fn try_load_account_from_path(base_path: &Path) -> Result<AccountLoadResult, AuraError> {
    let storage = open_bootstrap_storage(base_path);
    try_load_account(&storage).await
}

async fn persist_account_config(
    storage: &impl StorageCoreEffects,
    time: &impl PhysicalTimeEffects,
    authority_id: AuthorityId,
    context_id: ContextId,
    nickname_suggestion: Option<String>,
) -> Result<(), AuraError> {
    let created_at = time
        .physical_time()
        .await
        .map_err(|error| AuraError::internal(format!("Failed to fetch physical time: {error}")))?
        .ts_ms;

    let config = AccountConfig {
        authority_id,
        context_id,
        nickname_suggestion,
        created_at,
    };

    let content = serde_json::to_vec_pretty(&config).map_err(|error| {
        AuraError::internal(format!("Failed to serialize account config: {error}"))
    })?;

    storage
        .store(ACCOUNT_FILENAME, content)
        .await
        .map_err(|error| AuraError::internal(format!("Failed to write account config: {error}")))?;

    Ok(())
}

pub(super) async fn load_pending_account_bootstrap(
    storage: &impl StorageCoreEffects,
) -> Result<Option<PendingAccountBootstrap>, AuraError> {
    let Some(bytes) = storage
        .retrieve(PENDING_ACCOUNT_BOOTSTRAP_FILENAME)
        .await
        .map_err(|error| {
            AuraError::internal(format!("Failed to read pending account bootstrap: {error}"))
        })?
    else {
        return Ok(None);
    };

    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        AuraError::internal(format!("Invalid pending account bootstrap data: {error}"))
    })
}

async fn persist_pending_account_bootstrap(
    storage: &impl StorageCoreEffects,
    pending_bootstrap: &PendingAccountBootstrap,
) -> Result<(), AuraError> {
    let bytes = serde_json::to_vec(pending_bootstrap).map_err(|error| {
        AuraError::internal(format!(
            "Failed to serialize pending account bootstrap: {error}"
        ))
    })?;
    storage
        .store(PENDING_ACCOUNT_BOOTSTRAP_FILENAME, bytes)
        .await
        .map_err(|error| {
            AuraError::internal(format!(
                "Failed to persist pending account bootstrap: {error}"
            ))
        })
}

pub(super) async fn clear_pending_account_bootstrap(
    storage: &impl StorageExtendedEffects,
) -> Result<(), AuraError> {
    storage
        .remove(PENDING_ACCOUNT_BOOTSTRAP_FILENAME)
        .await
        .map_err(|error| {
            AuraError::internal(format!(
                "Failed to clear pending account bootstrap: {error}"
            ))
        })
        .map(|_| ())
}

pub(super) async fn load_selected_runtime_identity(
    storage: &impl StorageCoreEffects,
) -> Result<Option<BootstrapRuntimeIdentity>, AuraError> {
    let Some(bytes) = storage
        .retrieve(SELECTED_RUNTIME_IDENTITY_FILENAME)
        .await
        .map_err(|error| {
            AuraError::internal(format!("Failed to read selected runtime identity: {error}"))
        })?
    else {
        return Ok(None);
    };

    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        AuraError::internal(format!("Invalid selected runtime identity data: {error}"))
    })
}

pub(super) fn load_prepared_device_enrollment_invitee_authority(
    base_path: &Path,
) -> Result<Option<AuthorityId>, AuraError> {
    let path = base_path.join(PREPARED_DEVICE_ENROLLMENT_INVITEE_AUTHORITY_FILENAME);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AuraError::internal(format!(
                "Failed to read prepared device enrollment invitee authority: {error}"
            )));
        }
    };
    let authority = raw.trim();
    if authority.is_empty() {
        return Ok(None);
    }
    authority.parse::<AuthorityId>().map(Some).map_err(|error| {
        AuraError::internal(format!(
            "Invalid prepared device enrollment invitee authority: {error}"
        ))
    })
}

async fn persist_selected_runtime_identity(
    storage: &impl StorageCoreEffects,
    runtime_identity: &BootstrapRuntimeIdentity,
) -> Result<(), AuraError> {
    let bytes = serde_json::to_vec(runtime_identity).map_err(|error| {
        AuraError::internal(format!(
            "Failed to serialize selected runtime identity: {error}"
        ))
    })?;
    storage
        .store(SELECTED_RUNTIME_IDENTITY_FILENAME, bytes)
        .await
        .map_err(|error| {
            AuraError::internal(format!(
                "Failed to persist selected runtime identity: {error}"
            ))
        })
}

pub(super) async fn persist_selected_authority(
    base_path: &Path,
    authority_id: AuthorityId,
    nickname_suggestion: Option<String>,
) -> Result<ContextId, AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();
    let context_id = default_context_id_for_authority(authority_id);

    persist_account_config(
        &storage,
        &time,
        authority_id,
        context_id,
        nickname_suggestion,
    )
    .await?;

    Ok(context_id)
}

/// Create a new account and save to disk.
pub async fn create_account(
    base_path: &Path,
    nickname_suggestion: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let pending_bootstrap = prepare_pending_account_bootstrap(nickname_suggestion)?;
    create_account_with_pending_bootstrap(base_path, pending_bootstrap, None).await
}

/// Create a new account and stage a device-enrollment import for the first runtime start.
pub async fn create_account_with_device_enrollment(
    base_path: &Path,
    nickname_suggestion: &str,
    device_enrollment_code: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let pending_bootstrap = prepare_pending_account_bootstrap(nickname_suggestion)?
        .with_device_enrollment_code(device_enrollment_code.to_string());
    create_account_with_pending_bootstrap(base_path, pending_bootstrap, None).await
}

pub async fn create_account_with_device_enrollment_runtime_identity(
    base_path: &Path,
    runtime_identity: BootstrapRuntimeIdentity,
    nickname_suggestion: &str,
    device_enrollment_code: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let pending_bootstrap = prepare_pending_account_bootstrap(nickname_suggestion)?
        .with_device_enrollment_code(device_enrollment_code.to_string());
    create_account_with_pending_bootstrap(base_path, pending_bootstrap, Some(runtime_identity))
        .await
}

async fn create_account_with_pending_bootstrap(
    base_path: &Path,
    pending_bootstrap: PendingAccountBootstrap,
    runtime_identity: Option<BootstrapRuntimeIdentity>,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let staged_event = BootstrapEvent::new(
        BootstrapSurface::Tui,
        BootstrapEventKind::PendingBootstrapStaged,
    );
    tracing::info!(event = %staged_event, path = %base_path.display());
    tracing::info!(
        path = %base_path.display(),
        nickname = pending_bootstrap.nickname_suggestion,
        pending_device_enrollment = pending_bootstrap.has_pending_device_enrollment(),
        "tui create_account begin"
    );
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();
    let crypto = RealCryptoHandler::new();

    let (authority_id, context_id) = if let Some(identity) = runtime_identity.clone() {
        (
            identity.authority_id,
            default_context_id_for_authority(identity.authority_id),
        )
    } else {
        let authority_id = new_authority_id(&crypto).await;
        let context_id = new_context_id(&crypto).await;
        (authority_id, context_id)
    };

    tracing::info!("tui create_account persisting account config");
    persist_pending_account_bootstrap(&storage, &pending_bootstrap).await?;
    if let Some(identity) = runtime_identity.as_ref() {
        persist_selected_runtime_identity(&storage, identity).await?;
    }
    persist_account_config(
        &storage,
        &time,
        authority_id,
        context_id,
        Some(pending_bootstrap.nickname_suggestion.clone()),
    )
    .await?;
    tracing::info!("tui create_account persisted account config");

    Ok((authority_id, context_id))
}

/// Restore an account from guardian-based recovery.
pub async fn restore_recovered_account(
    base_path: &Path,
    recovered_authority_id: AuthorityId,
    recovered_context_id: Option<ContextId>,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();
    let context_id = recovered_context_id
        .unwrap_or_else(|| derive_recovered_context_id(&recovered_authority_id));

    persist_account_config(&storage, &time, recovered_authority_id, context_id, None).await?;

    Ok((recovered_authority_id, context_id))
}

/// Export account to a portable backup code.
pub async fn export_account_backup(
    base_path: &Path,
    device_id: Option<&str>,
) -> Result<String, AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();

    let Some(account_bytes) = storage
        .retrieve(ACCOUNT_FILENAME)
        .await
        .map_err(|error| AuraError::internal(format!("Failed to read account config: {error}")))?
    else {
        return Err(AuraError::internal("No account exists to backup"));
    };

    let account: AccountConfig = serde_json::from_slice(&account_bytes)
        .map_err(|error| AuraError::internal(format!("Failed to parse account config: {error}")))?;

    let journal = storage
        .retrieve(JOURNAL_FILENAME)
        .await
        .map_err(|error| AuraError::internal(format!("Failed to read journal: {error}")))?
        .and_then(|bytes| String::from_utf8(bytes).ok());

    let backup_at = time
        .physical_time()
        .await
        .map_err(|error| AuraError::internal(format!("Failed to fetch physical time: {error}")))?
        .ts_ms;

    let backup = AccountBackup::new(account, journal, backup_at, device_id.map(String::from));

    backup
        .encode()
        .map_err(|error| AuraError::internal(format!("Failed to encode backup: {error}")))
}

/// Import and restore account from backup code.
pub async fn import_account_backup(
    base_path: &Path,
    backup_code: &str,
    overwrite: bool,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let backup = parse_backup_code(backup_code)?;

    let authority_id = backup.account.authority_id;
    let context_id = backup.account.context_id;

    if storage.exists(ACCOUNT_FILENAME).await.map_err(|error| {
        AuraError::internal(format!("Failed to check account existence: {error}"))
    })? && !overwrite
    {
        return Err(AuraError::internal(
            "Account already exists. Use overwrite=true to replace.",
        ));
    }

    let account_content = serde_json::to_vec_pretty(&backup.account).map_err(|error| {
        AuraError::internal(format!("Failed to serialize account config: {error}"))
    })?;

    storage
        .store(ACCOUNT_FILENAME, account_content)
        .await
        .map_err(|error| AuraError::internal(format!("Failed to write account config: {error}")))?;

    if let Some(journal_content) = &backup.journal {
        storage
            .store(JOURNAL_FILENAME, journal_content.as_bytes().to_vec())
            .await
            .map_err(|error| AuraError::internal(format!("Failed to write journal: {error}")))?;
    }

    Ok((authority_id, context_id))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn create_account_persists_pending_bootstrap_and_account() {
        let temp_dir = tempdir().expect("create temp dir");

        let (authority_id, context_id) = create_account(temp_dir.path(), "Alice")
            .await
            .expect("create account");

        let storage = open_bootstrap_storage(temp_dir.path());
        let pending = load_pending_account_bootstrap(&storage)
            .await
            .expect("read pending bootstrap")
            .expect("pending bootstrap should exist");
        assert_eq!(pending.nickname_suggestion, "Alice");

        let loaded = try_load_account(&storage).await.expect("load account");
        match loaded {
            AccountLoadResult::Loaded {
                authority,
                context,
                nickname_suggestion,
            } => {
                assert_eq!(authority, authority_id);
                assert_eq!(context, context_id);
                assert_eq!(nickname_suggestion.as_deref(), Some("Alice"));
            }
            AccountLoadResult::NotFound => panic!("account should have been persisted"),
        }
    }

    #[tokio::test]
    async fn try_load_account_from_path_loads_persisted_identity() {
        let temp_dir = tempdir().expect("create temp dir");

        let (authority_id, context_id) = create_account(temp_dir.path(), "Alice")
            .await
            .expect("create account");

        let loaded = try_load_account_from_path(temp_dir.path())
            .await
            .expect("load persisted account");

        match loaded {
            AccountLoadResult::Loaded {
                authority,
                context,
                nickname_suggestion,
            } => {
                assert_eq!(authority, authority_id);
                assert_eq!(context, context_id);
                assert_eq!(nickname_suggestion.as_deref(), Some("Alice"));
            }
            AccountLoadResult::NotFound => panic!("persisted account should be loaded"),
        }
    }

    #[tokio::test]
    async fn try_load_account_from_path_reports_missing_account() {
        let temp_dir = tempdir().expect("create temp dir");

        let loaded = try_load_account_from_path(temp_dir.path())
            .await
            .expect("load account");
        assert!(matches!(loaded, AccountLoadResult::NotFound));
    }
}
