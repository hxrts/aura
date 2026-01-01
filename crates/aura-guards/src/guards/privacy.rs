//! Privacy budget tracking for protocol operations
//!
//! This module implements leakage budget tracking across different adversary classes
//! as specified in the formal model. It provides observer models for external,
//! neighbor, and in-group adversaries with appropriate privacy guarantees.

use super::{traits::GuardContextProvider, GuardEffects, LeakageBudget};
use aura_core::{
    effects::PhysicalTimeEffects,
    identifiers::{AuthorityId, ContextId},
    AuraError, AuraResult, FactValue, Journal,
};
use aura_journal::fact::{FactContent, LeakageFact, LeakageObserverClass, RelationalFact};
// TimeEffects removed - using PhysicalTimeEffects directly
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
    pub async fn consume_budget<T: PhysicalTimeEffects>(
        &mut self,
        operation_id: String,
        consumed: LeakageBudget,
        observable_by: Vec<AdversaryClass>,
        time_effects: &T,
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
                "Operation '{operation_id}' would exceed privacy budget"
            )));
        }

        // Consume the budget
        self.state.consumed_budget = self.state.consumed_budget.add(&consumed);

        // Record the consumption
        let timestamp = time_effects
            .physical_time()
            .await
            .map_err(|err| AuraError::internal(format!("time provider unavailable: {err}")))?
            .ts_ms;

        let consumption = BudgetConsumption {
            operation_id: operation_id.clone(),
            consumed: consumed.clone(),
            timestamp,
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
    pub async fn cleanup_old_records<T: PhysicalTimeEffects>(
        &mut self,
        time_effects: &T,
    ) -> AuraResult<()> {
        let current_time = time_effects
            .physical_time()
            .await
            .map_err(|err| AuraError::internal(format!("time provider unavailable: {err}")))?
            .ts_ms;

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
        Ok(())
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
pub async fn track_leakage_consumption<
    E: GuardEffects + PhysicalTimeEffects + GuardContextProvider,
>(
    context_id: ContextId,
    peer: Option<AuthorityId>,
    leakage_budget: &LeakageBudget,
    operation_id: &str,
    observable_by: Vec<AdversaryClass>,
    effect_system: &E,
) -> AuraResult<LeakageBudget> {
    let authority_id = effect_system.authority_id();

    let tracker = load_privacy_tracker(context_id, authority_id, effect_system).await?;
    if !tracker.can_afford_operation(leakage_budget) {
        return Err(AuraError::permission_denied(format!(
            "Operation '{operation_id}' would exceed privacy budget limits"
        )));
    }

    append_leakage_facts(
        context_id,
        authority_id,
        peer.unwrap_or(authority_id),
        leakage_budget,
        operation_id,
        effect_system,
        observable_by,
    )
    .await?;

    let updated = load_privacy_tracker(context_id, authority_id, effect_system).await?;

    debug!(
        operation_id = %operation_id,
        external = leakage_budget.external,
        neighbor = leakage_budget.neighbor,
        in_group = leakage_budget.in_group,
        remaining_external = updated.get_available_budget().external,
        remaining_neighbor = updated.get_available_budget().neighbor,
        remaining_in_group = updated.get_available_budget().in_group,
        "Privacy budget consumption tracked and persisted via journal facts"
    );

    Ok(updated.state.consumed_budget)
}

const RESET_OPERATION: &str = "privacy:reset";

fn default_limits() -> LeakageBudget {
    LeakageBudget {
        external: 1000,
        neighbor: 500,
        in_group: 100,
    }
}

/// Load privacy tracker state from journal facts
async fn load_privacy_tracker<E: GuardEffects + PhysicalTimeEffects>(
    context_id: ContextId,
    authority_id: AuthorityId,
    effect_system: &E,
) -> AuraResult<PrivacyBudgetTracker> {
    let journal = effect_system.get_journal().await?;
    let events = extract_leakage_facts(&journal, context_id);
    let limits = extract_limits(&journal, context_id).unwrap_or_else(default_limits);

    let mut tracker = PrivacyBudgetTracker::new(authority_id, limits);

    let now_ms = effect_system.physical_time().await?.ts_ms;
    let window_start =
        now_ms.saturating_sub(tracker.state.tracking_window_hours.saturating_mul(3_600_000));

    let reset_ts = events
        .iter()
        .filter(|event| event.operation == RESET_OPERATION)
        .map(|event| event.timestamp.ts_ms)
        .max()
        .unwrap_or(0);

    for event in events {
        if event.timestamp.ts_ms < window_start || event.timestamp.ts_ms < reset_ts {
            continue;
        }

        let consumed = leakage_budget_from_event(&event);
        tracker.state.consumed_budget = tracker.state.consumed_budget.add(&consumed);
        tracker.state.operation_history.push(BudgetConsumption {
            operation_id: event.operation.clone(),
            consumed,
            timestamp: event.timestamp.ts_ms,
            observable_by: vec![adversary_from_observer(event.observer)],
        });
    }

    Ok(tracker)
}

async fn append_leakage_facts<E: GuardEffects + PhysicalTimeEffects + GuardContextProvider>(
    context_id: ContextId,
    source: AuthorityId,
    destination: AuthorityId,
    leakage_budget: &LeakageBudget,
    operation_id: &str,
    effect_system: &E,
    observable_by: Vec<AdversaryClass>,
) -> AuraResult<()> {
    let timestamp = effect_system.physical_time().await?;
    let mut delta = Journal::new();

    for (observer, amount) in leakage_budget_entries(leakage_budget, &observable_by) {
        if amount == 0 {
            continue;
        }

        let leakage_fact = LeakageFact {
            context_id,
            source,
            destination,
            observer,
            amount: amount as u64,
            operation: operation_id.to_string(),
            timestamp: timestamp.clone(),
        };

        let fact_content = FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::LeakageEvent(leakage_fact),
        ));
        let bytes = serde_json::to_vec(&fact_content)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        let nonce = effect_system.random_bytes(8).await;
        let key = leakage_fact_key(context_id, observer, timestamp.ts_ms, &nonce);

        delta.facts.insert(key, FactValue::Bytes(bytes))?;
    }

    if delta.facts.is_empty() {
        return Ok(());
    }

    let merged = effect_system
        .merge_facts(&effect_system.get_journal().await?, &delta)
        .await?;
    effect_system.persist_journal(&merged).await?;

    Ok(())
}

/// Check if an authority can afford a specific operation using persistent state
pub async fn can_afford_operation<E: GuardEffects + PhysicalTimeEffects>(
    context_id: ContextId,
    authority_id: AuthorityId,
    requested_budget: &LeakageBudget,
    effect_system: &E,
) -> AuraResult<bool> {
    let tracker = load_privacy_tracker(context_id, authority_id, effect_system).await?;
    Ok(tracker.can_afford_operation(requested_budget))
}

/// Get privacy budget status for an authority using persistent storage
pub async fn get_privacy_budget_status<E: GuardEffects + PhysicalTimeEffects>(
    context_id: ContextId,
    authority_id: AuthorityId,
    effect_system: &E,
) -> AuraResult<Option<PrivacyBudgetState>> {
    match load_privacy_tracker(context_id, authority_id, effect_system).await {
        Ok(tracker) => Ok(Some(tracker.state)),
        Err(_) => Ok(None),
    }
}

/// Reset privacy budget for an authority with new limits
pub async fn reset_privacy_budget<E: GuardEffects + PhysicalTimeEffects + GuardContextProvider>(
    context_id: ContextId,
    authority_id: AuthorityId,
    new_limits: LeakageBudget,
    effect_system: &E,
) -> AuraResult<()> {
    let mut delta = Journal::new();
    let limits_key = privacy_limits_key(context_id);
    let limits_bytes =
        serde_json::to_vec(&new_limits).map_err(|e| AuraError::serialization(e.to_string()))?;
    delta
        .facts
        .insert(limits_key, FactValue::Bytes(limits_bytes))?;

    let reset_event = LeakageFact {
        context_id,
        source: authority_id,
        destination: authority_id,
        observer: LeakageObserverClass::External,
        amount: 0,
        operation: RESET_OPERATION.to_string(),
        timestamp: effect_system.physical_time().await?,
    };
    let reset_content = FactContent::Relational(RelationalFact::Protocol(
        aura_journal::ProtocolRelationalFact::LeakageEvent(reset_event),
    ));
    let reset_bytes =
        serde_json::to_vec(&reset_content).map_err(|e| AuraError::serialization(e.to_string()))?;
    let reset_key = leakage_fact_key(context_id, LeakageObserverClass::External, 0, b"reset");
    delta.facts.insert(reset_key, FactValue::Bytes(reset_bytes))?;

    let merged = effect_system
        .merge_facts(&effect_system.get_journal().await?, &delta)
        .await?;
    effect_system.persist_journal(&merged).await?;

    info!(
        authority_id = ?authority_id,
        new_external_limit = new_limits.external,
        new_neighbor_limit = new_limits.neighbor,
        new_in_group_limit = new_limits.in_group,
        "Privacy budget reset with new limits"
    );

    Ok(())
}

fn privacy_limits_key(context_id: ContextId) -> String {
    format!("privacy_limits:{context_id}")
}

fn leakage_fact_key(
    context_id: ContextId,
    observer: LeakageObserverClass,
    timestamp_ms: u64,
    nonce: &[u8],
) -> String {
    let observer_tag = match observer {
        LeakageObserverClass::External => "external",
        LeakageObserverClass::Neighbor => "neighbor",
        LeakageObserverClass::InGroup => "in_group",
    };
    format!(
        "leakage:{}:{}:{}:{}",
        context_id,
        observer_tag,
        timestamp_ms,
        hex::encode(nonce)
    )
}

fn extract_limits(journal: &Journal, context_id: ContextId) -> Option<LeakageBudget> {
    let key = privacy_limits_key(context_id);
    journal
        .read_facts()
        .get(&key)
        .and_then(|value| match value {
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes).ok(),
            FactValue::String(text) => serde_json::from_str(text).ok(),
            FactValue::Nested(nested) => serde_json::to_vec(nested)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
            _ => None,
        })
}

fn extract_leakage_facts(journal: &Journal, context_id: ContextId) -> Vec<LeakageFact> {
    journal
        .read_facts()
        .iter()
        .filter_map(|(_key, value)| decode_fact_content(value))
        .filter_map(|content| match content {
            FactContent::Relational(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::LeakageEvent(event),
            )) if event.context_id == context_id => Some(event),
            _ => None,
        })
        .collect()
}

fn decode_fact_content(value: &FactValue) -> Option<FactContent> {
    match value {
        FactValue::Bytes(bytes) => serde_json::from_slice(bytes).ok(),
        FactValue::String(text) => serde_json::from_str(text).ok(),
        FactValue::Nested(nested) => serde_json::to_vec(nested)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
        _ => None,
    }
}

fn leakage_budget_entries(
    budget: &LeakageBudget,
    observable_by: &[AdversaryClass],
) -> Vec<(LeakageObserverClass, u32)> {
    let mut entries = Vec::new();
    for observer in observable_by {
        match observer {
            AdversaryClass::External => {
                entries.push((LeakageObserverClass::External, budget.external));
            }
            AdversaryClass::Neighbor => {
                entries.push((LeakageObserverClass::Neighbor, budget.neighbor));
            }
            AdversaryClass::InGroup => {
                entries.push((LeakageObserverClass::InGroup, budget.in_group));
            }
        }
    }
    entries
}

fn leakage_budget_from_event(event: &LeakageFact) -> LeakageBudget {
    let mut budget = LeakageBudget::zero();
    let amount = u64::min(event.amount, u64::from(u32::MAX)) as u32;
    match event.observer {
        LeakageObserverClass::External => budget.external = amount,
        LeakageObserverClass::Neighbor => budget.neighbor = amount,
        LeakageObserverClass::InGroup => budget.in_group = amount,
    }
    budget
}

fn adversary_from_observer(observer: LeakageObserverClass) -> AdversaryClass {
    match observer {
        LeakageObserverClass::External => AdversaryClass::External,
        LeakageObserverClass::Neighbor => AdversaryClass::Neighbor,
        LeakageObserverClass::InGroup => AdversaryClass::InGroup,
    }
}
