use aura_app::IntentError;
use aura_core::effects::amp::AmpChannelError;

pub(super) fn map_amp_error(err: AmpChannelError) -> IntentError {
    IntentError::internal_error(format!("AMP error: {err}"))
}
