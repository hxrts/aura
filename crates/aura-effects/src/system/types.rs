//! Shared taxonomy types for system logging and monitoring.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Log level taxonomy used by system handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LogLevel {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(()),
        }
    }
}

impl TryFrom<&str> for LogLevel {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, ()> {
        LogLevel::from_str(value)
    }
}

/// Component identifier taxonomy for system handlers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    System,
    Logging,
    Monitoring,
    Metrics,
    Transport,
    Storage,
    Crypto,
    Network,
    Protocol,
    Consensus,
    Guard,
    Journal,
    Custom(String),
}

impl ComponentId {
    pub fn as_str(&self) -> &str {
        match self {
            ComponentId::System => "system",
            ComponentId::Logging => "logging",
            ComponentId::Monitoring => "monitoring",
            ComponentId::Metrics => "metrics",
            ComponentId::Transport => "transport",
            ComponentId::Storage => "storage",
            ComponentId::Crypto => "crypto",
            ComponentId::Network => "network",
            ComponentId::Protocol => "protocol",
            ComponentId::Consensus => "consensus",
            ComponentId::Guard => "guard",
            ComponentId::Journal => "journal",
            ComponentId::Custom(value) => value.as_str(),
        }
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ComponentId {
    fn from(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => ComponentId::System,
            "logging" => ComponentId::Logging,
            "monitoring" => ComponentId::Monitoring,
            "metrics" => ComponentId::Metrics,
            "transport" => ComponentId::Transport,
            "storage" => ComponentId::Storage,
            "crypto" => ComponentId::Crypto,
            "network" => ComponentId::Network,
            "protocol" => ComponentId::Protocol,
            "consensus" => ComponentId::Consensus,
            "guard" => ComponentId::Guard,
            "journal" => ComponentId::Journal,
            other => ComponentId::Custom(other.to_string()),
        }
    }
}

impl From<String> for ComponentId {
    fn from(value: String) -> Self {
        ComponentId::from(value.as_str())
    }
}

/// Audit action taxonomy for security-relevant operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditAction {
    Create,
    Read,
    Update,
    Delete,
    Authenticate,
    Authorize,
    KeyOperation,
    Rotate,
    Recover,
    Invite,
    Revoke,
    Custom(String),
}

impl AuditAction {
    pub fn as_str(&self) -> &str {
        match self {
            AuditAction::Create => "create",
            AuditAction::Read => "read",
            AuditAction::Update => "update",
            AuditAction::Delete => "delete",
            AuditAction::Authenticate => "authenticate",
            AuditAction::Authorize => "authorize",
            AuditAction::KeyOperation => "key-operation",
            AuditAction::Rotate => "rotate",
            AuditAction::Recover => "recover",
            AuditAction::Invite => "invite",
            AuditAction::Revoke => "revoke",
            AuditAction::Custom(value) => value.as_str(),
        }
    }
}

impl fmt::Display for AuditAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for AuditAction {
    fn from(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "create" => AuditAction::Create,
            "read" => AuditAction::Read,
            "update" => AuditAction::Update,
            "delete" => AuditAction::Delete,
            "authenticate" | "authentication" => AuditAction::Authenticate,
            "authorize" | "authorization" => AuditAction::Authorize,
            "key-operation" | "key_operation" | "keyoperation" => AuditAction::KeyOperation,
            "rotate" | "rotation" => AuditAction::Rotate,
            "recover" | "recovery" => AuditAction::Recover,
            "invite" | "invitation" => AuditAction::Invite,
            "revoke" => AuditAction::Revoke,
            other => AuditAction::Custom(other.to_string()),
        }
    }
}

impl From<String> for AuditAction {
    fn from(value: String) -> Self {
        AuditAction::from(value.as_str())
    }
}
