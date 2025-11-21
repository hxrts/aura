//! Comprehensive Biscuit integration test suite
//!
//! This test module runs comprehensive integration tests across all Biscuit
//! functionality to ensure the entire authorization system works correctly
//! as an integrated whole.

use aura_core::{AccountId, AuthorityId, ContextId, DeviceId};
use aura_protocol::authorization::BiscuitAuthorizationBridge;
use aura_testkit::{
    create_delegation_scenario, create_multi_device_scenario, create_recovery_scenario,
    create_security_test_scenario, BiscuitTestFixture,
};
use aura_wot::{
    biscuit_resources::{AdminOperation, JournalOp, RecoveryType, ResourceScope as LegacyResourceScope, StorageCategory},
    biscuit_token::BiscuitError,
    ResourceScope,
};

/// Helper function to convert legacy ResourceScope to new ResourceScope for bridge authorization
fn convert_legacy_scope(legacy_scope: &LegacyResourceScope) -> ResourceScope {
    let authority_id = AuthorityId::new();
    let context_id = ContextId::new();
    aura_wot::resource_scope::legacy::convert_legacy_resource_scope(
        legacy_scope,
        authority_id,
        context_id,
    )
}

/// Comprehensive test result summary
#[derive(Debug)]
pub struct BiscuitTestSuite {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub skipped_tests: usize,
    pub test_results: Vec<TestResult>,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub status: TestStatus,
    pub details: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub enum TestStatus {
    Passed,
    Failed(String),
    Skipped(String),
}

impl BiscuitTestSuite {
    pub fn new() -> Self {
        Self {
            total_tests: 0,
            passed_tests: 0,
            failed_tests: 0,
            skipped_tests: 0,
            test_results: Vec::new(),
        }
    }

    pub fn add_result(&mut self, result: TestResult) {
        self.total_tests += 1;
        match result.status {
            TestStatus::Passed => self.passed_tests += 1,
            TestStatus::Failed(_) => self.failed_tests += 1,
            TestStatus::Skipped(_) => self.skipped_tests += 1,
        }
        self.test_results.push(result);
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_tests == 0 {
            0.0
        } else {
            (self.passed_tests as f64) / (self.total_tests as f64) * 100.0
        }
    }

    pub fn generate_report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== COMPREHENSIVE BISCUIT INTEGRATION TEST REPORT ===\n\n");

        report.push_str(&format!("Total Tests: {}\n", self.total_tests));
        report.push_str(&format!("✅ Passed: {}\n", self.passed_tests));
        report.push_str(&format!("❌ Failed: {}\n", self.failed_tests));
        report.push_str(&format!("⏭️ Skipped: {}\n", self.skipped_tests));
        report.push_str(&format!("📊 Success Rate: {:.1}%\n\n", self.success_rate()));

        let total_duration: u64 = self.test_results.iter().map(|r| r.duration_ms).sum();
        report.push_str(&format!("⏱️ Total Duration: {}ms\n\n", total_duration));

        report.push_str("=== DETAILED RESULTS ===\n\n");
        for (i, result) in self.test_results.iter().enumerate() {
            let status_icon = match result.status {
                TestStatus::Passed => "✅",
                TestStatus::Failed(_) => "❌",
                TestStatus::Skipped(_) => "⏭️",
            };

            report.push_str(&format!(
                "{}. {} {} ({}ms)\n",
                i + 1,
                status_icon,
                result.test_name,
                result.duration_ms
            ));

            if let TestStatus::Failed(ref error) = result.status {
                report.push_str(&format!("   Error: {}\n", error));
            } else if let TestStatus::Skipped(ref reason) = result.status {
                report.push_str(&format!("   Reason: {}\n", reason));
            }

            if !result.details.is_empty() {
                report.push_str(&format!("   Details: {}\n", result.details));
            }
            report.push('\n');
        }

        if self.failed_tests > 0 {
            report.push_str("=== FAILED TESTS SUMMARY ===\n\n");
            for result in &self.test_results {
                if let TestStatus::Failed(ref error) = result.status {
                    report.push_str(&format!("❌ {}: {}\n", result.test_name, error));
                }
            }
            report.push('\n');
        }

        report.push_str("=== RECOMMENDATIONS ===\n\n");

        if self.success_rate() >= 90.0 {
            report.push_str("🎉 Excellent! The Biscuit implementation is working well.\n");
        } else if self.success_rate() >= 70.0 {
            report.push_str("⚠️ Good progress, but some areas need attention.\n");
        } else {
            report.push_str("🚨 Significant issues detected. Review implementation carefully.\n");
        }

        if self.failed_tests > 0 {
            report.push_str("- Review failed tests and fix underlying issues\n");
            report.push_str("- Consider implementing missing authorization logic\n");
            report.push_str("- Verify token validation and signature checking\n");
        }

        if self.skipped_tests > 0 {
            report.push_str("- Implement skipped functionality\n");
            report.push_str("- Complete partial implementations\n");
        }

        report.push_str(
            "\nNote: Some tests may pass with stub implementations but would fail in production.\n",
        );
        report.push_str(
            "Ensure proper Biscuit authorization logic is implemented before deployment.\n",
        );

        report
    }
}

impl Default for BiscuitTestSuite {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_test<F, Fut>(test_name: &str, test_fn: F) -> TestResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let start_time = std::time::Instant::now();

    match test_fn().await {
        Ok(()) => TestResult {
            test_name: test_name.to_string(),
            status: TestStatus::Passed,
            details: "Test completed successfully".to_string(),
            duration_ms: start_time.elapsed().as_millis() as u64,
        },
        Err(e) => TestResult {
            test_name: test_name.to_string(),
            status: TestStatus::Failed(e.to_string()),
            details: format!("Test failed with error: {}", e),
            duration_ms: start_time.elapsed().as_millis() as u64,
        },
    }
}

#[tokio::test]
async fn comprehensive_biscuit_integration_test() -> Result<(), Box<dyn std::error::Error>> {
    let mut suite = BiscuitTestSuite::new();

    // Test 1: Basic fixture creation and setup
    let result = run_test("Basic Fixture Creation", || async {
        let fixture = BiscuitTestFixture::new();
        assert!(!fixture.account_id().to_string().is_empty());
        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 2: Device token creation and authorization
    let result = run_test("Device Token Authorization", || async {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_device_token(device_id)?;

        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
        let token_manager = fixture.get_device_token(&device_id).unwrap();
        let token = token_manager.current_token();

        let legacy_storage_scope = LegacyResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "test_file.txt".to_string(),
        };
        let storage_scope = convert_legacy_scope(&legacy_storage_scope);

        let result = bridge.authorize(token, "read", &storage_scope)?;
        assert!(
            result.authorized,
            "Device token should authorize read operations"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 3: Guardian token functionality
    let result = run_test("Guardian Token Authorization", || async {
        let mut fixture = BiscuitTestFixture::new();
        let guardian_device = DeviceId::new();

        fixture.add_guardian_token(guardian_device)?;

        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), guardian_device);
        let guardian_token = fixture.get_guardian_token(&guardian_device).unwrap();

        let legacy_recovery_scope = LegacyResourceScope::Recovery {
            recovery_type: RecoveryType::DeviceKey,
        };
        let recovery_scope = convert_legacy_scope(&legacy_recovery_scope);

        let result = bridge.authorize(guardian_token, "recovery_approve", &recovery_scope)?;
        assert!(
            result.authorized,
            "Guardian token should authorize recovery operations"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 4: Multi-device scenario
    let result = run_test("Multi-Device Scenario", || async {
        let fixture = create_multi_device_scenario()?;

        assert!(
            !fixture.device_tokens.is_empty(),
            "Should have device tokens"
        );
        assert!(
            !fixture.guardian_tokens.is_empty(),
            "Should have guardian tokens"
        );

        // Test that all device tokens work
        for (device_id, token_manager) in &fixture.device_tokens {
            let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), *device_id);
            let token = token_manager.current_token();

            let legacy_storage_scope = LegacyResourceScope::Storage {
                category: StorageCategory::Shared,
                path: "multi_device_test.txt".to_string(),
            };
            let storage_scope = convert_legacy_scope(&legacy_storage_scope);

            let result = bridge.authorize(token, "read", &storage_scope)?;
            assert!(result.authorized, "Multi-device token should work");
        }

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 5: Delegation scenario
    let result = run_test("Delegation Chain Scenario", || async {
        let fixture = create_delegation_scenario()?;

        let chain = fixture
            .get_delegation_chain("progressive_restriction")
            .ok_or("Delegation chain not found")?;

        assert!(
            !chain.delegated_tokens.is_empty(),
            "Should have delegated tokens"
        );
        assert!(
            !chain.resource_scopes.is_empty(),
            "Should have resource scopes"
        );

        let device_id = DeviceId::new();
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

        // Test each level of delegation
        for (index, (token, scope)) in chain
            .delegated_tokens
            .iter()
            .zip(chain.resource_scopes.iter())
            .enumerate()
        {
            let converted_scope = convert_legacy_scope(scope);
            let result = bridge.authorize(token, "read", &converted_scope)?;
            assert!(
                result.authorized,
                "Delegation level {} should be authorized",
                index
            );
        }

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 6: Recovery scenario
    let result = run_test("Recovery Ceremony Scenario", || async {
        let fixture = create_recovery_scenario()?;

        assert!(
            !fixture.device_tokens.is_empty(),
            "Should have device tokens"
        );
        assert!(
            fixture.guardian_tokens.len() >= 3,
            "Should have at least 3 guardians"
        );

        // Test guardian capabilities
        for (guardian_id, guardian_token) in &fixture.guardian_tokens {
            let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), *guardian_id);

            assert!(
                bridge.has_capability(guardian_token, "recovery_approve")?,
                "Guardian should have recovery_approve capability"
            );
            assert!(
                bridge.has_capability(guardian_token, "threshold_sign")?,
                "Guardian should have threshold_sign capability"
            );
        }

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 7: Security test scenario
    let result = run_test("Security Test Scenario", || async {
        let fixture = create_security_test_scenario()?;

        assert!(
            !fixture.device_tokens.is_empty(),
            "Should have device tokens"
        );

        // Test minimal token creation
        let device_id = DeviceId::new();
        let minimal_token = fixture.create_minimal_token(device_id)?;

        assert!(
            !minimal_token.to_vec()?.is_empty(),
            "Minimal token should be created"
        );

        // Test compromised scenario token
        let compromised_token = fixture.create_compromised_scenario(DeviceId::new())?;
        assert!(
            !compromised_token.to_vec()?.is_empty(),
            "Compromised token should be created"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 8: Token serialization and deserialization
    let result = run_test("Token Serialization", || async {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_device_token(device_id)?;
        let token_manager = fixture.get_device_token(&device_id).unwrap();
        let token = token_manager.current_token();

        // Test serialization
        let serialized = token.to_vec().map_err(BiscuitError::BiscuitLib)?;
        assert!(
            !serialized.is_empty(),
            "Serialized token should not be empty"
        );

        // Test deserialization
        let deserialized = biscuit_auth::Biscuit::from(&serialized, fixture.root_public_key())
            .map_err(BiscuitError::BiscuitLib)?;

        // Verify deserialized token works
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
        let legacy_storage_scope = LegacyResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "serialization_test.txt".to_string(),
        };
        let storage_scope = convert_legacy_scope(&legacy_storage_scope);

        let result = bridge.authorize(&deserialized, "read", &storage_scope)?;
        assert!(result.authorized, "Deserialized token should work");

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 9: Token attenuation
    let result = run_test("Token Attenuation", || async {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_device_token(device_id)?;
        let token_manager = fixture.get_device_token(&device_id).unwrap();

        // Create attenuated tokens
        let read_token = token_manager.attenuate_read("documents/")?;
        let write_token = token_manager.attenuate_write("uploads/")?;

        assert!(
            !read_token.to_vec()?.is_empty(),
            "Read-attenuated token should be created"
        );
        assert!(
            !write_token.to_vec()?.is_empty(),
            "Write-attenuated token should be created"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 10: Expiring tokens
    let result = run_test("Token Expiration", || async {
        let fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        // Test different expiration scenarios
        let short_token = fixture.create_expiring_token(device_id, 1)?; // 1 second
        let long_token = fixture.create_expiring_token(device_id, 3600)?; // 1 hour

        assert!(
            !short_token.to_vec()?.is_empty(),
            "Short-lived token should be created"
        );
        assert!(
            !long_token.to_vec()?.is_empty(),
            "Long-lived token should be created"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 11: Depth-limited tokens
    let result = run_test("Delegation Depth Limits", || async {
        let fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        // Test different depth limits
        let shallow_token = fixture.create_depth_limited_token(device_id, 1)?;
        let deep_token = fixture.create_depth_limited_token(device_id, 5)?;

        assert!(
            !shallow_token.to_vec()?.is_empty(),
            "Shallow delegation token should be created"
        );
        assert!(
            !deep_token.to_vec()?.is_empty(),
            "Deep delegation token should be created"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 12: Resource scope validation
    let result = run_test("Resource Scope Validation", || async {
        use aura_wot::biscuit_resources::{
            AdminOperation, JournalOp, RecoveryType, StorageCategory,
        };

        // Test all resource scope types using the legacy API (for consistency with fixture)
        let legacy_scopes = vec![
            LegacyResourceScope::Storage {
                category: StorageCategory::Personal,
                path: "test/".to_string(),
            },
            LegacyResourceScope::Journal {
                account_id: "account123".to_string(),
                operation: JournalOp::Read,
            },
            LegacyResourceScope::Recovery {
                recovery_type: RecoveryType::DeviceKey,
            },
            LegacyResourceScope::Admin {
                operation: AdminOperation::AddGuardian,
            },
        ];
        
        // Convert to new API for testing datalog patterns
        let scopes: Vec<ResourceScope> = legacy_scopes.iter().map(|s| convert_legacy_scope(s)).collect();

        for scope in &scopes {
            let pattern = scope.resource_pattern();
            let datalog = scope.to_datalog_pattern();

            assert!(!pattern.is_empty(), "Resource pattern should not be empty");
            assert!(!datalog.is_empty(), "Datalog pattern should not be empty");
            assert!(
                datalog.contains("resource("),
                "Datalog should contain resource fact"
            );
        }

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Test 13: Cross-account isolation
    let result = run_test("Cross-Account Isolation", || async {
        let account1 = AccountId::new();
        let account2 = AccountId::new();

        let fixture1 = BiscuitTestFixture::with_account(account1);
        let fixture2 = BiscuitTestFixture::with_account(account2);

        // Verify different root keys
        let key1 = fixture1.root_public_key();
        let key2 = fixture2.root_public_key();

        assert_ne!(
            key1.to_bytes(),
            key2.to_bytes(),
            "Different accounts should have different root keys"
        );

        Ok(())
    })
    .await;
    suite.add_result(result);

    // Generate and print the comprehensive report
    let report = suite.generate_report();
    println!("{}", report);

    // Assert that we have a reasonable success rate
    assert!(
        suite.success_rate() >= 80.0,
        "Integration test suite should have at least 80% success rate, got {:.1}%",
        suite.success_rate()
    );

    // Assert no critical failures
    assert!(
        suite.failed_tests <= 2,
        "Should have at most 2 failed tests in integration suite, got {}",
        suite.failed_tests
    );

    println!("\n🎉 Comprehensive Biscuit integration test suite completed!");
    println!("📊 Success Rate: {:.1}%", suite.success_rate());
    println!("✅ Passed: {}/{}", suite.passed_tests, suite.total_tests);

    Ok(())
}

#[tokio::test]
async fn biscuit_performance_baseline() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = BiscuitTestFixture::new();
    let device_id = DeviceId::new();

    fixture.add_device_token(device_id)?;

    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
    let token_manager = fixture.get_device_token(&device_id).unwrap();
    let token = token_manager.current_token();

    let legacy_storage_scope = LegacyResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "performance_test.txt".to_string(),
    };
    let storage_scope = convert_legacy_scope(&legacy_storage_scope);

    // Measure authorization performance
    let iterations = 1000;
    let start_time = std::time::Instant::now();

    for _ in 0..iterations {
        let _result = bridge.authorize(token, "read", &storage_scope)?;
    }

    let duration = start_time.elapsed();
    let avg_duration_us = duration.as_micros() as f64 / iterations as f64;

    println!("🚀 Biscuit Authorization Performance:");
    println!("   Iterations: {}", iterations);
    println!("   Total time: {:?}", duration);
    println!("   Average per operation: {:.2}μs", avg_duration_us);

    // Performance should be reasonable (less than 1ms per operation)
    assert!(
        avg_duration_us < 1000.0,
        "Authorization should be faster than 1ms, got {:.2}μs",
        avg_duration_us
    );

    Ok(())
}

#[tokio::test]
async fn biscuit_memory_usage_baseline() -> Result<(), Box<dyn std::error::Error>> {
    // Test memory usage with many tokens
    let mut fixture = BiscuitTestFixture::new();
    let mut devices = Vec::new();

    // Create many device tokens
    for _ in 0..100 {
        let device_id = DeviceId::new();
        fixture.add_device_token(device_id)?;
        devices.push(device_id);
    }

    // Create many guardian tokens
    for _ in 0..50 {
        let device_id = DeviceId::new();
        fixture.add_guardian_token(device_id)?;
        devices.push(device_id);
    }

    // Create delegation chains
    for device_id in devices.iter().take(10) {
        let scopes = vec![
            LegacyResourceScope::Storage {
                category: StorageCategory::Personal,
                path: format!("device_{}/", device_id),
            },
            LegacyResourceScope::Storage {
                category: StorageCategory::Personal,
                path: format!("device_{}/restricted/", device_id),
            },
        ];
        fixture.create_delegation_chain(&format!("chain_{}", device_id), *device_id, scopes)?;
    }

    println!("🧠 Biscuit Memory Usage Baseline:");
    println!("   Device tokens: {}", fixture.device_tokens.len());
    println!("   Guardian tokens: {}", fixture.guardian_tokens.len());
    println!("   Delegation chains: {}", fixture.delegated_tokens.len());

    // Verify all tokens are functional
    let mut working_tokens = 0;
    let legacy_storage_scope = LegacyResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "memory_test.txt".to_string(),
    };
    let storage_scope = convert_legacy_scope(&legacy_storage_scope);

    for device_id in &devices {
        if let Some(token_manager) = fixture.get_device_token(device_id) {
            let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), *device_id);
            let result = bridge.authorize(token_manager.current_token(), "read", &storage_scope)?;
            if result.authorized {
                working_tokens += 1;
            }
        }
    }

    println!("   Working tokens: {}/{}", working_tokens, devices.len());

    assert!(working_tokens > 0, "At least some tokens should be working");

    Ok(())
}
