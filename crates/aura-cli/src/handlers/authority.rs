//! Authority management handler (placeholder implementation).

use crate::commands::authority::AuthorityCommands;
use anyhow::Result;
use aura_agent::AuraEffectSystem;
use aura_core::effects::ConsoleEffects;

/// Execute authority management commands.
pub async fn handle_authority(
    effect_system: &AuraEffectSystem,
    command: &AuthorityCommands,
) -> Result<()> {
    match command {
        AuthorityCommands::Create { threshold } => {
            let msg = match threshold {
                Some(t) => format!(
                    "Authority creation is not yet wired (requested threshold={})",
                    t
                ),
                None => "Authority creation is not yet wired".to_string(),
            };
            let _ = ConsoleEffects::log_info(effect_system, &msg).await;
        }
        AuthorityCommands::Status { authority_id } => {
            let _ = ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "Authority status inspection is not yet available for {}",
                    authority_id
                ),
            )
            .await;
        }
        AuthorityCommands::List => {
            let _ = ConsoleEffects::log_info(
                effect_system,
                "Authority listing is not yet available in this build",
            )
            .await;
        }
        AuthorityCommands::AddDevice {
            authority_id,
            public_key,
        } => {
            let _ = ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "Add-device flow is not yet wired (authority={} key={})",
                    authority_id, public_key
                ),
            )
            .await;
        }
    }
    Ok(())
}
