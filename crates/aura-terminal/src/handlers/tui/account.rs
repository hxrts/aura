use std::path::{Path, PathBuf};
use std::sync::Arc;

use aura_agent::core::default_context_id_for_authority;
use aura_agent::{AgentConfig, AuraEffectSystem};
use aura_app::ui::types::{
    AccountBackup, AccountConfig, BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity,
    BootstrapSurface, PendingAccountBootstrap, PENDING_ACCOUNT_BOOTSTRAP_FILENAME,
};
use aura_app::ui::workflows::account::{
    derive_recovered_context_id, parse_backup_code, prepare_pending_account_bootstrap,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{StorageCoreEffects, StorageEffects, StorageExtendedEffects};
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

/// One profile's account records: account configuration, staged account
/// bootstrap and selected runtime identity.
///
/// A production profile holds the profile's exclusive owner and stores its
/// records exactly as the production runtime stores the profile (same owned
/// files, same selected secure-storage provider, same encryption policy), so
/// the encryption master key is created where the runtime reads it. Hand
/// [`ProfileStore::owner`] to production assembly so both share one owned
/// profile. Demo profiles keep the simulation runtime's nonproduction store.
#[derive(Clone)]
pub struct ProfileStore {
    base_path: PathBuf,
    storage: Arc<dyn StorageEffects>,
    owner: Option<Arc<aura_effects::profile_storage::OwnedProfileLease>>,
}

impl ProfileStore {
    /// Acquire the production profile at `base_path`. Fails while another
    /// process (a running TUI or `aura serve`) owns it.
    pub fn production(base_path: &Path) -> Result<Self, AuraError> {
        let mut config = AgentConfig::default();
        config.storage.base_path = base_path.to_path_buf();
        if let Some(backend) = crate::env::secure_storage_backend_override()? {
            config.storage.secure_storage_backend = backend;
        }
        let (owner, storage) =
            AuraEffectSystem::acquire_production_profile(&config).map_err(|error| {
                AuraError::Internal {
                    message: format!("open profile {}", base_path.display()),
                    source: Some(Arc::new(error)),
                }
            })?;
        Ok(Self {
            base_path: base_path.to_path_buf(),
            storage: Arc::new(storage),
            owner: Some(owner),
        })
    }

    /// The nonproduction (demo/simulation) store at `base_path`.
    #[must_use]
    pub fn nonproduction(base_path: &Path) -> Self {
        let secure = Arc::new(FilesystemFallbackSecureStorageHandler::with_base_path(
            base_path.to_path_buf(),
        ));
        let storage = EncryptedStorage::new(
            FilesystemStorageHandler::from_path(base_path.to_path_buf()),
            Arc::new(RealCryptoHandler::new()),
            secure,
            EncryptedStorageConfig::default(),
        );
        Self {
            base_path: base_path.to_path_buf(),
            storage: Arc::new(storage),
            owner: None,
        }
    }

    /// The store for a TUI mode.
    pub fn for_mode(base_path: &Path, mode: super::TuiMode) -> Result<Self, AuraError> {
        if mode.is_demo() {
            Ok(Self::nonproduction(base_path))
        } else {
            Self::production(base_path)
        }
    }

    /// The records' storage.
    #[must_use]
    pub fn storage(&self) -> &Arc<dyn StorageEffects> {
        &self.storage
    }

    /// The profile's exclusive owner (production profiles only).
    #[must_use]
    pub fn owner(&self) -> Option<Arc<aura_effects::profile_storage::OwnedProfileLease>> {
        self.owner.clone()
    }

    /// The profile directory.
    #[must_use]
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }
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
    store: &ProfileStore,
    authority_id: AuthorityId,
    nickname_suggestion: Option<String>,
) -> Result<ContextId, AuraError> {
    let storage = store.storage().clone();
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

/// Stage a new account in `store`; the first launch on it initializes the
/// runtime account.
pub async fn create_account_in(
    store: &ProfileStore,
    nickname_suggestion: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let pending_bootstrap = prepare_pending_account_bootstrap(nickname_suggestion)?;
    create_account_with_pending_bootstrap(store, pending_bootstrap, None).await
}

/// Stage a new production account at `base_path`, refusing when one exists.
pub async fn create_new_account(
    base_path: &Path,
    nickname_suggestion: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let store = ProfileStore::production(base_path)?;
    if let AccountLoadResult::Loaded { authority, .. } = try_load_account(store.storage()).await? {
        return Err(AuraError::invalid(format!(
            "an account ({authority}) already exists at {}",
            base_path.display()
        )));
    }
    create_account_in(&store, nickname_suggestion).await
}

/// Stage a new production account at `base_path`, owning the profile only
/// while writing.
pub async fn create_account(
    base_path: &Path,
    nickname_suggestion: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    create_account_in(&ProfileStore::production(base_path)?, nickname_suggestion).await
}

/// Native configured staging adapter. Semantic completion belongs to the app producer.
pub async fn stage_account_for_bootstrap(
    store: &ProfileStore,
    app: &Arc<async_lock::RwLock<aura_app::ui::types::AppCore>>,
    nickname: String,
    instance: Option<aura_app::ui_contract::OperationInstanceId>,
) -> aura_app::ui_contract::WorkflowTerminalOutcome<(AuthorityId, ContextId)> {
    let storage = store.storage().clone();
    let time = PhysicalTimeHandler::new();
    let crypto = RealCryptoHandler::new();
    aura_app::ui::workflows::account::stage_runtime_free_account_with_terminal_status(
        app, &storage, &time, &crypto, nickname, instance,
    )
    .await
}

/// Persist only an app-issued accepted/adopted enrollment identity.
pub async fn persist_completed_enrollment_runtime_identity(
    store: &ProfileStore,
    completed: &aura_app::ui::workflows::invitation::DeviceEnrollmentImportCompleted,
) -> Result<(), AuraError> {
    let storage = store.storage().clone();
    let old = storage
        .retrieve(ACCOUNT_FILENAME)
        .await
        .map_err(|error| AuraError::Storage {
            message: "Read actual provisional account profile".into(),
            source: Some(Arc::new(error)),
        })?
        .ok_or_else(|| AuraError::storage("Missing actual provisional account profile"))?;
    let mut config: AccountConfig =
        serde_json::from_slice(&old).map_err(|error| AuraError::Internal {
            message: "Decode actual provisional account profile".into(),
            source: Some(Arc::new(error)),
        })?;
    config.authority_id = completed.subject_authority();
    config.context_id = default_context_id_for_authority(completed.subject_authority());
    let identity =
        BootstrapRuntimeIdentity::new(completed.subject_authority(), completed.device_id());
    let identity_bytes = serde_json::to_vec(&identity).map_err(|error| AuraError::Internal {
        message: "Encode accepted runtime identity".into(),
        source: Some(Arc::new(error)),
    })?;
    let config_bytes = serde_json::to_vec_pretty(&config).map_err(|error| AuraError::Internal {
        message: "Encode accepted account profile".into(),
        source: Some(Arc::new(error)),
    })?;
    storage
        .store(SELECTED_RUNTIME_IDENTITY_FILENAME, identity_bytes)
        .await
        .map_err(|error| AuraError::Storage {
            message: "Persist accepted runtime identity".into(),
            source: Some(Arc::new(error)),
        })?;
    storage
        .store(ACCOUNT_FILENAME, config_bytes)
        .await
        .map_err(|error| AuraError::Storage {
            message: "Persist accepted account profile".into(),
            source: Some(Arc::new(error)),
        })?;
    storage
        .remove(PENDING_ACCOUNT_BOOTSTRAP_FILENAME)
        .await
        .map_err(|error| AuraError::Storage {
            message: "Clear obsolete pending bootstrap".into(),
            source: Some(Arc::new(error)),
        })?;
    Ok(())
}

async fn create_account_with_pending_bootstrap(
    store: &ProfileStore,
    pending_bootstrap: PendingAccountBootstrap,
    runtime_identity: Option<BootstrapRuntimeIdentity>,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let staged_event = BootstrapEvent::new(
        BootstrapSurface::Tui,
        BootstrapEventKind::PendingBootstrapStaged,
    );
    tracing::info!(event = %staged_event, path = %store.base_path().display());
    tracing::info!(
        path = %store.base_path().display(),
        nickname = pending_bootstrap.nickname_suggestion,
        pending_device_enrollment = pending_bootstrap.has_pending_device_enrollment(),
        "tui create_account begin"
    );
    let storage = store.storage().clone();
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
    store: &ProfileStore,
    recovered_authority_id: AuthorityId,
    recovered_context_id: Option<ContextId>,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = store.storage().clone();
    let time = PhysicalTimeHandler::new();
    let context_id = recovered_context_id
        .unwrap_or_else(|| derive_recovered_context_id(&recovered_authority_id));

    persist_account_config(&storage, &time, recovered_authority_id, context_id, None).await?;

    Ok((recovered_authority_id, context_id))
}

/// Export account to a portable backup code.
pub async fn export_account_backup(
    store: &ProfileStore,
    device_id: Option<&str>,
) -> Result<String, AuraError> {
    let storage = store.storage().clone();
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
    store: &ProfileStore,
    backup_code: &str,
    overwrite: bool,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = store.storage().clone();
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
    use aura_core::effects::ReactiveEffects;
    use tempfile::tempdir;

    use super::*;

    /// The production profile's record storage (owning the profile while
    /// the returned handle lives).
    fn open_bootstrap_storage(path: &Path) -> Arc<dyn StorageEffects> {
        ProfileStore::production(path).unwrap().storage().clone()
    }

    async fn try_load_account_from_path(path: &Path) -> Result<AccountLoadResult, AuraError> {
        try_load_account(&open_bootstrap_storage(path)).await
    }

    #[tokio::test]
    async fn native_staging_producer_retains_original_account_operation_in_attached_signal() {
        use aura_app::ui::types::{AppConfig, AppCore};
        use aura_app::ui_contract::{
            AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationPhase,
        };
        let dir = tempdir().expect("actual configured native root");
        let app = Arc::new(async_lock::RwLock::new(
            AppCore::new(AppConfig::default()).unwrap(),
        ));
        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let instance = OperationInstanceId("native-stage-original-instance".into());
        let staged = stage_account_for_bootstrap(
            &ProfileStore::production(dir.path()).unwrap(),
            &app,
            "Alice".into(),
            Some(instance.clone()),
        )
        .await;
        let (authority, context) = staged.result.expect("native encrypted writes acknowledged");
        assert!(staged.terminal.is_some());
        let loaded = try_load_account_from_path(dir.path()).await.unwrap();
        assert!(
            matches!(loaded, AccountLoadResult::Loaded { authority: a, context: c, .. } if a == authority && c == context)
        );
        let facts = app.read().await.authoritative_semantic_facts();
        let original = facts.iter().find(|fact| matches!(fact,
        AuthoritativeSemanticFact::OperationStatus { operation_id, instance_id, status, .. }
        if *operation_id == OperationId::account_create() && instance_id.as_ref() == Some(&instance)
            && status.phase == SemanticOperationPhase::Succeeded
    )).expect("actual producer published original terminal").clone();
        let effect_context = crate::handlers::EffectContext::new(
            authority,
            context,
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = Arc::new(
            aura_agent::AgentBuilder::new()
                .with_authority(authority)
                .build_testing_async(&effect_context)
                .await
                .unwrap(),
        );
        app.write()
            .await
            .attach_bootstrap_runtime(agent.clone().as_runtime_bridge())
            .unwrap();

        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let snapshot = app
            .read()
            .await
            .read(&*aura_app::ui::signals::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
            .await
            .unwrap();
        assert!(snapshot.facts.contains(&original));
        assert!(app
            .write()
            .await
            .attach_bootstrap_runtime(agent.clone().as_runtime_bridge())
            .is_err());
        assert!(AppCore::detach_runtime(&app).await);
        drop(app);
        agent
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let agent = match Arc::try_unwrap(agent) {
            Ok(agent) => agent,
            Err(_) => panic!("acknowledged native tasks must release the original agent owner"),
        };
        agent.shutdown(&effect_context).await.unwrap();
    }

    #[tokio::test]
    async fn native_staging_rejects_runtime_backed_app_without_writes_or_success() {
        use aura_app::ui::types::{AppConfig, AppCore};
        use aura_app::ui_contract::{AuthoritativeSemanticFact, SemanticOperationPhase};
        let dir = tempdir().unwrap();
        let authority = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"aura-terminal.account.native-staging-rejects-runtime-backed.authority",
        ));
        let effect_context = crate::handlers::EffectContext::new(
            authority,
            ContextId::new_from_entropy(aura_core::hash::hash(
                b"aura-terminal.account.native-staging-rejects-runtime-backed.context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = Arc::new(
            aura_agent::AgentBuilder::new()
                .with_authority(authority)
                .build_testing_async(&effect_context)
                .await
                .unwrap(),
        );
        let runtime = agent.clone().as_runtime_bridge();

        let app = Arc::new(async_lock::RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
        ));
        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let staged = stage_account_for_bootstrap(
            &ProfileStore::production(dir.path()).unwrap(),
            &app,
            "Alice".into(),
            None,
        )
        .await;
        assert!(staged.result.is_err());
        assert!(staged.terminal.is_some());
        assert!(matches!(
            try_load_account_from_path(dir.path()).await.unwrap(),
            AccountLoadResult::NotFound
        ));
        assert!(!app.read().await.authoritative_semantic_facts().iter().any(|fact| matches!(fact,
        AuthoritativeSemanticFact::OperationStatus { status, .. } if status.phase == SemanticOperationPhase::Succeeded)));
        assert!(AppCore::detach_runtime(&app).await);
        drop(app);
        agent
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let agent = match Arc::try_unwrap(agent) {
            Ok(agent) => agent,
            Err(_) => panic!("acknowledged native tasks must release the original agent owner"),
        };
        agent.shutdown(&effect_context).await.unwrap();
    }

    struct RejectAccountProfile(Arc<dyn StorageEffects>);

    #[async_trait::async_trait]
    impl StorageCoreEffects for RejectAccountProfile {
        async fn store(
            &self,
            key: &str,
            value: Vec<u8>,
        ) -> Result<(), aura_core::effects::StorageError> {
            if key == ACCOUNT_FILENAME {
                return Err(aura_core::effects::StorageError::BackendFailure {
                    operation: "profile acknowledgment".into(),
                    source: AuraError::Storage {
                        message: "injected native profile failure".into(),
                        source: Some(Arc::new(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "original profile cause",
                        ))),
                    },
                });
            }
            self.0.store(key, value).await
        }
        async fn retrieve(
            &self,
            key: &str,
        ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
            self.0.retrieve(key).await
        }
        async fn remove(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
            self.0.remove(key).await
        }
        async fn list_keys(
            &self,
            prefix: Option<&str>,
        ) -> Result<Vec<String>, aura_core::effects::StorageError> {
            self.0.list_keys(prefix).await
        }
    }

    #[tokio::test]
    async fn actual_staging_profile_failure_keeps_native_source_and_never_publishes_success() {
        use aura_app::ui::types::{AppConfig, AppCore};
        use aura_app::ui_contract::{
            AuthoritativeSemanticFact, OperationInstanceId, SemanticOperationPhase,
        };
        use std::error::Error;
        let dir = tempdir().unwrap();
        let app = Arc::new(async_lock::RwLock::new(
            AppCore::new(AppConfig::default()).unwrap(),
        ));
        AppCore::init_signals_with_hooks(&app).await.unwrap();
        let storage = RejectAccountProfile(open_bootstrap_storage(dir.path()));
        let outcome =
            aura_app::ui::workflows::account::stage_runtime_free_account_with_terminal_status(
                &app,
                &storage,
                &PhysicalTimeHandler::new(),
                &RealCryptoHandler::new(),
                "Alice".into(),
                Some(OperationInstanceId("original-profile-failure".into())),
            )
            .await;
        let cause = outcome.result.unwrap_err();
        let mut source = Some(&cause as &(dyn Error + 'static));
        let mut native = false;
        while let Some(error) = source {
            if let Some(io) = error.downcast_ref::<std::io::Error>() {
                assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
                native = true;
            }
            source = error.source();
        }
        assert!(
            native,
            "original backend cause must survive terminal publication"
        );
        assert!(outcome.terminal.is_some());
        assert!(storage
            .retrieve(PENDING_ACCOUNT_BOOTSTRAP_FILENAME)
            .await
            .unwrap()
            .is_some());
        assert!(storage.retrieve(ACCOUNT_FILENAME).await.unwrap().is_none());
        let facts = app.read().await.authoritative_semantic_facts();
        assert!(facts.iter().any(|fact| matches!(fact, AuthoritativeSemanticFact::OperationStatus { status, .. } if status.phase == SemanticOperationPhase::Failed)));
        assert!(!facts.iter().any(|fact| matches!(fact, AuthoritativeSemanticFact::OperationStatus { status, .. } if status.phase == SemanticOperationPhase::Succeeded)));
    }

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
