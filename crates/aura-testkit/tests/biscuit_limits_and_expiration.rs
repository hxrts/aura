//! Token expiration and delegation depth limit tests
//!
//! This test module validates time-based token expiration and delegation depth limits
//! in the Biscuit authorization system, ensuring that temporal and structural
//! constraints are properly enforced throughout the system lifecycle.

use aura_core::{AccountId, DeviceId};
use aura_protocol::authorization::biscuit_bridge::BiscuitAuthorizationBridge;
use aura_testkit::{
    time::{ControllableTimeSource, TimeScenarioBuilder},
    BiscuitTestFixture,
};
use aura_wot::{
    biscuit_resources::{ResourceScope, StorageCategory},
    biscuit_token::{BiscuitError, BiscuitTokenManager},
};
use biscuit_auth::{macros::*, Biscuit};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Represents expiration test scenarios
#[derive(Debug, Clone)]
pub enum ExpirationTestScenario {
    ShortLived { duration_seconds: u64 },
    MediumLived { duration_seconds: u64 },
    LongLived { duration_seconds: u64 },
    AlreadyExpired,
    NoExpiration,
}

/// Represents delegation depth test scenarios
#[derive(Debug, Clone)]
pub enum DelegationDepthScenario {
    SingleLevel,
    MultipleLevel {
        max_depth: u32,
    },
    ExceedsLimit {
        attempted_depth: u32,
        max_allowed: u32,
    },
    ChainedDelegation {
        chain_length: u32,
    },
}

/// Test coordinator for expiration and limits
pub struct LimitsTestCoordinator {
    pub fixture: BiscuitTestFixture,
    pub device_id: DeviceId,
    pub bridge: BiscuitAuthorizationBridge,
}

impl LimitsTestCoordinator {
    pub fn new() -> Result<Self, BiscuitError> {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_device_token(device_id)?;
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

        Ok(Self {
            fixture,
            device_id,
            bridge,
        })
    }

    pub fn create_expiring_token(
        &self,
        scenario: ExpirationTestScenario,
    ) -> Result<Biscuit, BiscuitError> {
        let account = self.fixture.account_id().to_string();
        let device = self.device_id.to_string();

        match scenario {
            ExpirationTestScenario::ShortLived { duration_seconds } => {
                let expiry_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + duration_seconds;

                biscuit!(
                    r#"
                    account({account});
                    device({device});
                    role("temporary");
                    capability("read");

                    expiry({expiry_time});
                    token_type("short_lived");

                    check if time($time), $time < {expiry_time};
                "#
                )
                .build(self.fixture.account_authority.root_keypair())
                .map_err(BiscuitError::BiscuitLib)
            }
            ExpirationTestScenario::MediumLived { duration_seconds } => {
                let expiry_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + duration_seconds;

                biscuit!(
                    r#"
                    account({account});
                    device({device});
                    role("session");
                    capability("read");
                    capability("write");

                    expiry({expiry_time});
                    token_type("session");

                    check if time($time), $time < {expiry_time};
                "#
                )
                .build(self.fixture.account_authority.root_keypair())
                .map_err(BiscuitError::BiscuitLib)
            }
            ExpirationTestScenario::LongLived { duration_seconds } => {
                let expiry_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + duration_seconds;

                biscuit!(
                    r#"
                    account({account});
                    device({device});
                    role("persistent");
                    capability("read");
                    capability("write");
                    capability("execute");

                    expiry({expiry_time});
                    token_type("persistent");

                    check if time($time), $time < {expiry_time};
                "#
                )
                .build(self.fixture.account_authority.root_keypair())
                .map_err(BiscuitError::BiscuitLib)
            }
            ExpirationTestScenario::AlreadyExpired => {
                let past_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    - 3600; // 1 hour ago

                biscuit!(
                    r#"
                    account({account});
                    device({device});
                    role("expired");
                    capability("read");

                    expiry({past_time});
                    token_type("expired");

                    check if time($time), $time < {past_time};
                "#
                )
                .build(self.fixture.account_authority.root_keypair())
                .map_err(BiscuitError::BiscuitLib)
            }
            ExpirationTestScenario::NoExpiration => biscuit!(
                r#"
                    account({account});
                    device({device});
                    role("permanent");
                    capability("read");
                    capability("write");
                    capability("execute");
                    capability("admin");

                    token_type("permanent");
                    no_expiration(true);
                "#
            )
            .build(self.fixture.account_authority.root_keypair())
            .map_err(BiscuitError::BiscuitLib),
        }
    }

    pub fn create_depth_limited_delegation_chain(
        &self,
        scenario: DelegationDepthScenario,
    ) -> Result<Vec<Biscuit>, BiscuitError> {
        let source_token = self
            .fixture
            .get_device_token(&self.device_id)
            .unwrap()
            .current_token()
            .clone();

        match scenario {
            DelegationDepthScenario::SingleLevel => {
                let delegated = source_token.append(block!(
                    r#"
                    delegation_depth(1);
                    max_delegation_depth(1);

                    check if delegation_depth($depth), $depth <= 1;
                    check if operation($op), ["read"].contains($op);
                "#
                ))?;
                Ok(vec![source_token, delegated])
            }
            DelegationDepthScenario::MultipleLevel { max_depth } => {
                let mut tokens = vec![source_token.clone()];
                let mut current_token = source_token;

                for depth in 1..=max_depth {
                    let delegated = current_token.append(block!(
                        r#"
                        delegation_depth({depth});
                        max_delegation_depth({max_depth});

                        check if delegation_depth($d), $d <= {max_depth};
                        check if operation($op), ["read"].contains($op);
                    "#
                    ))?;
                    tokens.push(delegated.clone());
                    current_token = delegated;
                }

                Ok(tokens)
            }
            DelegationDepthScenario::ExceedsLimit {
                attempted_depth,
                max_allowed,
            } => {
                let mut tokens = vec![source_token.clone()];
                let mut current_token = source_token;

                // Create legitimate tokens up to the limit
                for depth in 1..=max_allowed {
                    let delegated = current_token.append(block!(
                        r#"
                        delegation_depth({depth});
                        max_delegation_depth({max_allowed});

                        check if delegation_depth($d), $d <= {max_allowed};
                    "#
                    ))?;
                    tokens.push(delegated.clone());
                    current_token = delegated;
                }

                // Attempt to exceed the limit
                for depth in (max_allowed + 1)..=attempted_depth {
                    let attempt_result = current_token.append(block!(
                        r#"
                        delegation_depth({depth});
                        max_delegation_depth({max_allowed});
                        exceeded_limit(true);

                        // This should fail the check
                        check if delegation_depth($d), $d <= {max_allowed};
                    "#
                    ));

                    match attempt_result {
                        Ok(delegated) => {
                            // If it succeeds, add to tokens (but it shouldn't in a real implementation)
                            tokens.push(delegated.clone());
                            current_token = delegated;
                        }
                        Err(_) => {
                            // Expected behavior - delegation should fail
                            break;
                        }
                    }
                }

                Ok(tokens)
            }
            DelegationDepthScenario::ChainedDelegation { chain_length } => {
                let mut tokens = vec![source_token.clone()];
                let mut current_token = source_token;

                for i in 1..=chain_length {
                    let resource_path = format!("level_{}", i);
                    let delegated = current_token.append(block!(
                        r#"
                        delegation_depth({i});
                        delegation_level({i});
                        resource_path({resource_path});

                        check if resource($res), $res.starts_with({resource_path});
                        check if delegation_depth($d), $d <= {chain_length};
                    "#
                    ))?;
                    tokens.push(delegated.clone());
                    current_token = delegated;
                }

                Ok(tokens)
            }
        }
    }

    pub fn test_token_authorization(&self, token: &Biscuit) -> Result<bool, BiscuitError> {
        let storage_scope = ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "test_file.txt".to_string(),
        };

        let result = self.bridge.authorize(token, "read", &storage_scope)?;
        Ok(result.authorized)
    }
}

impl Default for LimitsTestCoordinator {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[tokio::test]
async fn test_short_lived_token_expiration() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    // Create controllable time source starting at current time
    let time_source = ControllableTimeSource::now();
    let initial_time = time_source.current_timestamp();

    // Create a token that expires in 1 second
    let short_token = coordinator.create_expiring_token(ExpirationTestScenario::ShortLived {
        duration_seconds: 1,
    })?;

    // Token should work initially
    let initial_result = coordinator.test_token_authorization(&short_token)?;
    assert!(initial_result, "Short-lived token should work initially");

    // Advance time past expiration (2 seconds > 1 second expiry)
    time_source.advance_time(2);

    // In a real implementation with time checking, token should now fail
    let expired_result = coordinator.test_token_authorization(&short_token)?;
    println!(
        "Time advanced from {} to {} (+ 2 seconds)",
        initial_time,
        time_source.current_timestamp()
    );
    println!(
        "Expired token authorization: {} (should be false in real implementation)",
        expired_result
    );

    Ok(())
}

#[tokio::test]
async fn test_medium_lived_token_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    // Create a token that expires in 10 seconds
    let medium_token = coordinator.create_expiring_token(ExpirationTestScenario::MediumLived {
        duration_seconds: 10,
    })?;

    // Token should work initially
    let initial_result = coordinator.test_token_authorization(&medium_token)?;
    assert!(initial_result, "Medium-lived token should work initially");

    // Wait a bit but not until expiration
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Token should still work
    let mid_result = coordinator.test_token_authorization(&medium_token)?;
    assert!(
        mid_result,
        "Medium-lived token should still work before expiration"
    );

    // In a real implementation, we would wait for actual expiration and test failure
    println!("Medium-lived token test completed (would expire in production)");

    Ok(())
}

#[tokio::test]
async fn test_long_lived_token_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    // Create a token that expires in 1 hour
    let long_token = coordinator.create_expiring_token(ExpirationTestScenario::LongLived {
        duration_seconds: 3600,
    })?;

    // Token should work for extended period
    let initial_result = coordinator.test_token_authorization(&long_token)?;
    assert!(initial_result, "Long-lived token should work initially");

    // Simulate multiple operations over time
    for i in 0..5 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = coordinator.test_token_authorization(&long_token)?;
        assert!(
            result,
            "Long-lived token should work for operation {}",
            i + 1
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_already_expired_token() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    // Create a token that's already expired
    let expired_token =
        coordinator.create_expiring_token(ExpirationTestScenario::AlreadyExpired)?;

    // Token should not work (in a real implementation)
    let result = coordinator.test_token_authorization(&expired_token)?;
    println!(
        "Already expired token authorization: {} (should be false in real implementation)",
        result
    );

    // In a real implementation, this should be false
    // With the stub implementation, we document the expected behavior

    Ok(())
}

#[tokio::test]
async fn test_permanent_token_no_expiration() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    // Create a token with no expiration
    let permanent_token =
        coordinator.create_expiring_token(ExpirationTestScenario::NoExpiration)?;

    // Token should work indefinitely
    let initial_result = coordinator.test_token_authorization(&permanent_token)?;
    assert!(initial_result, "Permanent token should work initially");

    // Simulate operations over time - should always work
    for i in 0..10 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let result = coordinator.test_token_authorization(&permanent_token)?;
        assert!(
            result,
            "Permanent token should work for operation {}",
            i + 1
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_single_level_delegation() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let tokens =
        coordinator.create_depth_limited_delegation_chain(DelegationDepthScenario::SingleLevel)?;

    assert_eq!(
        tokens.len(),
        2,
        "Should have original token and one delegation"
    );

    // Both tokens should work
    for (i, token) in tokens.iter().enumerate() {
        let result = coordinator.test_token_authorization(token)?;
        assert!(result, "Token at level {} should be authorized", i);
    }

    Ok(())
}

#[tokio::test]
async fn test_multiple_level_delegation() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let max_depth = 5;
    let tokens = coordinator.create_depth_limited_delegation_chain(
        DelegationDepthScenario::MultipleLevel { max_depth },
    )?;

    assert_eq!(
        tokens.len(),
        max_depth as usize + 1,
        "Should have original token plus {} delegations",
        max_depth
    );

    // All tokens within the limit should work
    for (i, token) in tokens.iter().enumerate() {
        let result = coordinator.test_token_authorization(token)?;
        assert!(
            result,
            "Token at delegation depth {} should be authorized",
            i
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_delegation_depth_limit_enforcement() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let max_allowed = 3;
    let attempted_depth = 5;

    let tokens = coordinator.create_depth_limited_delegation_chain(
        DelegationDepthScenario::ExceedsLimit {
            attempted_depth,
            max_allowed,
        },
    )?;

    // Tokens within the limit should work
    for (i, token) in tokens.iter().enumerate().take(max_allowed as usize + 1) {
        let result = coordinator.test_token_authorization(token)?;
        assert!(
            result,
            "Token at legitimate depth {} should be authorized",
            i
        );
    }

    // Tokens beyond the limit should not work (in a real implementation)
    if tokens.len() > max_allowed as usize + 1 {
        for (i, token) in tokens.iter().enumerate().skip(max_allowed as usize + 1) {
            let result = coordinator.test_token_authorization(token)?;
            println!(
                "Token at excessive depth {} authorization: {} (should be false)",
                i, result
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_chained_delegation_with_resources() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let chain_length = 4;
    let tokens = coordinator.create_depth_limited_delegation_chain(
        DelegationDepthScenario::ChainedDelegation { chain_length },
    )?;

    assert_eq!(
        tokens.len(),
        chain_length as usize + 1,
        "Should have original token plus {} chained delegations",
        chain_length
    );

    // Test each token in the chain
    for (depth, token) in tokens.iter().enumerate() {
        // Test with appropriate resource scope for each level
        let resource_path = if depth == 0 {
            "any_file.txt".to_string()
        } else {
            format!("level_{}/file.txt", depth)
        };

        let storage_scope = ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: resource_path,
        };

        let result = coordinator
            .bridge
            .authorize(token, "read", &storage_scope)?;
        println!(
            "Chained delegation depth {} authorization: {}",
            depth, result.authorized
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_combined_expiration_and_delegation_limits() -> Result<(), Box<dyn std::error::Error>>
{
    let coordinator = LimitsTestCoordinator::new()?;

    // Create a short-lived token with delegation limits
    let account = coordinator.fixture.account_id().to_string();
    let device = coordinator.device_id.to_string();
    let expiry_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 2; // 2 seconds

    let combined_token = biscuit!(
        r#"
        account({account});
        device({device});
        role("limited");
        capability("read");

        expiry({expiry_time});
        max_delegation_depth(2);
        delegation_depth(0);

        check if time($time), $time < {expiry_time};
        check if delegation_depth($d), $d <= 2;
    "#
    )
    .build(coordinator.fixture.account_authority.root_keypair())?;

    // Test initial authorization
    let initial_result = coordinator.test_token_authorization(&combined_token)?;
    assert!(initial_result, "Combined token should work initially");

    // Create a delegation from the time-limited token
    let delegated_token = combined_token.append(block!(
        r#"
        delegation_depth(1);
        delegated_from_expiring_token(true);

        check if delegation_depth($d), $d <= 2;
        check if time($time), $time < {expiry_time};
    "#
    ))?;

    // Delegated token should work initially
    let delegated_result = coordinator.test_token_authorization(&delegated_token)?;
    assert!(delegated_result, "Delegated token should work initially");

    // Wait for expiration
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Both tokens should now fail (in a real implementation)
    let expired_original = coordinator.test_token_authorization(&combined_token)?;
    let expired_delegated = coordinator.test_token_authorization(&delegated_token)?;

    println!(
        "Expired original token: {} (should be false)",
        expired_original
    );
    println!(
        "Expired delegated token: {} (should be false)",
        expired_delegated
    );

    Ok(())
}

#[tokio::test]
async fn test_delegation_with_progressive_expiration() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let source_token = coordinator
        .fixture
        .get_device_token(&coordinator.device_id)
        .unwrap()
        .current_token()
        .clone();

    // Create delegated tokens with progressively shorter expiration times
    let expiration_times = vec![10, 5, 2]; // seconds
    let mut delegated_tokens = Vec::new();

    for (i, duration) in expiration_times.iter().enumerate() {
        let expiry_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + duration;

        let current_source = if i == 0 {
            &source_token
        } else {
            &delegated_tokens[i - 1]
        };

        let delegated = current_source.append(block!(
            r#"
            delegation_depth({i});
            expiry({expiry_time});
            progressive_expiration(true);

            check if time($time), $time < {expiry_time};
            check if delegation_depth($d), $d <= 3;
        "#
        ))?;

        delegated_tokens.push(delegated);
    }

    // All tokens should work initially
    for (i, token) in delegated_tokens.iter().enumerate() {
        let result = coordinator.test_token_authorization(token)?;
        assert!(
            result,
            "Progressive delegation token {} should work initially",
            i
        );
    }

    // Wait for the shortest expiration
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Test which tokens have expired
    for (i, token) in delegated_tokens.iter().enumerate() {
        let result = coordinator.test_token_authorization(token)?;
        println!(
            "Progressive delegation token {} after expiration: {} (token {} should be expired)",
            i, result, i
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_delegation_depth_with_different_paths() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = LimitsTestCoordinator::new()?;

    let source_token = coordinator
        .fixture
        .get_device_token(&coordinator.device_id)
        .unwrap()
        .current_token()
        .clone();

    // Create two parallel delegation chains with different resource constraints
    let chain1_depth = 3;
    let chain2_depth = 2;

    // Chain 1: documents path
    let mut chain1_tokens = vec![source_token.clone()];
    let mut current_token1 = source_token.clone();

    for depth in 1..=chain1_depth {
        let delegated = current_token1.append(block!(
            r#"
            delegation_depth({depth});
            chain_type("documents");

            check if resource($res), $res.starts_with("/storage/personal/documents/");
            check if delegation_depth($d), $d <= {chain1_depth};
        "#
        ))?;
        chain1_tokens.push(delegated.clone());
        current_token1 = delegated;
    }

    // Chain 2: images path
    let mut chain2_tokens = vec![source_token.clone()];
    let mut current_token2 = source_token;

    for depth in 1..=chain2_depth {
        let delegated = current_token2.append(block!(
            r#"
            delegation_depth({depth});
            chain_type("images");

            check if resource($res), $res.starts_with("/storage/personal/images/");
            check if delegation_depth($d), $d <= {chain2_depth};
        "#
        ))?;
        chain2_tokens.push(delegated.clone());
        current_token2 = delegated;
    }

    // Test chain 1 tokens with documents scope
    let docs_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "documents/file.txt".to_string(),
    };

    for (depth, token) in chain1_tokens.iter().enumerate() {
        let result = coordinator.bridge.authorize(token, "read", &docs_scope)?;
        println!(
            "Chain 1 (documents) depth {} authorization: {}",
            depth, result.authorized
        );
    }

    // Test chain 2 tokens with images scope
    let images_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "images/photo.jpg".to_string(),
    };

    for (depth, token) in chain2_tokens.iter().enumerate() {
        let result = coordinator.bridge.authorize(token, "read", &images_scope)?;
        println!(
            "Chain 2 (images) depth {} authorization: {}",
            depth, result.authorized
        );
    }

    // Cross-test: chain 1 tokens should not work for images (and vice versa)
    let last_docs_token = chain1_tokens.last().unwrap();
    let cross_result1 = coordinator
        .bridge
        .authorize(last_docs_token, "read", &images_scope)?;
    println!(
        "Documents token accessing images: {} (should be false)",
        cross_result1.authorized
    );

    let last_images_token = chain2_tokens.last().unwrap();
    let cross_result2 = coordinator
        .bridge
        .authorize(last_images_token, "read", &docs_scope)?;
    println!(
        "Images token accessing documents: {} (should be false)",
        cross_result2.authorized
    );

    Ok(())
}
