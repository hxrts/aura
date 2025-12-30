//! Admin maintenance commands (replacement, fork controls).
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::identifiers::{AccountId, AuthorityId};
use aura_app::ui::workflows::admin;

use crate::AdminAction;

/// Handle admin-related maintenance commands.
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_admin(
    ctx: &HandlerContext<'_>,
    action: &AdminAction,
) -> TerminalResult<CliOutput> {
    match action {
        AdminAction::Replace {
            account,
            new_admin,
            activation_epoch,
        } => replace_admin(ctx, account, new_admin, *activation_epoch).await,
    }
}

async fn replace_admin(
    ctx: &HandlerContext<'_>,
    account: &str,
    new_admin: &str,
    activation_epoch: u64,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let account_id: AccountId = account
        .parse()
        .map_err(|e: uuid::Error| TerminalError::Input(format!("{e}")))?;
    let new_admin_id: AuthorityId = new_admin
        .parse()
        .map_err(|e: uuid::Error| TerminalError::Input(format!("{e}")))?;

    output.println(format!(
        "Replacing admin for account {account_id} with {new_admin_id} (activation epoch {activation_epoch})"
    ));

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(ctx.device_id().0);

    admin::replace_admin(
        ctx.effects(),
        authority_id,
        account_id,
        new_admin_id,
        activation_epoch,
    )
    .await
    .map_err(|e| TerminalError::Operation(format!("Failed to replace admin: {e}")))?;

    output.println(format!(
        "Admin replacement recorded; new admin {new_admin_id} activates at epoch {activation_epoch}"
    ));

    Ok(output)
}
