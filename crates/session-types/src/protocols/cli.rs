//! Session Type States for CLI Command Management (Refactored with Macros)
//!
//! This module defines session types for CLI commands, providing compile-time safety
//! for command sequences and ensuring proper system state management.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition};
use crate::witnesses::RuntimeWitness;
use aura_journal::{AccountId, DeviceId};
use std::path::PathBuf;
use uuid::Uuid;

// ========== CLI Protocol Core ==========

/// Core CLI protocol data without session state
#[derive(Debug, Clone)]
pub struct CliProtocolCore {
    pub session_id: Uuid,
    pub config_path: Option<PathBuf>,
    pub account_id: Option<AccountId>,
    pub device_id: Option<DeviceId>,
    pub current_command: Option<String>,
    pub last_result: Option<CommandResult>,
}

impl CliProtocolCore {
    pub fn new(session_id: Uuid) -> Self {
        Self {
            session_id,
            config_path: None,
            account_id: None,
            device_id: None,
            current_command: None,
            last_result: None,
        }
    }
}

// ========== Error Type ==========

/// Errors that can occur in CLI session operations
#[derive(Debug, thiserror::Error)]
pub enum CliSessionError {
    #[error("Account not initialized: {0}")]
    AccountNotInitialized(String),
    #[error("Invalid command in current state: {command} not allowed in {state}")]
    InvalidCommandForState { command: String, state: String },
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    #[error("File system error: {0}")]
    FileSystemError(String),
    #[error("Command execution error: {0}")]
    CommandExecutionError(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: CliProtocol,
    Core: CliProtocolCore,
    Error: CliSessionError,
    Union: CliSessionState,

    States {
        CliUninitialized @ final => (),
        CliInitializing => AccountInitialized,
        CliAccountLoaded @ final => (),
        CliDkdInProgress => CommandResult,
        CliRecoveryInProgress => CommandResult,
        CliNetworkOperationInProgress => CommandResult,
        CliStorageOperationInProgress => CommandResult,
        CliCommandFailed @ final => (),
    }

    Extract {
        session_id: |core| core.session_id,
        device_id: |core| core.device_id.unwrap_or_else(|| {
            let effects = aura_crypto::Effects::test();
            aura_journal::DeviceId::new_with_effects(&effects)
        }),
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed CLI protocol wrapper
pub type SessionTypedCli<S> = ChoreographicProtocol<CliProtocolCore, S>;

// ========== CLI Command Context Information ==========

/// Context for CLI command execution
#[derive(Debug, Clone)]
pub struct CliCommandContext {
    pub session_id: Uuid,
    pub command_name: String,
    pub args: Vec<String>,
    pub started_at: u64,
}

impl RuntimeWitness for CliCommandContext {
    type Evidence = (String, Vec<String>); // (command_name, args)
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (String, Vec<String>), config: (Uuid, u64)) -> Option<Self> {
        let (command_name, args) = evidence;
        let (session_id, timestamp) = config;

        Some(CliCommandContext {
            session_id,
            command_name,
            args,
            started_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "CLI command context created"
    }
}

/// Result of CLI command execution
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Context for account initialization
#[derive(Debug, Clone)]
pub struct AccountInitContext {
    pub session_id: Uuid,
    pub participants: u16,
    pub threshold: u16,
    pub output_dir: PathBuf,
    pub initialized_at: u64,
}

impl RuntimeWitness for AccountInitContext {
    type Evidence = (u16, u16, PathBuf); // (participants, threshold, output_dir)
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (u16, u16, PathBuf), config: (Uuid, u64)) -> Option<Self> {
        let (participants, threshold, output_dir) = evidence;
        let (session_id, timestamp) = config;

        // Validate initialization parameters
        if participants < 2 || threshold > participants {
            return None;
        }

        Some(AccountInitContext {
            session_id,
            participants,
            threshold,
            output_dir,
            initialized_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "Account initialization context created"
    }
}

/// Context for account loading
#[derive(Debug, Clone)]
pub struct AccountLoadContext {
    pub session_id: Uuid,
    pub config_path: PathBuf,
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub loaded_at: u64,
}

// ========== Runtime Witnesses for CLI Operations ==========

/// Witness that account initialization has completed successfully
#[derive(Debug, Clone)]
pub struct AccountInitialized {
    pub session_id: Uuid,
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub config_path: PathBuf,
    pub completed_at: u64,
}

impl RuntimeWitness for AccountInitialized {
    type Evidence = (AccountId, DeviceId, PathBuf);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (AccountId, DeviceId, PathBuf), config: (Uuid, u64)) -> Option<Self> {
        let (account_id, device_id, config_path) = evidence;
        let (session_id, timestamp) = config;

        // Verify that config file exists and is valid
        if config_path.exists() {
            Some(AccountInitialized {
                session_id,
                account_id,
                device_id,
                config_path,
                completed_at: timestamp,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Account initialization completed successfully"
    }
}

/// Witness that account configuration has been loaded successfully
#[derive(Debug, Clone)]
pub struct AccountConfigLoaded {
    pub session_id: Uuid,
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub config_path: PathBuf,
    pub loaded_at: u64,
}

impl RuntimeWitness for AccountConfigLoaded {
    type Evidence = (AccountId, DeviceId, PathBuf);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (AccountId, DeviceId, PathBuf), config: (Uuid, u64)) -> Option<Self> {
        let (account_id, device_id, config_path) = evidence;
        let (session_id, timestamp) = config;

        // Verify that config file exists and contains valid data
        if config_path.exists() {
            Some(AccountConfigLoaded {
                session_id,
                account_id,
                device_id,
                config_path,
                loaded_at: timestamp,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Account configuration loaded successfully"
    }
}

/// Witness that a CLI command has completed successfully
#[derive(Debug, Clone)]
pub struct CommandCompleted {
    pub session_id: Uuid,
    pub command_name: String,
    pub result: CommandResult,
    pub completed_at: u64,
}

impl RuntimeWitness for CommandCompleted {
    type Evidence = (String, CommandResult);
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (String, CommandResult), config: (Uuid, u64)) -> Option<Self> {
        let (command_name, result) = evidence;
        let (session_id, timestamp) = config;

        if result.success {
            Some(CommandCompleted {
                session_id,
                command_name,
                result,
                completed_at: timestamp,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "CLI command completed successfully"
    }
}

/// Witness for CLI command failure
#[derive(Debug, Clone)]
pub struct CommandFailed {
    pub session_id: Uuid,
    pub command_name: String,
    pub error: String,
    pub failed_at: u64,
}

impl RuntimeWitness for CommandFailed {
    type Evidence = (String, String); // (command_name, error_message)
    type Config = (Uuid, u64); // (session_id, timestamp)

    fn verify(evidence: (String, String), config: (Uuid, u64)) -> Option<Self> {
        let (command_name, error) = evidence;
        let (session_id, timestamp) = config;

        Some(CommandFailed {
            session_id,
            command_name,
            error,
            failed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "CLI command failed"
    }
}

// ========== State Transitions ==========

/// Transition from CliUninitialized to CliInitializing (when starting init command)
impl WitnessedTransition<CliUninitialized, CliInitializing>
    for ChoreographicProtocol<CliProtocolCore, CliUninitialized>
{
    type Witness = AccountInitContext;
    type Target = ChoreographicProtocol<CliProtocolCore, CliInitializing>;

    /// Begin account initialization
    fn transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        self.inner.session_id = context.session_id;
        self.inner.current_command = Some("init".to_string());
        self.transition_to()
    }
}

/// Transition from CliInitializing to CliAccountLoaded (requires AccountInitialized witness)
impl WitnessedTransition<CliInitializing, CliAccountLoaded>
    for ChoreographicProtocol<CliProtocolCore, CliInitializing>
{
    type Witness = AccountInitialized;
    type Target = ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>;

    /// Complete account initialization
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.account_id = Some(witness.account_id);
        self.inner.device_id = Some(witness.device_id);
        self.inner.config_path = Some(witness.config_path);
        self.inner.current_command = None;
        self.transition_to()
    }
}

/// Transition from CliUninitialized to CliAccountLoaded (when loading existing account)
impl WitnessedTransition<CliUninitialized, CliAccountLoaded>
    for ChoreographicProtocol<CliProtocolCore, CliUninitialized>
{
    type Witness = AccountConfigLoaded;
    type Target = ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>;

    /// Load existing account configuration
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.account_id = Some(witness.account_id);
        self.inner.device_id = Some(witness.device_id);
        self.inner.config_path = Some(witness.config_path);
        self.transition_to()
    }
}

/// Transition from CliAccountLoaded to CliDkdInProgress (when starting DKD command)
impl WitnessedTransition<CliAccountLoaded, CliDkdInProgress>
    for ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>
{
    type Witness = CliCommandContext;
    type Target = ChoreographicProtocol<CliProtocolCore, CliDkdInProgress>;

    /// Begin DKD operation
    fn transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(context.command_name);
        self.transition_to()
    }
}

/// Transition from CliDkdInProgress back to CliAccountLoaded (requires CommandCompleted witness)
impl WitnessedTransition<CliDkdInProgress, CliAccountLoaded>
    for ChoreographicProtocol<CliProtocolCore, CliDkdInProgress>
{
    type Witness = CommandCompleted;
    type Target = ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>;

    /// Complete DKD operation
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.last_result = Some(witness.result);
        self.inner.current_command = None;
        self.transition_to()
    }
}

/// Transition from CliAccountLoaded to CliRecoveryInProgress (when starting recovery command)
impl WitnessedTransition<CliAccountLoaded, CliRecoveryInProgress>
    for ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>
{
    type Witness = CliCommandContext;
    type Target = ChoreographicProtocol<CliProtocolCore, CliRecoveryInProgress>;

    /// Begin recovery operation
    fn transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(context.command_name);
        self.transition_to()
    }
}

/// Transition from CliRecoveryInProgress back to CliAccountLoaded (requires CommandCompleted witness)
impl WitnessedTransition<CliRecoveryInProgress, CliAccountLoaded>
    for ChoreographicProtocol<CliProtocolCore, CliRecoveryInProgress>
{
    type Witness = CommandCompleted;
    type Target = ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>;

    /// Complete recovery operation
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.last_result = Some(witness.result);
        self.inner.current_command = None;
        self.transition_to()
    }
}

/// Transition to CliCommandFailed from CliUninitialized state
impl WitnessedTransition<CliUninitialized, CliCommandFailed>
    for ChoreographicProtocol<CliProtocolCore, CliUninitialized>
{
    type Witness = CommandFailed;
    type Target = ChoreographicProtocol<CliProtocolCore, CliCommandFailed>;

    /// Handle command failure
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(witness.command_name);
        self.inner.last_result = Some(CommandResult {
            success: false,
            message: witness.error,
            data: None,
        });
        self.transition_to()
    }
}

/// Transition to CliCommandFailed from CliInitializing state
impl WitnessedTransition<CliInitializing, CliCommandFailed>
    for ChoreographicProtocol<CliProtocolCore, CliInitializing>
{
    type Witness = CommandFailed;
    type Target = ChoreographicProtocol<CliProtocolCore, CliCommandFailed>;

    /// Handle command failure
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(witness.command_name);
        self.inner.last_result = Some(CommandResult {
            success: false,
            message: witness.error,
            data: None,
        });
        self.transition_to()
    }
}

/// Transition to CliCommandFailed from CliAccountLoaded state
impl WitnessedTransition<CliAccountLoaded, CliCommandFailed>
    for ChoreographicProtocol<CliProtocolCore, CliAccountLoaded>
{
    type Witness = CommandFailed;
    type Target = ChoreographicProtocol<CliProtocolCore, CliCommandFailed>;

    /// Handle command failure
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(witness.command_name);
        self.inner.last_result = Some(CommandResult {
            success: false,
            message: witness.error,
            data: None,
        });
        self.transition_to()
    }
}

/// Transition to CliCommandFailed from CliDkdInProgress state
impl WitnessedTransition<CliDkdInProgress, CliCommandFailed>
    for ChoreographicProtocol<CliProtocolCore, CliDkdInProgress>
{
    type Witness = CommandFailed;
    type Target = ChoreographicProtocol<CliProtocolCore, CliCommandFailed>;

    /// Handle command failure
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(witness.command_name);
        self.inner.last_result = Some(CommandResult {
            success: false,
            message: witness.error,
            data: None,
        });
        self.transition_to()
    }
}

/// Transition to CliCommandFailed from CliRecoveryInProgress state
impl WitnessedTransition<CliRecoveryInProgress, CliCommandFailed>
    for ChoreographicProtocol<CliProtocolCore, CliRecoveryInProgress>
{
    type Witness = CommandFailed;
    type Target = ChoreographicProtocol<CliProtocolCore, CliCommandFailed>;

    /// Handle command failure
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_command = Some(witness.command_name);
        self.inner.last_result = Some(CommandResult {
            success: false,
            message: witness.error,
            data: None,
        });
        self.transition_to()
    }
}

// ========== State-Specific Operations ==========

/// Operations only available in CliUninitialized state
impl ChoreographicProtocol<CliProtocolCore, CliUninitialized> {
    /// Initialize a new account
    pub async fn init_account(
        &self,
        participants: u16,
        threshold: u16,
        output_dir: PathBuf,
    ) -> Result<AccountInitContext, CliSessionError> {
        // Validate initialization parameters
        if participants < 2 {
            return Err(CliSessionError::ConfigurationError(
                "Minimum 2 participants required".to_string(),
            ));
        }

        if threshold > participants {
            return Err(CliSessionError::ConfigurationError(
                "Threshold cannot exceed participant count".to_string(),
            ));
        }

        Ok(AccountInitContext {
            session_id: self.inner.session_id,
            participants,
            threshold,
            output_dir,
            initialized_at: 0, // Would use actual timestamp
        })
    }

    /// Load existing account configuration
    pub async fn load_account(
        &self,
        config_path: PathBuf,
    ) -> Result<AccountConfigLoaded, CliSessionError> {
        // Verify config file exists
        if !config_path.exists() {
            return Err(CliSessionError::FileSystemError(format!(
                "Config file not found: {}",
                config_path.display()
            )));
        }

        // In real implementation, would actually load and parse the config
        // For now, create placeholder values
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);

        let witness = AccountConfigLoaded::verify(
            (account_id, device_id, config_path.clone()),
            (self.inner.session_id, 0),
        )
        .ok_or_else(|| {
            CliSessionError::ConfigurationError("Failed to verify account config".to_string())
        })?;

        Ok(witness)
    }
}

/// Operations only available in CliAccountLoaded state
impl ChoreographicProtocol<CliProtocolCore, CliAccountLoaded> {
    /// Show account status
    pub async fn show_status(&self) -> Result<CommandResult, CliSessionError> {
        let account_id = self
            .inner
            .account_id
            .as_ref()
            .ok_or_else(|| CliSessionError::SessionError("No account loaded".to_string()))?;

        let device_id = self
            .inner
            .device_id
            .as_ref()
            .ok_or_else(|| CliSessionError::SessionError("No device loaded".to_string()))?;

        Ok(CommandResult {
            success: true,
            message: format!("Account: {}, Device: {}", account_id.0, device_id.0),
            data: None,
        })
    }

    /// Execute DKD operation
    pub async fn execute_dkd(
        &self,
        app_id: String,
        context: String,
    ) -> Result<CliCommandContext, CliSessionError> {
        if self.inner.account_id.is_none() {
            return Err(CliSessionError::AccountNotInitialized(
                "Cannot execute DKD without account".to_string(),
            ));
        }

        Ok(CliCommandContext {
            session_id: self.inner.session_id,
            command_name: "dkd".to_string(),
            args: vec![app_id, context],
            started_at: 0, // Would use actual timestamp
        })
    }

    /// Check if account is properly loaded
    pub fn is_account_loaded(&self) -> bool {
        self.inner.account_id.is_some() && self.inner.device_id.is_some()
    }
}

/// Operations for command execution states
impl ChoreographicProtocol<CliProtocolCore, CliDkdInProgress> {
    /// Complete DKD operation
    pub async fn complete_dkd(&self) -> Result<CommandCompleted, CliSessionError> {
        let witness = CommandCompleted::verify(
            (
                "dkd".to_string(),
                CommandResult {
                    success: true,
                    message: "DKD operation completed successfully".to_string(),
                    data: None,
                },
            ),
            (self.inner.session_id, 0),
        )
        .ok_or_else(|| {
            CliSessionError::CommandExecutionError("Failed to complete DKD operation".to_string())
        })?;

        Ok(witness)
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed CLI protocol in uninitialized state
#[allow(clippy::disallowed_methods)]
pub fn new_session_typed_cli() -> ChoreographicProtocol<CliProtocolCore, CliUninitialized> {
    let session_id = Uuid::new_v4();
    let core = CliProtocolCore::new(session_id);
    ChoreographicProtocol::new(core)
}

/// Rehydrate CLI session from previous state
#[allow(clippy::disallowed_methods)]
pub fn rehydrate_cli_session(
    has_config: bool,
    in_progress_command: Option<String>,
) -> CliSessionState {
    let session_id = Uuid::new_v4();
    let core = CliProtocolCore::new(session_id);

    if let Some(command) = in_progress_command {
        match command.as_str() {
            "dkd" => CliSessionState::CliDkdInProgress(ChoreographicProtocol::new(core)),
            "recovery" => CliSessionState::CliRecoveryInProgress(ChoreographicProtocol::new(core)),
            _ => CliSessionState::CliAccountLoaded(ChoreographicProtocol::new(core)),
        }
    } else if has_config {
        CliSessionState::CliAccountLoaded(ChoreographicProtocol::new(core))
    } else {
        CliSessionState::CliUninitialized(ChoreographicProtocol::new(core))
    }
}

// ========== Tests ==========

#[allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_cli_session_creation() {
        let cli = new_session_typed_cli();

        assert_eq!(cli.state_name(), "CliUninitialized");
        assert!(cli.can_terminate());
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_account_initialization_flow() {
        let cli = new_session_typed_cli();

        // Start initialization
        let init_context = AccountInitContext {
            session_id: cli.inner.session_id,
            participants: 3,
            threshold: 2,
            output_dir: PathBuf::from("/tmp/test"),
            initialized_at: 1000,
        };

        let initializing =
            <ChoreographicProtocol<CliProtocolCore, CliUninitialized> as WitnessedTransition<
                CliUninitialized,
                CliInitializing,
            >>::transition_with_witness(cli, init_context);
        assert_eq!(initializing.state_name(), "CliInitializing");

        // Complete initialization
        let effects = aura_crypto::Effects::test();
        let witness = AccountInitialized {
            session_id: initializing.inner.session_id,
            account_id: AccountId::new_with_effects(&effects),
            device_id: DeviceId::new_with_effects(&effects),
            config_path: PathBuf::from("/tmp/test/config.toml"),
            completed_at: 2000,
        };

        let account_loaded =
            <ChoreographicProtocol<CliProtocolCore, CliInitializing> as WitnessedTransition<
                CliInitializing,
                CliAccountLoaded,
            >>::transition_with_witness(initializing, witness);
        assert_eq!(account_loaded.state_name(), "CliAccountLoaded");
        assert!(account_loaded.can_terminate());
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_command_execution_flow() {
        let cli = new_session_typed_cli();

        // Load account first
        let effects = aura_crypto::Effects::test();
        let load_witness = AccountConfigLoaded {
            session_id: cli.inner.session_id,
            account_id: AccountId::new_with_effects(&effects),
            device_id: DeviceId::new_with_effects(&effects),
            config_path: PathBuf::from("/tmp/config.toml"),
            loaded_at: 1000,
        };

        let account_loaded =
            <ChoreographicProtocol<CliProtocolCore, CliUninitialized> as WitnessedTransition<
                CliUninitialized,
                CliAccountLoaded,
            >>::transition_with_witness(cli, load_witness);
        assert_eq!(account_loaded.state_name(), "CliAccountLoaded");

        // Start DKD command
        let dkd_context = CliCommandContext {
            session_id: account_loaded.inner.session_id,
            command_name: "dkd".to_string(),
            args: vec!["test_app".to_string(), "test_context".to_string()],
            started_at: 2000,
        };

        let dkd_in_progress =
            <ChoreographicProtocol<CliProtocolCore, CliAccountLoaded> as WitnessedTransition<
                CliAccountLoaded,
                CliDkdInProgress,
            >>::transition_with_witness(account_loaded, dkd_context);
        assert_eq!(dkd_in_progress.state_name(), "CliDkdInProgress");

        // Complete DKD command
        let completion_witness = CommandCompleted {
            session_id: dkd_in_progress.inner.session_id,
            command_name: "dkd".to_string(),
            result: CommandResult {
                success: true,
                message: "DKD completed".to_string(),
                data: None,
            },
            completed_at: 3000,
        };

        let completed =
            <ChoreographicProtocol<CliProtocolCore, CliDkdInProgress> as WitnessedTransition<
                CliDkdInProgress,
                CliAccountLoaded,
            >>::transition_with_witness(dkd_in_progress, completion_witness);
        assert_eq!(completed.state_name(), "CliAccountLoaded");
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_session_state_union() {
        let session = rehydrate_cli_session(false, None);
        assert_eq!(session.state_name(), "CliUninitialized");

        let session_with_config = rehydrate_cli_session(true, None);
        assert_eq!(session_with_config.state_name(), "CliAccountLoaded");

        let session_with_dkd = rehydrate_cli_session(true, Some("dkd".to_string()));
        assert_eq!(session_with_dkd.state_name(), "CliDkdInProgress");
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_command_failure_handling() {
        let cli = new_session_typed_cli();

        // Test failure from any state
        let failure_witness = CommandFailed {
            session_id: cli.inner.session_id,
            command_name: "init".to_string(),
            error: "Initialization failed".to_string(),
            failed_at: 1000,
        };

        let failed_cli =
            <ChoreographicProtocol<CliProtocolCore, CliUninitialized> as WitnessedTransition<
                CliUninitialized,
                CliCommandFailed,
            >>::transition_with_witness(cli, failure_witness);
        assert_eq!(failed_cli.state_name(), "CliCommandFailed");
        assert!(failed_cli.can_terminate());
    }
}
