//! Privacy budget tracking for protocol operations
//!
//! This module implements leakage budget tracking across different adversary classes
//! as specified in the formal model. It provides observer models for external,
//! neighbor, and in-group adversaries with appropriate privacy guarantees.

#![allow(clippy::disallowed_methods)] // TODO: Replace direct time calls with effect system

use super::effect_system_trait::GuardEffectSystem;
use super::LeakageBudget;
use aura_core::{AuraError, AuraResult, identifiers::AuthorityId};
use tracing::{debug, info, warn};

/// Privacy budget tracking state for a device
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrivacyBudgetState {
    /// Total budget consumed across all adversary classes
    pub consumed_budget: LeakageBudget,
    /// Budget consumption per operation (for analysis)
    pub operation_history: Vec<BudgetConsumption>,
    /// Current budget limits (configurable)
    pub budget_limits: LeakageBudget,
    /// Time window for budget tracking (rolling window)
    pub tracking_window_hours: u64,
}

/// Record of budget consumption for a specific operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetConsumption {
    /// Operation identifier
    pub operation_id: String,
    /// Budget consumed by this operation
    pub consumed: LeakageBudget,
    /// Timestamp of consumption
    pub timestamp: u64,
    /// Adversary classes that could observe this operation
    pub observable_by: Vec<AdversaryClass>,
}

/// Adversary classes for privacy analysis
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AdversaryClass {
    /// External adversary (outside the system)
    External,
    /// Neighbor adversary (network-level observation)
    Neighbor,
    /// In-group adversary (within the peer group)
    InGroup,
}

/// Privacy budget tracker
pub struct PrivacyBudgetTracker {
    authority_id: AuthorityId,
    state: PrivacyBudgetState,
}

impl PrivacyBudgetTracker {
    /// Create a new privacy budget tracker
    pub fn new(authority_id: AuthorityId, limits: LeakageBudget) -> Self {
        Self {
            authority_id,
            state: PrivacyBudgetState {
                consumed_budget: LeakageBudget::zero(),
                operation_history: Vec::new(),
                budget_limits: limits,
                tracking_window_hours: 24, // 24-hour rolling window
            },
        }
    }

    /// Check if an operation can be performed within budget limits
    pub fn can_afford_operation(&self, requested_budget: &LeakageBudget) -> bool {
        let potential_consumption = self.state.consumed_budget.add(requested_budget);
        potential_consumption.is_within_limits(&self.state.budget_limits)
    }

    /// Consume budget for an operation
    pub fn consume_budget(
        &mut self,
        operation_id: String,
        consumed: LeakageBudget,
        observable_by: Vec<AdversaryClass>,
    ) -> AuraResult<()> {
        // Check if we can afford this operation
        if !self.can_afford_operation(&consumed) {
            warn!(
                operation_id = %operation_id,
                requested_external = consumed.external,
                requested_neighbor = consumed.neighbor,
                requested_in_group = consumed.in_group,
                available_external = self.state.budget_limits.external - self.state.consumed_budget.external,
                available_neighbor = self.state.budget_limits.neighbor - self.state.consumed_budget.neighbor,
                available_in_group = self.state.budget_limits.in_group - self.state.consumed_budget.in_group,
                "Operation would exceed privacy budget"
            );

            return Err(AuraError::permission_denied(format!(
                "Operation '{}' would exceed privacy budget",
                operation_id
            )));
        }

        // Consume the budget
        self.state.consumed_budget = self.state.consumed_budget.add(&consumed);

        // Record the consumption
        let consumption = BudgetConsumption {
            operation_id: operation_id.clone(),
            consumed: consumed.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            observable_by,
        };

        self.state.operation_history.push(consumption);

        debug!(
            operation_id = %operation_id,
            consumed_external = consumed.external,
            consumed_neighbor = consumed.neighbor,
            consumed_in_group = consumed.in_group,
            total_consumed_external = self.state.consumed_budget.external,
            total_consumed_neighbor = self.state.consumed_budget.neighbor,
            total_consumed_in_group = self.state.consumed_budget.in_group,
            "Privacy budget consumed"
        );

        Ok(())
    }

    /// Clean up old budget consumption records outside the tracking window
    pub fn cleanup_old_records(&mut self) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let window_start = current_time.saturating_sub(self.state.tracking_window_hours * 3600);

        // Remove old records and recalculate consumed budget
        self.state
            .operation_history
            .retain(|record| record.timestamp >= window_start);

        // Recalculate total consumption from remaining records
        let mut new_consumed = LeakageBudget::zero();
        for record in &self.state.operation_history {
            new_consumed = new_consumed.add(&record.consumed);
        }

        if new_consumed.external != self.state.consumed_budget.external
            || new_consumed.neighbor != self.state.consumed_budget.neighbor
            || new_consumed.in_group != self.state.consumed_budget.in_group
        {
            debug!(
                old_external = self.state.consumed_budget.external,
                new_external = new_consumed.external,
                old_neighbor = self.state.consumed_budget.neighbor,
                new_neighbor = new_consumed.neighbor,
                old_in_group = self.state.consumed_budget.in_group,
                new_in_group = new_consumed.in_group,
                records_cleaned = self.state.operation_history.len(),
                "Privacy budget updated after cleanup"
            );
        }

        self.state.consumed_budget = new_consumed;
    }

    /// Get current budget availability
    pub fn get_available_budget(&self) -> LeakageBudget {
        LeakageBudget {
            external: self
                .state
                .budget_limits
                .external
                .saturating_sub(self.state.consumed_budget.external),
            neighbor: self
                .state
                .budget_limits
                .neighbor
                .saturating_sub(self.state.consumed_budget.neighbor),
            in_group: self
                .state
                .budget_limits
                .in_group
                .saturating_sub(self.state.consumed_budget.in_group),
        }
    }

    /// Get consumption history for analysis
    pub fn get_consumption_history(&self) -> &[BudgetConsumption] {
        &self.state.operation_history
    }
}

/// Track leakage consumption for an operation with persistent state management
pub async fn track_leakage_consumption<E: GuardEffectSystem>(
    leakage_budget: &LeakageBudget,
    operation_id: &str,
    effect_system: &E,
) -> AuraResult<LeakageBudget> {
    // Get current authority ID from effect system
    let authority_id = effect_system.authority_id();

    // Load current privacy budget state from storage
    let mut tracker = load_privacy_tracker(authority_id, effect_system).await?;

    // Classify which adversary classes can observe this operation
    let observable_by = classify_operation_observability(operation_id, effect_system).await;

    // Check if we can afford this operation
    if !tracker.can_afford_operation(leakage_budget) {
        return Err(AuraError::permission_denied(format!(
            "Operation '{}' would exceed privacy budget limits",
            operation_id
        )));
    }

    // Consume the budget and record the operation
    tracker.consume_budget(
        operation_id.to_string(),
        leakage_budget.clone(),
        observable_by,
    )?;

    // Clean up old records outside the tracking window
    tracker.cleanup_old_records();

    // Save updated state back to storage
    save_privacy_tracker(&tracker, effect_system).await?;

    debug!(
        operation_id = %operation_id,
        external = leakage_budget.external,
        neighbor = leakage_budget.neighbor,
        in_group = leakage_budget.in_group,
        remaining_external = tracker.get_available_budget().external,
        remaining_neighbor = tracker.get_available_budget().neighbor,
        remaining_in_group = tracker.get_available_budget().in_group,
        "Privacy budget consumption tracked and persisted"
    );

    Ok(leakage_budget.clone())
}

/// Classify which adversary classes can observe an operation
async fn classify_operation_observability<E: GuardEffectSystem>(
    operation_id: &str,
    effect_system: &E,
) -> Vec<AdversaryClass> {
    // This is a TODO fix - Simplified classification - in practice, this would analyze
    // the operation type, network patterns, and protocol specifics

    let mut observable_by = Vec::new();

    // Network operations are observable by external and neighbor adversaries
    if operation_id.contains("send") || operation_id.contains("receive") {
        observable_by.push(AdversaryClass::External);
        observable_by.push(AdversaryClass::Neighbor);
    }

    // Peer-to-peer operations are observable by in-group adversaries
    if operation_id.contains("p2p") || operation_id.contains("peer") {
        observable_by.push(AdversaryClass::InGroup);
    }

    // Broadcast operations are observable by all adversary classes
    if operation_id.contains("broadcast") || operation_id.contains("gossip") {
        observable_by.push(AdversaryClass::External);
        observable_by.push(AdversaryClass::Neighbor);
        observable_by.push(AdversaryClass::InGroup);
    }

    debug!(
        operation_id = %operation_id,
        observable_by = ?observable_by,
        "Classified operation observability"
    );

    observable_by
}

/// Load privacy tracker state from persistent storage
async fn load_privacy_tracker<E: GuardEffectSystem>(
    authority_id: AuthorityId,
    effect_system: &E,
) -> AuraResult<PrivacyBudgetTracker> {
    let storage_key = format!("privacy_budget_{}", authority_id);

    use aura_core::effects::StorageEffects;
    let storage_result = effect_system.retrieve(&storage_key).await;

    match storage_result {
        Ok(Some(data)) => {
            // Deserialize existing tracker state
            let state: PrivacyBudgetState = serde_json::from_slice(&data).map_err(|e| {
                AuraError::invalid(format!("Failed to deserialize privacy state: {}", e))
            })?;

            Ok(PrivacyBudgetTracker { authority_id, state })
        }
        Ok(None) => {
            // Create new tracker with default limits
            let default_limits = LeakageBudget {
                external: 1000, // Conservative daily limits
                neighbor: 500,
                in_group: 100,
            };
            Ok(PrivacyBudgetTracker::new(authority_id, default_limits))
        }
        Err(e) => {
            warn!("Failed to load privacy budget state: {}", e);
            // Fallback to conservative default
            let default_limits = LeakageBudget {
                external: 100, // Very conservative fallback
                neighbor: 50,
                in_group: 10,
            };
            Ok(PrivacyBudgetTracker::new(authority_id, default_limits))
        }
    }
}

/// Save privacy tracker state to persistent storage
async fn save_privacy_tracker<E: GuardEffectSystem>(
    tracker: &PrivacyBudgetTracker,
    effect_system: &E,
) -> AuraResult<()> {
    let storage_key = format!("privacy_budget_{}", tracker.authority_id);

    let serialized = serde_json::to_vec(&tracker.state)
        .map_err(|e| AuraError::invalid(format!("Failed to serialize privacy state: {}", e)))?;

    // Use StorageEffects trait method
    use aura_core::effects::StorageEffects;
    effect_system
        .store(&storage_key, serialized)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to store privacy state: {}", e)))?;

    Ok(())
}

/// Check if an authority can afford a specific operation using persistent state
pub async fn can_afford_operation<E: GuardEffectSystem>(
    authority_id: AuthorityId,
    requested_budget: &LeakageBudget,
    effect_system: &E,
) -> AuraResult<bool> {
    // Load current tracker state
    let tracker = load_privacy_tracker(authority_id, effect_system).await?;

    // Check affordability
    Ok(tracker.can_afford_operation(requested_budget))
}

/// Get privacy budget status for an authority using persistent storage
pub async fn get_privacy_budget_status<E: GuardEffectSystem>(
    authority_id: AuthorityId,
    effect_system: &E,
) -> AuraResult<Option<PrivacyBudgetState>> {
    match load_privacy_tracker(authority_id, effect_system).await {
        Ok(tracker) => {
            let mut state = tracker.state.clone();
            // Clean up old records before returning status
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let window_start = current_time.saturating_sub(state.tracking_window_hours * 3600);

            state
                .operation_history
                .retain(|record| record.timestamp >= window_start);

            Ok(Some(state))
        }
        Err(_) => {
            // Return None if no state exists or cannot be loaded
            Ok(None)
        }
    }
}

/// Reset privacy budget for an authority with new limits
pub async fn reset_privacy_budget<E: GuardEffectSystem>(
    authority_id: AuthorityId,
    new_limits: LeakageBudget,
    effect_system: &E,
) -> AuraResult<()> {
    // Create fresh tracker with new limits
    let tracker = PrivacyBudgetTracker::new(authority_id, new_limits.clone());

    // Save to persistent storage
    save_privacy_tracker(&tracker, effect_system).await?;

    info!(
        authority_id = ?authority_id,
        new_external_limit = new_limits.external,
        new_neighbor_limit = new_limits.neighbor,
        new_in_group_limit = new_limits.in_group,
        "Privacy budget reset with new limits"
    );

    Ok(())
}
