use crate::error::{TerminalError, TerminalResult};
use crate::handlers::HandlerContext;
use aura_core::effects::StorageEffects;

/// Load config bytes from StorageEffects by key (usually a path string).
pub async fn load_config_bytes(ctx: &HandlerContext<'_>, key: &str) -> TerminalResult<Vec<u8>> {
    let data = ctx
        .effects()
        .retrieve(key)
        .await
        .map_err(|e| TerminalError::Config(format!("read {}: {}", key, e)))?
        .ok_or_else(|| TerminalError::NotFound(format!("config {}", key)))?;
    Ok(data)
}

/// Load UTF-8 config string from StorageEffects by key.
pub async fn load_config_utf8(ctx: &HandlerContext<'_>, key: &str) -> TerminalResult<String> {
    let bytes = load_config_bytes(ctx, key).await?;
    String::from_utf8(bytes)
        .map_err(|e| TerminalError::Config(format!("config {} is not UTF-8: {}", key, e)))
}
