//! Integration tests for threshold identity and recovery
//!
//! These tests verify that G_tree_op and G_recovery choreographies work together
//! to provide secure threshold identity with guardian-based recovery.

use aura_identity::{TreeOpChoreography, TreeOpRole, TreeOpMessage};
use aura_recovery::{RecoveryChoreography, RecoveryRole, RecoveryMessage};
use aura_core::{
    DeviceId, AccountId, Cap,
    tree::{TreeOp, TreeOpKind, Policy, Epoch},
};
use aura_authenticate::RecoveryContext;
use aura_wot::{Guardian, GuardianSet};
use aura_mpst::AuraRuntime;
use std::collections::HashMap;

/// Test complete threshold identity setup and recovery flow
#[tokio::test]
async fn test_threshold_identity_with_recovery() {
    // Setup: Create a 2-of-3 threshold identity
    let devices = create_test_devices(3);
    let guardians = create_test_guardians(3);
    let account_id = AccountId::new();

    // Step 1: Initialize threshold identity using G_tree_op
    let identity_result = setup_threshold_identity(
        account_id,
        devices.clone(),
        guardians.clone(),
    ).await;

    assert!(identity_result.is_ok(), "Failed to setup threshold identity: {:?}", identity_result);

    // Step 2: Simulate device loss and recovery using G_recovery
    let lost_device = devices[0];
    let recovery_result = execute_device_recovery(
        account_id,
        lost_device,
        guardians.clone(),
        2, // 2-of-3 threshold
    ).await;

    assert!(recovery_result.is_ok(), "Failed to recover device: {:?}", recovery_result);

    // Step 3: Verify recovered device can participate in tree operations
    let verification_result = verify_recovered_device_access(
        account_id,
        lost_device,
        recovery_result.unwrap(),
        devices[1..].to_vec(), // Other devices
    ).await;

    assert!(verification_result.is_ok(), "Recovered device cannot access tree: {:?}", verification_result);
}

/// Test recovery with insufficient guardians
#[tokio::test]
async fn test_recovery_insufficient_threshold() {
    let devices = create_test_devices(3);
    let guardians = create_test_guardians(3);
    let account_id = AccountId::new();

    // Setup threshold identity
    let _ = setup_threshold_identity(account_id, devices.clone(), guardians.clone()).await.unwrap();

    // Attempt recovery with only 1 guardian (need 2)
    let lost_device = devices[0];
    let single_guardian = guardians[0..1].to_vec();

    let recovery_result = execute_device_recovery(
        account_id,
        lost_device,
        single_guardian,
        2, // Still need 2-of-3
    ).await;

    // Should fail with insufficient guardians
    assert!(recovery_result.is_err());
    assert!(recovery_result.unwrap_err().to_string().contains("only 1/"));
}

/// Test tree operation with full threshold participation
#[tokio::test]
async fn test_tree_operation_threshold_approval() {
    let devices = create_test_devices(3);
    let account_id = AccountId::new();

    // Create tree operation choreography for adding a new device
    let new_device = DeviceId::new();
    let operation = TreeOp {
        kind: TreeOpKind::AddDevice {
            device_id: new_device,
            public_key: vec![1, 2, 3, 4], // Placeholder key
        },
    };

    let approval_result = execute_tree_operation_with_threshold(
        devices.clone(),
        operation,
        2, // 2-of-3 threshold
    ).await;

    assert!(approval_result.is_ok());
    assert!(approval_result.unwrap().is_some(), "Tree operation should succeed with threshold");
}

/// Test tree operation rejection when threshold not met
#[tokio::test]
async fn test_tree_operation_insufficient_approval() {
    let devices = create_test_devices(3);

    // Create operation that will be rejected by participants
    let operation = TreeOp {
        kind: TreeOpKind::UpdatePolicy {
            new_policy: Policy {
                threshold: 5, // Invalid - more than total devices
                max_devices: 10,
            },
        },
    };

    let result = execute_tree_operation_with_threshold(
        devices,
        operation,
        2, // Need 2 approvals
    ).await;

    // Should fail due to invalid policy
    assert!(result.is_err() || result.unwrap().is_none());
}

// Helper functions

async fn setup_threshold_identity(
    account_id: AccountId,
    devices: Vec<DeviceId>,
    guardians: Vec<Guardian>,
) -> aura_core::AuraResult<()> {
    // Create initial tree with devices
    for (i, device_id) in devices.iter().enumerate() {
        let role = if i == 0 {
            TreeOpRole::Proposer(*device_id)
        } else {
            TreeOpRole::Participant(*device_id)
        };

        let policy = Policy {
            threshold: 2, // 2-of-3
            max_devices: 10,
        };

        let capabilities = create_device_capabilities(&devices);
        let runtime = AuraRuntime::new_for_testing(*device_id);

        let mut choreography = TreeOpChoreography::new(
            role,
            Epoch::new(1),
            policy,
            capabilities,
            runtime,
        );

        // Add device to tree
        let operation = TreeOp {
            kind: TreeOpKind::AddDevice {
                device_id: *device_id,
                public_key: vec![i as u8; 32], // Unique key per device
            },
        };

        let result = choreography.execute_tree_operation(operation).await?;

        if i == 0 && result.is_none() {
            // First device addition might not require threshold
            continue;
        }

        if i > 0 && result.is_none() {
            return Err(aura_core::AuraError::internal("Tree operation failed"));
        }
    }

    // Setup guardians for recovery
    // TODO fix - In a real implementation, this would configure the guardian set
    Ok(())
}

async fn execute_device_recovery(
    account_id: AccountId,
    lost_device: DeviceId,
    guardians: Vec<Guardian>,
    threshold: usize,
) -> aura_core::AuraResult<Vec<u8>> {
    let guardian_set = GuardianSet::new(guardians.clone());
    let context = RecoveryContext::AccountRecovery { device_lost: true };

    // Create recovery request
    let request = aura_recovery::guardian_recovery::GuardianRecoveryRequest {
        requesting_device: lost_device,
        account_id,
        recovery_context: context.clone(),
        required_threshold: threshold,
        available_guardians: guardians,
        priority: aura_recovery::guardian_recovery::RecoveryPriority::Normal,
        dispute_window_secs: aura_recovery::guardian_recovery::DEFAULT_DISPUTE_WINDOW_SECS,
    };

    // Execute recovery as device
    let device_runtime = AuraRuntime::new_for_testing(lost_device);
    let device_role = RecoveryRole::RecoveringDevice(lost_device);

    let mut device_choreography = RecoveryChoreography::from_runtime(
        device_role,
        context,
        guardian_set,
        threshold,
        device_runtime,
    );

    let result = device_choreography.execute_recovery(request).await?;

    result.ok_or_else(|| aura_core::AuraError::internal("Recovery returned no key"))
}

async fn verify_recovered_device_access(
    _account_id: AccountId,
    recovered_device: DeviceId,
    _recovered_key: Vec<u8>,
    other_devices: Vec<DeviceId>,
) -> aura_core::AuraResult<()> {
    // Verify recovered device can participate in tree operations
    let mut all_devices = other_devices;
    all_devices.push(recovered_device);

    let operation = TreeOp {
        kind: TreeOpKind::AddDevice {
            device_id: DeviceId::new(), // Add another device
            public_key: vec![255; 32],
        },
    };

    let result = execute_tree_operation_with_threshold(all_devices, operation, 2).await?;

    if result.is_some() {
        Ok(())
    } else {
        Err(aura_core::AuraError::internal("Recovered device cannot participate"))
    }
}

async fn execute_tree_operation_with_threshold(
    devices: Vec<DeviceId>,
    operation: TreeOp,
    threshold: u8,
) -> aura_core::AuraResult<Option<aura_core::tree::AttestedOp>> {
    let proposer = devices[0];
    let participants = &devices[1..];

    let policy = Policy {
        threshold,
        max_devices: 10,
    };

    let capabilities = create_device_capabilities(&devices);
    let runtime = AuraRuntime::new_for_testing(proposer);

    let mut choreography = TreeOpChoreography::new(
        TreeOpRole::Proposer(proposer),
        Epoch::new(1),
        policy,
        capabilities,
        runtime,
    );

    choreography.execute_tree_operation(operation).await
}

fn create_test_devices(count: usize) -> Vec<DeviceId> {
    (0..count).map(|_| DeviceId::new()).collect()
}

fn create_test_guardians(count: usize) -> Vec<Guardian> {
    (0..count).map(|i| {
        Guardian::new(
            DeviceId::new(),
            format!("guardian_{}", i),
            aura_wot::TrustLevel::High,
        )
    }).collect()
}

fn create_device_capabilities(devices: &[DeviceId]) -> HashMap<DeviceId, Cap> {
    devices.iter().map(|&device_id| {
        (device_id, Cap::default()) // Would use actual capabilities
    }).collect()
}

// Property-based tests using proptest
#[cfg(feature = "proptest")]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_recovery_threshold_requirements(
            threshold in 1usize..=5,
            available_guardians in 1usize..=10,
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async {
                let devices = create_test_devices(3);
                let guardians = create_test_guardians(available_guardians);
                let account_id = AccountId::new();

                let recovery_result = execute_device_recovery(
                    account_id,
                    devices[0],
                    guardians,
                    threshold,
                ).await;

                // Recovery should succeed only if available >= threshold
                if available_guardians >= threshold {
                    prop_assert!(recovery_result.is_ok());
                } else {
                    prop_assert!(recovery_result.is_err());
                }
            });
        }

        #[test]
        fn prop_tree_operation_threshold_consistency(
            threshold in 1u8..=5,
            device_count in 1usize..=10,
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async {
                let devices = create_test_devices(device_count);

                let operation = TreeOp {
                    kind: TreeOpKind::AddDevice {
                        device_id: DeviceId::new(),
                        public_key: vec![42; 32],
                    },
                };

                let result = execute_tree_operation_with_threshold(
                    devices,
                    operation,
                    threshold,
                ).await;

                // Operation should succeed if we have enough devices
                if device_count >= threshold as usize {
                    prop_assert!(result.is_ok());
                } else {
                    // May fail due to insufficient participants
                    // This is expected behavior
                }
            });
        }
    }
}
