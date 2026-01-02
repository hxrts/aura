use super::AuraEffectSystem;
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
        self.ensure_mock_system("shutdown")?;
        Ok(())
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        self.ensure_mock_system("get_system_info")?;
        let mut info = HashMap::new();
        info.insert("version".to_string(), "0.1.0".to_string());
        info.insert("build_time".to_string(), "mock".to_string());
        info.insert("commit_hash".to_string(), "mock".to_string());
        info.insert("platform".to_string(), "test".to_string());
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

    async fn set_config(&self, _key: &str, _value: &str) -> Result<(), SystemError> {
        self.ensure_mock_system("set_config")?;
        Ok(())
    }

    async fn get_config(&self, _key: &str) -> Result<String, SystemError> {
        self.ensure_mock_system("get_config")?;
        Ok("mock_value".to_string())
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        self.ensure_mock_system("health_check")?;
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        self.ensure_mock_system("get_metrics")?;
        Ok(HashMap::new())
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        self.ensure_mock_system("restart_component")?;
        Ok(())
    }
}
