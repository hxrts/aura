use aura_core::{
    ContextId, DeviceId, Hash32, KeyResolutionError, TrustedKeyResolver, TrustedPublicKey,
};
use aura_guards::{DecodedIngress, IngressSource, VerifiedIngressMetadata};
use aura_sync::protocols::receipts::Receipt;
use aura_sync::protocols::{ReceiptVerificationConfig, ReceiptVerificationProtocol};

struct Keys;

impl TrustedKeyResolver for Keys {
    fn resolve_authority_threshold_key(
        &self,
        _authority: aura_core::AuthorityId,
        _epoch: u64,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        unimplemented!()
    }

    fn resolve_device_key(&self, _device: DeviceId) -> Result<TrustedPublicKey, KeyResolutionError> {
        unimplemented!()
    }

    fn resolve_guardian_key(
        &self,
        _guardian: aura_core::AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        unimplemented!()
    }

    fn resolve_release_key(
        &self,
        _authority: aura_core::AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError> {
        unimplemented!()
    }
}

fn crypto() -> &'static aura_testkit::mock_effects::MockEffects {
    unimplemented!()
}

fn main() {
    let protocol = ReceiptVerificationProtocol::new(ReceiptVerificationConfig::default());
    let signer = DeviceId::new_from_entropy([1; 32]);
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(signer),
        ContextId::new_from_entropy([2; 32]),
        None,
        Hash32::zero(),
        1,
    );
    let decoded = DecodedIngress::new(
        Receipt {
            message_hash: Hash32::zero(),
            signer,
            public_key: vec![0; 32],
            signature: vec![0; 64],
            timestamp: 1,
            consensus_id: None,
            previous_receipt: None,
        },
        metadata,
    );

    let _ = protocol.verify_receipt(&decoded, crypto(), &Keys);
}
