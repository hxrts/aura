//! Terminal environment configuration helpers.
//!
//! Scope is intentionally split:
//! - product runtime overrides used by the real TUI/CLI shell
//! - harness/tooling overrides consumed by terminal-owned bring-up paths

use std::path::PathBuf;

use aura_agent::BootstrapBrokerLanBindPolicy;

const AURA_TUI_ALLOW_STDIO: &str = "AURA_TUI_ALLOW_STDIO";
const AURA_TUI_LOG_PATH: &str = "AURA_TUI_LOG_PATH";
const AURA_DEMO_DEVICE_ID: &str = "AURA_DEMO_DEVICE_ID";
const AURA_CLIPBOARD_MODE: &str = "AURA_CLIPBOARD_MODE";
const AURA_CLIPBOARD_FILE: &str = "AURA_CLIPBOARD_FILE";
const AURA_HARNESS_LAN_DISCOVERY_ENABLED: &str = "AURA_HARNESS_LAN_DISCOVERY_ENABLED";
const AURA_HARNESS_LAN_DISCOVERY_BIND_ADDR: &str = "AURA_HARNESS_LAN_DISCOVERY_BIND_ADDR";
const AURA_HARNESS_LAN_DISCOVERY_BROADCAST_ADDR: &str = "AURA_HARNESS_LAN_DISCOVERY_BROADCAST_ADDR";
const AURA_HARNESS_LAN_DISCOVERY_PORT: &str = "AURA_HARNESS_LAN_DISCOVERY_PORT";
const AURA_BOOTSTRAP_BROKER_BIND: &str = "AURA_BOOTSTRAP_BROKER_BIND";
const AURA_BOOTSTRAP_BROKER_URL: &str = "AURA_BOOTSTRAP_BROKER_URL";
const AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND: &str = "AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND";
const AURA_BOOTSTRAP_BROKER_AUTH_TOKEN: &str = "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN";
const AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN: &str = "AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN";

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessLanDiscoveryEnv {
    pub enabled: bool,
    pub bind_addr: String,
    pub broadcast_addr: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapBrokerEnv {
    pub bind_addr: Option<String>,
    pub base_url: Option<String>,
    pub lan_bind_policy: BootstrapBrokerLanBindPolicy,
    pub auth_token: Option<String>,
    pub invitation_retrieval_token: Option<String>,
}

pub fn tui_allows_stdio() -> bool {
    non_empty_env(AURA_TUI_ALLOW_STDIO).as_deref() == Some("1")
}

pub fn tui_log_path_override() -> Option<String> {
    non_empty_env(AURA_TUI_LOG_PATH)
}

pub fn demo_device_id_override() -> Option<String> {
    non_empty_env(AURA_DEMO_DEVICE_ID)
}

pub fn clipboard_mode_override() -> Option<String> {
    non_empty_env(AURA_CLIPBOARD_MODE)
}

pub fn clipboard_capture_file() -> Option<PathBuf> {
    non_empty_env(AURA_CLIPBOARD_FILE).map(PathBuf::from)
}

pub fn harness_lan_discovery_override() -> Option<HarnessLanDiscoveryEnv> {
    let enabled = non_empty_env(AURA_HARNESS_LAN_DISCOVERY_ENABLED)
        .and_then(|value| value.parse::<bool>().ok())?;
    Some(HarnessLanDiscoveryEnv {
        enabled,
        bind_addr: non_empty_env(AURA_HARNESS_LAN_DISCOVERY_BIND_ADDR)
            .unwrap_or_else(|| "0.0.0.0".to_string()),
        broadcast_addr: non_empty_env(AURA_HARNESS_LAN_DISCOVERY_BROADCAST_ADDR)
            .unwrap_or_else(|| "255.255.255.255".to_string()),
        port: non_empty_env(AURA_HARNESS_LAN_DISCOVERY_PORT)
            .and_then(|value| value.parse::<u16>().ok()),
    })
}

pub fn bootstrap_broker_override() -> Option<BootstrapBrokerEnv> {
    let override_env = BootstrapBrokerEnv {
        bind_addr: non_empty_env(AURA_BOOTSTRAP_BROKER_BIND),
        base_url: non_empty_env(AURA_BOOTSTRAP_BROKER_URL),
        lan_bind_policy: match non_empty_env(AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND)
            .and_then(|value| value.parse::<bool>().ok())
            .unwrap_or(false)
        {
            true => BootstrapBrokerLanBindPolicy::AllowLanDevOnly,
            false => BootstrapBrokerLanBindPolicy::LoopbackOnly,
        },
        auth_token: non_empty_env(AURA_BOOTSTRAP_BROKER_AUTH_TOKEN),
        invitation_retrieval_token: non_empty_env(AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN),
    };
    if override_env.bind_addr.is_none() && override_env.base_url.is_none() {
        None
    } else {
        Some(override_env)
    }
}
