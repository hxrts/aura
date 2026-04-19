use aura_core::effects::amp::AmpChannelError;

use super::error_boundary::bridge_internal;
use aura_app::IntentError;

pub(super) fn map_amp_error(err: AmpChannelError) -> IntentError {
    bridge_internal("AMP operation failed", err)
}
