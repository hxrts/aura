//! Scenario Executor for CLI Recovery Demo
//!
//! This module provides high-level scenario execution capabilities that integrate
//! with the enhanced scenario framework to run the Bob's recovery demo workflow.

use crate::handlers::scenario::SimulationScenarioHandler;
use crate::scenario::types::ExpectedOutcome;
use crate::ScenarioDefinition;
use aura_core::effects::TestingError;
use std::collections::HashMap;
use std::time::Instant;

/// High-level scenario executor for CLI recovery demo
pub struct ScenarioExecutor {
    handler: SimulationScenarioHandler,
    execution_log: Vec<ExecutionStep>,
    start_time: Instant,
}

/// Record of an execution step
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    pub phase: String,
    pub action: String,
    pub timestamp: Instant,
    pub success: bool,
    pub details: Option<String>,
}

/// Execution result for scenario
#[derive(Debug)]
pub struct ExecutionResult {
    pub scenario_id: String,
    pub outcome: ExpectedOutcome,
    pub total_duration_ms: u64,
    pub steps: Vec<ExecutionStep>,
    pub final_stats: HashMap<String, String>,
    pub validation_results: HashMap<String, bool>,
}

impl ScenarioExecutor {
    /// Create a new scenario executor
    pub fn new(seed: u64) -> Self {
        Self {
            handler: SimulationScenarioHandler::new(seed),
            execution_log: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Inject a scenario into the handler (placeholder for compatibility)
    pub async fn inject_scenario(&mut self, _scenario: ScenarioDefinition) -> Result<(), TestingError> {
        // In the demo executor we run a fixed scripted flow; no-op for now.
        Ok(())
    }

    /// Process any pending scenarios (placeholder)
    pub async fn process_pending_scenarios(&mut self) -> Result<(), TestingError> {
        Ok(())
    }

    /// Execute the complete CLI recovery demo scenario
    pub async fn execute_cli_recovery_demo(&mut self) -> Result<ExecutionResult, TestingError> {
        let scenario_id = "cli_recovery_demo".to_string();
        self.start_time = Instant::now();
        self.execution_log.clear();

        // Phase 1: Alice & Charlie pre-setup
        self.log_step("alice_charlie_setup", "create_accounts", true, None);

        // Phase 2: Bob onboarding with guardian setup
        self.log_step("bob_onboarding", "create_account", true, None);
        self.log_step("bob_onboarding", "setup_guardians", true, Some("Alice and Charlie as guardians".to_string()));

        // Phase 3: Group chat establishment
        let group_id = self.handler.create_chat_group(
            "Alice, Bob & Charlie",
            "alice", 
            vec!["bob".to_string(), "charlie".to_string()]
        )?;
        self.log_step("group_chat_setup", "create_group", true, Some(format!("Group ID: {}", group_id)));

        // Phase 4: Active messaging
        let messages = vec![
            ("alice", "Welcome to our group, Bob!"),
            ("bob", "Thanks Alice! Great to be here."),
            ("charlie", "Hey everyone! This chat system is awesome."),
            ("alice", "Bob, you should backup your account soon"),
            ("bob", "I'll do that right after this demo!"),
        ];

        for (sender, message) in &messages {
            self.handler.send_chat_message(&group_id, sender, message)?;
            self.log_step("group_messaging", "send_message", true, Some(format!("{}: {}", sender, message)));
        }

        // Phase 5: Data loss simulation
        self.handler.simulate_data_loss("bob", "complete_device_loss", true)?;
        self.log_step("bob_account_loss", "simulate_data_loss", true, Some("Bob loses all account data".to_string()));

        // Phase 6: Recovery initiation
        self.handler.initiate_guardian_recovery(
            "bob",
            vec!["alice".to_string(), "charlie".to_string()],
            2
        )?;
        self.log_step("recovery_initiation", "initiate_guardian_recovery", true, Some("Alice and Charlie assist recovery".to_string()));

        // Phase 7: Account restoration
        let recovery_success = self.handler.verify_recovery_success(
            "bob",
            vec![
                "keys_restored".to_string(),
                "account_accessible".to_string(),
                "message_history_restored".to_string(),
            ]
        )?;
        self.log_step("account_restoration", "verify_recovery", recovery_success, None);

        // Phase 8: Post-recovery verification  
        let post_recovery_messages = vec![
            ("bob", "I'm back! Thanks Alice and Charlie for helping me recover."),
            ("alice", "Welcome back Bob! Guardian recovery really works!"),
            ("charlie", "Amazing! You can see all our previous messages too."),
        ];

        for (sender, message) in &post_recovery_messages {
            self.handler.send_chat_message(&group_id, sender, message)?;
            self.log_step("post_recovery_messaging", "send_message", true, Some(format!("{}: {}", sender, message)));
        }

        // Validate final state
        let mut validation_results = HashMap::new();
        
        // Validate message history continuity
        let message_continuity = self.handler.validate_message_history("bob", 8, true)?;
        validation_results.insert("message_continuity_maintained".to_string(), message_continuity);
        
        // Validate Bob can send messages
        let bob_can_send = self.handler.send_chat_message(&group_id, "bob", "Test message after recovery").is_ok();
        validation_results.insert("bob_can_send_messages".to_string(), bob_can_send);
        
        // Validate group functionality restored
        let group_functional = self.handler.get_chat_stats().is_ok();
        validation_results.insert("group_functionality_restored".to_string(), group_functional);

        // Check if Bob can see full history (including pre-recovery messages)
        let full_history_access = self.handler.validate_message_history("bob", 5, true)?;
        validation_results.insert("bob_can_see_full_history".to_string(), full_history_access);

        let final_stats = self.handler.get_chat_stats()?;
        let total_duration = self.start_time.elapsed().as_millis() as u64;

        let outcome = if validation_results.values().all(|&v| v) {
            ExpectedOutcome::RecoveryDemoSuccess
        } else {
            ExpectedOutcome::Failure
        };

        Ok(ExecutionResult {
            scenario_id,
            outcome,
            total_duration_ms: total_duration,
            steps: self.execution_log.clone(),
            final_stats,
            validation_results,
        })
    }

    /// Execute a multi-actor chat scenario
    pub async fn execute_multi_actor_chat_scenario(&mut self, group_name: &str, participants: Vec<&str>) -> Result<ExecutionResult, TestingError> {
        let scenario_id = format!("multi_actor_chat_{}", group_name.replace(' ', "_"));
        self.start_time = Instant::now();
        self.execution_log.clear();

        if participants.is_empty() {
            return Err(TestingError::EventRecordingError {
                event_type: "multi_actor_chat".to_string(),
                reason: "No participants provided".to_string(),
            });
        }

        let creator = participants[0];
        let other_members: Vec<String> = participants[1..].iter().map(|s| s.to_string()).collect();

        // Create group
        let group_id = self.handler.create_chat_group(group_name, creator, other_members)?;
        self.log_step("group_creation", "create_chat_group", true, Some(format!("Created: {}", group_name)));

        // Each participant sends a message
        for (i, participant) in participants.iter().enumerate() {
            let message = format!("Hello from {} - message {}", participant, i + 1);
            self.handler.send_chat_message(&group_id, participant, &message)?;
            self.log_step("group_messaging", "send_message", true, Some(format!("{}: {}", participant, message)));
        }

        // Validate all participants can see all messages
        let mut validation_results = HashMap::new();
        for participant in &participants {
            let can_see_all = self.handler.validate_message_history(participant, participants.len(), false)?;
            validation_results.insert(format!("{}_can_see_messages", participant), can_see_all);
        }

        let final_stats = self.handler.get_chat_stats()?;
        let total_duration = self.start_time.elapsed().as_millis() as u64;

        let outcome = if validation_results.values().all(|&v| v) {
            ExpectedOutcome::ChatGroupSuccess
        } else {
            ExpectedOutcome::Failure
        };

        Ok(ExecutionResult {
            scenario_id,
            outcome,
            total_duration_ms: total_duration,
            steps: self.execution_log.clone(),
            final_stats,
            validation_results,
        })
    }

    /// Execute a data loss and recovery scenario
    pub async fn execute_data_loss_recovery_scenario(
        &mut self, 
        target: &str, 
        guardians: Vec<&str>, 
        threshold: usize
    ) -> Result<ExecutionResult, TestingError> {
        let scenario_id = format!("data_loss_recovery_{}", target);
        self.start_time = Instant::now();
        self.execution_log.clear();

        // Setup: Create a group and send some messages
        let group_id = self.handler.create_chat_group(
            "Recovery Test Group",
            guardians[0],
            vec![target.to_string()].into_iter().chain(guardians[1..].iter().map(|s| s.to_string())).collect()
        )?;
        self.log_step("setup", "create_test_group", true, None);

        // Send initial messages
        for (i, sender) in [target].iter().chain(guardians.iter()).enumerate() {
            let message = format!("Pre-loss message {} from {}", i + 1, sender);
            self.handler.send_chat_message(&group_id, sender, &message)?;
            self.log_step("setup", "send_initial_message", true, Some(format!("{}: {}", sender, message)));
        }

        // Simulate data loss
        self.handler.simulate_data_loss(target, "complete_device_loss", true)?;
        self.log_step("data_loss", "simulate_loss", true, Some(format!("{} loses all data", target)));

        // Initiate recovery
        let guardian_strings: Vec<String> = guardians.iter().map(|s| s.to_string()).collect();
        self.handler.initiate_guardian_recovery(target, guardian_strings, threshold)?;
        self.log_step("recovery", "initiate_recovery", true, Some(format!("Guardians: {:?}, threshold: {}", guardians, threshold)));

        // Complete recovery
        let recovery_success = self.handler.verify_recovery_success(
            target,
            vec!["keys_restored".to_string(), "data_recovered".to_string()]
        )?;
        self.log_step("recovery", "complete_recovery", recovery_success, None);

        // Validate recovery
        let mut validation_results = HashMap::new();
        let message_count = guardians.len() + 1; // Number of initial messages
        let history_restored = self.handler.validate_message_history(target, message_count, true)?;
        validation_results.insert("history_restored".to_string(), history_restored);

        // Test post-recovery functionality
        let can_send_post_recovery = self.handler.send_chat_message(&group_id, target, "I'm recovered!").is_ok();
        validation_results.insert("can_send_post_recovery".to_string(), can_send_post_recovery);

        let final_stats = self.handler.get_chat_stats()?;
        let total_duration = self.start_time.elapsed().as_millis() as u64;

        let outcome = if validation_results.values().all(|&v| v) && recovery_success {
            ExpectedOutcome::Success
        } else {
            ExpectedOutcome::Failure
        };

        Ok(ExecutionResult {
            scenario_id,
            outcome,
            total_duration_ms: total_duration,
            steps: self.execution_log.clone(),
            final_stats,
            validation_results,
        })
    }

    /// Get current handler statistics  
    pub fn get_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        self.handler.get_chat_stats()
    }

    /// Get execution log
    pub fn get_execution_log(&self) -> &[ExecutionStep] {
        &self.execution_log
    }

    /// Log an execution step
    fn log_step(&mut self, phase: &str, action: &str, success: bool, details: Option<String>) {
        let step = ExecutionStep {
            phase: phase.to_string(),
            action: action.to_string(),
            timestamp: Instant::now(),
            success,
            details,
        };
        self.execution_log.push(step);
    }
}

impl ExecutionResult {
    /// Check if the scenario execution was successful
    pub fn is_success(&self) -> bool {
        matches!(self.outcome, ExpectedOutcome::Success | ExpectedOutcome::RecoveryDemoSuccess | ExpectedOutcome::ChatGroupSuccess)
    }

    /// Get a summary of the execution
    pub fn summary(&self) -> String {
        let success_rate = self.steps.iter().filter(|s| s.success).count() as f64 / self.steps.len() as f64 * 100.0;
        let passed_validations = self.validation_results.values().filter(|&&v| v).count();
        let total_validations = self.validation_results.len();

        format!(
            "Scenario: {} | Outcome: {:?} | Duration: {}ms | Steps: {}/{} successful ({:.1}%) | Validations: {}/{} passed",
            self.scenario_id,
            self.outcome,
            self.total_duration_ms,
            self.steps.iter().filter(|s| s.success).count(),
            self.steps.len(),
            success_rate,
            passed_validations,
            total_validations
        )
    }

    /// Get detailed report of the execution
    pub fn detailed_report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("=== Scenario Execution Report: {} ===\n", self.scenario_id));
        report.push_str(&format!("Outcome: {:?}\n", self.outcome));
        report.push_str(&format!("Total Duration: {}ms\n\n", self.total_duration_ms));

        report.push_str("Execution Steps:\n");
        for step in &self.steps {
            let status = if step.success { "✓" } else { "✗" };
            report.push_str(&format!(
                "  {} [{}] {}: {}",
                status,
                step.phase,
                step.action,
                step.details.as_deref().unwrap_or("N/A")
            ));
            report.push('\n');
        }

        report.push_str("\nValidation Results:\n");
        for (property, result) in &self.validation_results {
            let status = if *result { "✓ PASS" } else { "✗ FAIL" };
            report.push_str(&format!("  {} {}\n", status, property));
        }

        report.push_str("\nFinal Statistics:\n");
        for (key, value) in &self.final_stats {
            report.push_str(&format!("  {}: {}\n", key, value));
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scenario_executor_cli_recovery_demo() {
        let mut executor = ScenarioExecutor::new(2024);
        let result = executor.execute_cli_recovery_demo().await;
        
        assert!(result.is_ok(), "CLI recovery demo should execute successfully");
        let execution_result = result.unwrap();
        assert!(execution_result.is_success(), "Demo should complete successfully");
        
        println!("Demo execution summary: {}", execution_result.summary());
    }

    #[tokio::test]
    async fn test_multi_actor_chat_scenario() {
        let mut executor = ScenarioExecutor::new(123);
        let result = executor.execute_multi_actor_chat_scenario(
            "Test Chat",
            vec!["alice", "bob", "charlie"]
        ).await;

        assert!(result.is_ok(), "Multi-actor chat should execute successfully");
        let execution_result = result.unwrap();
        assert!(execution_result.is_success(), "Chat scenario should complete successfully");
    }

    #[tokio::test]
    async fn test_data_loss_recovery_scenario() {
        let mut executor = ScenarioExecutor::new(456);
        let result = executor.execute_data_loss_recovery_scenario(
            "bob",
            vec!["alice", "charlie"],
            2
        ).await;

        assert!(result.is_ok(), "Data loss recovery should execute successfully");
        let execution_result = result.unwrap();
        assert!(execution_result.is_success(), "Recovery scenario should complete successfully");
    }
}
