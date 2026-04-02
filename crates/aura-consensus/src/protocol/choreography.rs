//! Choreography definition for the Aura consensus protocol.

use aura_macros::choreography;

// Define the consensus choreography protocol
choreography!(include_str!("src/protocol/choreography.tell"));

#[cfg(test)]
mod tests {
    use super::telltale_session_types_aura_consensus;

    #[test]
    fn consensus_manifest_remains_theorem_pack_free() {
        let manifest = telltale_session_types_aura_consensus::vm_artifacts::composition_manifest();
        assert!(
            telltale_session_types_aura_consensus::proof_status::REQUIRED_THEOREM_PACKS.is_empty(),
            "consensus choreography should not declare theorem packs until it owns consensus-profile admission",
        );
        assert!(
            manifest.theorem_packs.is_empty(),
            "consensus choreography should remain theorem-pack-free for now",
        );
        assert!(
            manifest.required_theorem_packs.is_empty(),
            "consensus choreography should not require theorem packs yet",
        );
        assert!(
            manifest.required_theorem_pack_capabilities.is_empty(),
            "consensus choreography should not require theorem-pack capabilities yet",
        );
    }
}
