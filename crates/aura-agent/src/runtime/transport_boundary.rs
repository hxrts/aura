//! Runtime-owned raw transport boundary.

use crate::runtime::AuraEffectSystem;
use aura_core::effects::transport::{TransportEnvelope, TransportError, TransportReceipt};
use aura_core::effects::TransportEffects;

/// Receipt evidence required by the production guarded send API.
#[derive(Debug, Clone)]
pub(crate) struct GuardChainSendReceipt {
    receipt: TransportReceipt,
}

impl GuardChainSendReceipt {
    /// Bind guard-chain receipt evidence to an envelope before transport emit.
    pub(crate) fn bind_to_envelope(
        receipt: TransportReceipt,
        envelope: &TransportEnvelope,
    ) -> Result<Self, TransportError> {
        validate_receipt_matches_envelope(&receipt, envelope)?;
        Ok(Self { receipt })
    }

    pub(crate) fn as_transport_receipt(&self) -> &TransportReceipt {
        &self.receipt
    }

    fn into_transport_receipt(self) -> TransportReceipt {
        self.receipt
    }
}

async fn send_raw_transport_envelope(
    effects: &AuraEffectSystem,
    envelope: TransportEnvelope,
) -> Result<(), TransportError> {
    if !effects.is_testing() && envelope.receipt.is_none() {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "production transport send requires guard-chain receipt evidence".to_string(),
        });
    }
    TransportEffects::send_envelope(effects, envelope).await
}

/// Send an envelope after receipt evidence proves the guard chain has run.
pub(crate) async fn send_guarded_transport_envelope(
    effects: &AuraEffectSystem,
    mut envelope: TransportEnvelope,
) -> Result<(), TransportError> {
    let Some(receipt) = envelope.receipt.take() else {
        if effects.is_testing() {
            return send_raw_transport_envelope(effects, envelope).await;
        }
        return Err(TransportError::ReceiptValidationFailed {
            reason: "production transport send requires guard-chain receipt evidence".to_string(),
        });
    };
    let receipt = GuardChainSendReceipt::bind_to_envelope(receipt, &envelope)?;
    validate_receipt_matches_envelope(receipt.as_transport_receipt(), &envelope)?;
    envelope.receipt = Some(receipt.into_transport_receipt());
    send_raw_transport_envelope(effects, envelope).await
}

fn validate_receipt_matches_envelope(
    receipt: &TransportReceipt,
    envelope: &TransportEnvelope,
) -> Result<(), TransportError> {
    if receipt.context != envelope.context {
        return Err(TransportError::ReceiptValidationFailed {
            reason: format!(
                "receipt context {} does not match envelope context {}",
                receipt.context, envelope.context
            ),
        });
    }
    if receipt.src != envelope.source {
        return Err(TransportError::ReceiptValidationFailed {
            reason: format!(
                "receipt source {} does not match envelope source {}",
                receipt.src, envelope.source
            ),
        });
    }
    if receipt.dst != envelope.destination {
        return Err(TransportError::ReceiptValidationFailed {
            reason: format!(
                "receipt destination {} does not match envelope destination {}",
                receipt.dst, envelope.destination
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GuardChainSendReceipt, TransportEnvelope, TransportReceipt};
    use aura_core::{AuthorityId, ContextId};
    use std::collections::HashMap;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn envelope() -> TransportEnvelope {
        TransportEnvelope {
            destination: authority(2),
            source: authority(1),
            context: context(3),
            payload: b"payload".to_vec(),
            metadata: HashMap::new(),
            receipt: None,
        }
    }

    fn receipt_for(envelope: &TransportEnvelope) -> TransportReceipt {
        TransportReceipt {
            context: envelope.context,
            src: envelope.source,
            dst: envelope.destination,
            epoch: 1,
            cost: 1,
            nonce: 9,
            prev: [0; 32],
            sig: vec![7],
        }
    }

    #[test]
    fn guard_chain_send_receipt_binds_to_matching_envelope() {
        let envelope = envelope();
        let receipt = receipt_for(&envelope);

        let bound = GuardChainSendReceipt::bind_to_envelope(receipt, &envelope);

        assert!(bound.is_ok());
    }

    #[test]
    fn guard_chain_send_receipt_rejects_mismatched_envelope() {
        let envelope = envelope();
        let mut receipt = receipt_for(&envelope);
        receipt.dst = authority(4);

        let bound = GuardChainSendReceipt::bind_to_envelope(receipt, &envelope);

        assert!(bound.is_err());
    }
}
