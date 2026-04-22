use aura_core::tree::AttestedOp;
use aura_core::{ContextId, DeviceId, Hash32};
use aura_guards::{DecodedIngress, IngressSource, VerifiedIngressMetadata};
use aura_sync::protocols::{AntiEntropyConfig, AntiEntropyProtocol};

fn main() {
    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
    let mut local_ops: Vec<AttestedOp> = Vec::new();
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(DeviceId::new_from_entropy([1; 32])),
        ContextId::new_from_entropy([2; 32]),
        None,
        Hash32::zero(),
        1,
    );
    let decoded = DecodedIngress::new(Vec::<AttestedOp>::new(), metadata);

    let _ = protocol.merge_batch(&mut local_ops, decoded);
}
