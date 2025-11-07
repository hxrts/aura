//! Test choreography to verify AuraHandlerAdapter integration

use aura_choreography::runtime::{AuraEndpoint, AuraHandlerAdapter, AuraHandlerAdapterFactory};
use aura_choreography::types::{ChoreographicRole, Proposal, Response};

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceId;

    #[tokio::test]
    async fn test_simple_choreography_compilation() {
        // Test that the choreography types compile and work with the adapter
        let device_id = DeviceId::new();
        let _adapter = AuraHandlerAdapterFactory::for_testing(device_id);
        let _endpoint = AuraEndpoint::new(device_id);

        // Test creating choreographic messages and roles
        let _leader_role = ChoreographicRole::coordinator();
        let _follower_role = ChoreographicRole::participant(0);

        let _proposal = Proposal {
            proposal_id: b"test_proposal".to_vec(),
            content: b"test_content".to_vec(),
            proposer: device_id,
            timestamp: 1234567890,
        };

        // For now, just verify it compiles - actual execution would require
        // setting up a full choreographic network with rumpsteak integration
    }
}
