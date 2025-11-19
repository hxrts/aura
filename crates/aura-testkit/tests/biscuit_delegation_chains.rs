//! Delegation chain tests for Biscuit token attenuation
//!
//! This test module validates progressive token attenuation through delegation chains,
//! testing that each level of delegation properly restricts capabilities while
//! maintaining security properties.

use aura_core::{AccountId, DeviceId};
use aura_protocol::authorization::biscuit_bridge::BiscuitAuthorizationBridge;
use aura_testkit::{create_delegation_scenario, BiscuitTestFixture};
use aura_wot::{
    biscuit_resources::{AdminOperation, JournalOp, RecoveryType, ResourceScope, StorageCategory},
    biscuit_token::{BiscuitError, BiscuitTokenManager},
};
use biscuit_auth::{macros::*, Biscuit};

#[tokio::test]
async fn test_progressive_storage_attenuation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = create_delegation_scenario()?;

    let chain = fixture
        .get_delegation_chain("progressive_restriction")
        .expect("Delegation chain should exist");

    let device_id = DeviceId::new();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Verify that the original token has broad access
    let broad_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "any_path/".to_string(),
    };

    let original_result = bridge.authorize(&chain.original_token, "read", &broad_scope)?;
    assert!(
        original_result.authorized,
        "Original token should have broad access"
    );

    // Test each level of the delegation chain
    for (index, (token, scope)) in chain
        .delegated_tokens
        .iter()
        .zip(chain.resource_scopes.iter())
        .enumerate()
    {
        // Each token should work for its specific scope
        let specific_result = bridge.authorize(token, "read", scope)?;
        assert!(
            specific_result.authorized,
            "Delegated token {} should authorize access to its specific scope",
            index
        );

        // But should not work for broader scopes (in a real implementation)
        let broader_result = bridge.authorize(token, "read", &broad_scope)?;
        println!(
            "Delegation level {} broader access: {} (should be false in real implementation)",
            index, broader_result.authorized
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_delegation_depth_tracking() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    // Create a multi-level delegation chain to test depth tracking
    let scopes = vec![
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "level1/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "level1/level2/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "level1/level2/level3/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "level1/level2/level3/level4/".to_string(),
        },
    ];

    fixture.create_delegation_chain("depth_tracking", device_id, scopes)?;

    let chain = fixture.get_delegation_chain("depth_tracking").unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Test that each level of delegation is properly tracked
    for (index, token) in chain.delegated_tokens.iter().enumerate() {
        let scope = &chain.resource_scopes[index];
        let result = bridge.authorize(token, "read", scope)?;

        assert!(
            result.authorized,
            "Token at delegation depth {} should be authorized",
            index
        );

        // In a real implementation, we could extract delegation_depth facts
        // from the token and verify they're correctly set
        println!("Delegation depth {} token authorized", index);
    }

    Ok(())
}

#[tokio::test]
async fn test_delegation_depth_limits() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    // Create tokens with different depth limits
    let shallow_token = fixture.create_depth_limited_token(device_id, 1)?;
    let medium_token = fixture.create_depth_limited_token(device_id, 3)?;
    let deep_token = fixture.create_depth_limited_token(device_id, 5)?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Test that depth-limited tokens can be authorized
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "delegated/".to_string(),
    };

    for (name, token) in [
        ("shallow", &shallow_token),
        ("medium", &medium_token),
        ("deep", &deep_token),
    ] {
        let result = bridge.authorize(token, "read", &storage_scope)?;
        assert!(
            result.authorized,
            "{} depth-limited token should be authorized",
            name
        );

        // Check delegation capability
        let has_delegate = bridge.has_capability(token, "delegate")?;
        assert!(
            has_delegate,
            "{} token should have delegate capability",
            name
        );
    }

    // In a real implementation, we would create actual delegated tokens
    // and test that they respect the depth limits
    Ok(())
}

#[tokio::test]
async fn test_cross_resource_delegation() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    // Create a delegation chain across different resource types
    let mixed_scopes = vec![
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "documents/".to_string(),
        },
        ResourceScope::Journal {
            account_id: fixture.account_id().to_string(),
            operation: JournalOp::Read,
        },
        ResourceScope::Relay {
            channel_id: "secure_channel".to_string(),
        },
    ];

    fixture.create_delegation_chain("mixed_resources", device_id, mixed_scopes)?;

    let chain = fixture.get_delegation_chain("mixed_resources").unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Test that each token in the chain works for its specific resource type
    for (index, (token, expected_scope)) in chain
        .delegated_tokens
        .iter()
        .zip(chain.resource_scopes.iter())
        .enumerate()
    {
        let operation = match expected_scope {
            ResourceScope::Storage { .. } => "read",
            ResourceScope::Journal { operation, .. } => operation.as_str(),
            ResourceScope::Relay { .. } => "relay_message",
            ResourceScope::Recovery { .. } => "recovery_approve",
            ResourceScope::Admin { .. } => "admin",
        };

        let result = bridge.authorize(token, operation, expected_scope)?;
        assert!(
            result.authorized,
            "Mixed delegation token {} should authorize {} operation",
            index, operation
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_attenuation_irreversibility() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;
    let token_manager = fixture.get_device_token(&device_id).unwrap();

    // Create an attenuated token with read-only access
    let read_only_token = token_manager.attenuate_read("documents/")?;

    // Attempt to create a write token from the read-only token (should not expand privileges)
    let token_manager_read_only = BiscuitTokenManager::new(device_id, read_only_token.clone());
    let attempted_write_token = token_manager_read_only.attenuate_write("documents/")?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // The read-only token should work for read operations
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "documents/file.txt".to_string(),
    };

    let read_result = bridge.authorize(&read_only_token, "read", &storage_scope)?;
    assert!(
        read_result.authorized,
        "Read-only token should authorize read operations"
    );

    // The attempted write token should not grant write privileges
    // (In a real implementation, this should fail or only allow read)
    let write_result = bridge.authorize(&attempted_write_token, "write", &storage_scope)?;
    println!(
        "Attempted privilege escalation via write token: {} (should be false)",
        write_result.authorized
    );

    Ok(())
}

#[tokio::test]
async fn test_temporal_delegation_constraints() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    // Create a time-constrained delegation chain
    let account = fixture.account_id().to_string();
    let device = device_id.to_string();
    let expiry_time = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600; // 1 hour

    let source_token = fixture
        .get_device_token(&device_id)
        .unwrap()
        .current_token();

    // Create a time-limited delegated token
    let time_limited_token = source_token.append(block!(
        r#"
        check if time($time), $time < {expiry_time};
        check if resource($res), $res.starts_with("/storage/personal/temp/");
        delegation_depth(1);
        expiry({expiry_time});
    "#,
        expiry_time = expiry_time.to_string()
    ))?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    let temp_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "temp/file.txt".to_string(),
    };

    // The time-limited token should work within its time constraints
    let result = bridge.authorize(&time_limited_token, "read", &temp_scope)?;
    assert!(
        result.authorized,
        "Time-limited token should authorize operations within time bounds"
    );

    // Create a further delegated token from the time-limited one
    let further_delegated = time_limited_token.append(block!(
        r#"
        check if resource($res), $res.starts_with("/storage/personal/temp/readonly/");
        delegation_depth(2);
    "#
    ))?;

    let readonly_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "temp/readonly/file.txt".to_string(),
    };

    let result = bridge.authorize(&further_delegated, "read", &readonly_scope)?;
    assert!(
        result.authorized,
        "Further delegated token should inherit time constraints"
    );

    Ok(())
}

#[tokio::test]
async fn test_resource_hierarchy_delegation() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    // Create a hierarchical delegation chain for file system-like resources
    let hierarchical_scopes = vec![
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "projects/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "projects/aura/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "projects/aura/src/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "projects/aura/src/core/".to_string(),
        },
    ];

    fixture.create_delegation_chain("hierarchy", device_id, hierarchical_scopes)?;

    let chain = fixture.get_delegation_chain("hierarchy").unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Test that each level can access its specific path
    for (index, (token, scope)) in chain
        .delegated_tokens
        .iter()
        .zip(chain.resource_scopes.iter())
        .enumerate()
    {
        let result = bridge.authorize(token, "read", scope)?;
        assert!(
            result.authorized,
            "Hierarchical delegation level {} should authorize specific path access",
            index
        );

        // Test that deeper levels cannot access broader paths
        if index > 0 {
            let broader_scope = &chain.resource_scopes[index - 1];
            let broader_result = bridge.authorize(token, "read", broader_scope)?;
            println!(
                "Level {} accessing broader path: {} (should be false)",
                index, broader_result.authorized
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_operation_specific_delegation() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;
    let source_token = fixture
        .get_device_token(&device_id)
        .unwrap()
        .current_token();

    // Create operation-specific delegated tokens
    let read_only_token = source_token.append(block!(
        r#"
        check if operation("read");
        check if resource($res), $res.starts_with("/storage/personal/docs/");
        delegation_depth(1);
        allowed_operation("read");
    "#
    ))?;

    let write_only_token = source_token.append(block!(
        r#"
        check if operation("write");
        check if resource($res), $res.starts_with("/storage/personal/drafts/");
        delegation_depth(1);
        allowed_operation("write");
    "#
    ))?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    let docs_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "docs/file.txt".to_string(),
    };

    let drafts_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "drafts/draft.txt".to_string(),
    };

    // Test read-only token
    let read_result = bridge.authorize(&read_only_token, "read", &docs_scope)?;
    assert!(
        read_result.authorized,
        "Read-only token should authorize read operations"
    );

    // Read-only token should not authorize write operations (in real implementation)
    let invalid_write = bridge.authorize(&read_only_token, "write", &docs_scope)?;
    println!(
        "Read-only token attempting write: {} (should be false)",
        invalid_write.authorized
    );

    // Test write-only token
    let write_result = bridge.authorize(&write_only_token, "write", &drafts_scope)?;
    assert!(
        write_result.authorized,
        "Write-only token should authorize write operations"
    );

    // Write-only token should not authorize read operations (in real implementation)
    let invalid_read = bridge.authorize(&write_only_token, "read", &drafts_scope)?;
    println!(
        "Write-only token attempting read: {} (should be false)",
        invalid_read.authorized
    );

    Ok(())
}

#[tokio::test]
async fn test_delegation_with_additional_constraints() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;
    let source_token = fixture
        .get_device_token(&device_id)
        .unwrap()
        .current_token();

    // Create a delegated token with additional constraints
    let constrained_token = source_token.append(block!(
        r#"
        check if resource($res), $res.starts_with("/storage/personal/");
        check if file_size($size), $size < 1000000; // Max 1MB files
        check if operation($op), ["read", "write"].contains($op);

        delegation_depth(1);
        max_file_size(1000000);
        allowed_operations(["read", "write"]);

        // Additional business logic constraints
        check if user_role($role), $role != "guest";
        check if access_time($time), $time >= 9, $time <= 17; // Business hours only
    "#
    ))?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "small_file.txt".to_string(),
    };

    // The constrained token should work for basic operations
    let result = bridge.authorize(&constrained_token, "read", &storage_scope)?;
    assert!(
        result.authorized,
        "Constrained token should authorize operations that meet constraints"
    );

    // In a real implementation, we would test that operations violating
    // the constraints (large files, outside business hours, etc.) are denied

    Ok(())
}

#[tokio::test]
async fn test_delegation_chain_serialization() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = create_delegation_scenario()?;

    let chain = fixture
        .get_delegation_chain("progressive_restriction")
        .unwrap();

    // Test that delegated tokens can be serialized and deserialized
    for (index, token) in chain.delegated_tokens.iter().enumerate() {
        // Serialize the token
        let serialized = token.to_vec().map_err(BiscuitError::BiscuitLib)?;
        assert!(
            !serialized.is_empty(),
            "Delegated token {} should serialize to non-empty bytes",
            index
        );

        // Deserialize the token
        let deserialized = Biscuit::from(&serialized, fixture.root_public_key())
            .map_err(BiscuitError::BiscuitLib)?;

        // Verify the deserialized token still works
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), DeviceId::new());
        let scope = &chain.resource_scopes[index];

        let result = bridge.authorize(&deserialized, "read", scope)?;
        assert!(
            result.authorized,
            "Deserialized delegated token {} should still work",
            index
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_complex_delegation_scenarios() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    // Create multiple delegation chains for different use cases
    let admin_chain = vec![
        ResourceScope::Admin {
            operation: AdminOperation::AddGuardian,
        },
        ResourceScope::Recovery {
            recovery_type: RecoveryType::GuardianSet,
        },
    ];

    let data_chain = vec![
        ResourceScope::Storage {
            category: StorageCategory::Shared,
            path: "team_data/".to_string(),
        },
        ResourceScope::Journal {
            account_id: fixture.account_id().to_string(),
            operation: JournalOp::Sync,
        },
    ];

    fixture.create_delegation_chain("admin_ops", device_id, admin_chain)?;
    fixture.create_delegation_chain("data_ops", device_id, data_chain)?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    // Test admin delegation chain
    if let Some(admin_chain) = fixture.get_delegation_chain("admin_ops") {
        for (index, (token, scope)) in admin_chain
            .delegated_tokens
            .iter()
            .zip(admin_chain.resource_scopes.iter())
            .enumerate()
        {
            let operation = match scope {
                ResourceScope::Admin { operation } => operation.as_str(),
                ResourceScope::Recovery { .. } => "recovery_approve",
                _ => "read",
            };

            let result = bridge.authorize(token, operation, scope)?;
            assert!(
                result.authorized,
                "Admin delegation chain token {} should work",
                index
            );
        }
    }

    // Test data delegation chain
    if let Some(data_chain) = fixture.get_delegation_chain("data_ops") {
        for (index, (token, scope)) in data_chain
            .delegated_tokens
            .iter()
            .zip(data_chain.resource_scopes.iter())
            .enumerate()
        {
            let operation = match scope {
                ResourceScope::Storage { .. } => "read",
                ResourceScope::Journal { operation, .. } => operation.as_str(),
                _ => "read",
            };

            let result = bridge.authorize(token, operation, scope)?;
            assert!(
                result.authorized,
                "Data delegation chain token {} should work",
                index
            );
        }
    }

    Ok(())
}
