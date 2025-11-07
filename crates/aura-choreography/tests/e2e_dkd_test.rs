//! Basic test for the DKD choreography implementation

use aura_choreography::protocols::dkd::{execute_dkd, DkdConfig};
use aura_protocol::AuraEffectSystem;
use aura_types::DeviceId;

#[tokio::test]
async fn test_basic_dkd_execution() {
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    
    let config = DkdConfig {
        participants: vec![device1, device2],
        threshold: 2,
        app_id: "test_app".to_string(),
        context: "test_context".to_string(),
        derivation_path: vec![0, 1, 2],
    };

    let mut effect_system = AuraEffectSystem::for_testing(device1);
    
    let result = execute_dkd(&mut effect_system, config).await;
    
    assert!(result.is_ok());
    let dkd_result = result.unwrap();
    assert!(dkd_result.success);
    assert_eq!(dkd_result.derived_keys.len(), 2);
}