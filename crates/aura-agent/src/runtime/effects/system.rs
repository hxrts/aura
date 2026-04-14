use super::{AuraEffectSystem, EffectApiLedgerState};
use async_trait::async_trait;
use aura_core::effects::{ConsoleEffects, SystemEffects, SystemError};
use aura_core::AuraError;
use std::collections::HashMap;

// Implementation of ConsoleEffects
#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        tracing::info!("{}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        tracing::warn!("{}", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        tracing::error!("{}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        tracing::debug!("{}", message);
        Ok(())
    }
}

// Implementation of SystemEffects
#[async_trait]
impl SystemEffects for AuraEffectSystem {
    async fn shutdown(&self) -> Result<(), SystemError> {
        #[cfg(not(target_arch = "wasm32"))]
        self.network_connections.write().clear();
        #[cfg(target_arch = "wasm32")]
        self.network_connections.write().clear();
        *self.lan_transport.write() = None;
        *self.rendezvous_manager.write() = None;
        *self.move_manager.write() = None;
        self.effect_api_ledger.lock().events.clear();
        self.effect_api_ledger.lock().device_activity.clear();
        tracing::info!("runtime effect system shutdown requested");
        Ok(())
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let mut info = HashMap::new();
        info.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
        info.insert(
            "execution_mode".to_string(),
            format!("{:?}", self.execution_mode),
        );
        info.insert("authority_id".to_string(), self.authority_id.to_string());
        info.insert("device_id".to_string(), self.device_id().to_string());
        info.insert(
            "harness_mode_enabled".to_string(),
            self.harness_mode_enabled.to_string(),
        );
        info.insert("platform".to_string(), std::env::consts::OS.to_string());
        Ok(info)
    }

    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        // Use tracing instead of println to avoid corrupting TUI
        match level.to_lowercase().as_str() {
            "error" => tracing::error!(component = component, "{}", message),
            "warn" => tracing::warn!(component = component, "{}", message),
            "debug" => tracing::debug!(component = component, "{}", message),
            "trace" => tracing::trace!(component = component, "{}", message),
            _ => tracing::info!(component = component, "{}", message),
        }
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        _context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        // Use tracing instead of println to avoid corrupting TUI
        match level.to_lowercase().as_str() {
            "error" => tracing::error!(component = component, "{}", message),
            "warn" => tracing::warn!(component = component, "{}", message),
            "debug" => tracing::debug!(component = component, "{}", message),
            "trace" => tracing::trace!(component = component, "{}", message),
            _ => tracing::info!(component = component, "{}", message),
        }
        Ok(())
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        self.system_config
            .write()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        self.system_config
            .read()
            .get(key)
            .cloned()
            .ok_or_else(|| SystemError::ResourceNotFound {
                resource: format!("config:{key}"),
            })
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        Ok(self.biscuit_cache.read().is_some() || self.execution_mode.is_deterministic())
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let ledger = self.effect_api_ledger.lock();
        let mut metrics = HashMap::new();
        metrics.insert(
            "choreography_active_sessions".to_string(),
            self.choreography_state.read().active_session_count() as f64,
        );
        metrics.insert("effect_api_events".to_string(), ledger.events.len() as f64);
        metrics.insert(
            "effect_api_known_devices".to_string(),
            ledger.device_activity.len() as f64,
        );
        metrics.insert(
            "system_config_overrides".to_string(),
            self.system_config.read().len() as f64,
        );
        #[cfg(not(target_arch = "wasm32"))]
        metrics.insert(
            "network_connections".to_string(),
            self.network_connections.read().len() as f64,
        );
        #[cfg(target_arch = "wasm32")]
        metrics.insert(
            "network_connections".to_string(),
            self.network_connections.read().len() as f64,
        );
        Ok(metrics)
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        match component {
            "effect_api" => {
                *self.effect_api_ledger.lock() = EffectApiLedgerState::default();
                Ok(())
            }
            "network" | "network_connections" => {
                #[cfg(not(target_arch = "wasm32"))]
                self.network_connections.write().clear();
                #[cfg(target_arch = "wasm32")]
                self.network_connections.write().clear();
                Ok(())
            }
            "lan_transport" => {
                *self.lan_transport.write() = None;
                Ok(())
            }
            "rendezvous_manager" => {
                *self.rendezvous_manager.write() = None;
                Ok(())
            }
            "move_manager" => {
                *self.move_manager.write() = None;
                Ok(())
            }
            "logging" | "metrics" => Ok(()),
            other => Err(SystemError::ResourceNotFound {
                resource: format!("component:{other}"),
            }),
        }
    }
}
