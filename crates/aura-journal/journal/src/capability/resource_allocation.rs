//! Resource Allocation and Service Differentiation
//!
//! Implements capability-based resource allocation, quota management, and reputation-based
//! service differentiation. Uses the unified capability system based on Keyhive capabilities.
//! Resource allocation logic is separated from protocol logic to maintain clean architecture.
//!
//! Reference: docs/041_rendezvous.md Post-MVP Roadmap Phase 2
//!          work/ssb_storage.md Phase 6.4

use super::unified::CapabilityToken;
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resource quota for relay operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayQuota {
    /// Capability token authorizing relay operations
    pub capability: CapabilityToken,

    /// Remaining operation count
    pub remaining_operations: u64,

    /// Issued timestamp
    pub issued_at: u64,

    /// Expiration timestamp (optional)
    pub expires_at: Option<u64>,
}

impl RelayQuota {
    /// Create new relay quota
    pub fn new(capability: CapabilityToken, operations: u64, issued_at: u64) -> Self {
        Self {
            capability,
            remaining_operations: operations,
            issued_at,
            expires_at: None,
        }
    }

    /// Check if quota is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expires_at.is_some_and(|expiry| current_time > expiry)
    }

    /// Consume operations from quota
    pub fn consume(&mut self, amount: u64) -> Result<(), ResourceError> {
        if self.remaining_operations < amount {
            return Err(ResourceError::InsufficientQuota);
        }
        self.remaining_operations -= amount;
        Ok(())
    }

    /// Set expiration
    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }
}

/// Storage quota offering from a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageQuotaOffer {
    /// Offer ID
    pub offer_id: Vec<u8>,

    /// Peer providing storage
    pub provider_id: Vec<u8>,

    /// Amount of storage in bytes
    pub storage_bytes: u64,

    /// Required operation allocation per GB per month
    pub operations_per_gb_month: u64,

    /// Minimum trust level required
    pub min_trust_level: TrustLevel,

    /// Whether offer is still available
    pub available: bool,

    /// Created timestamp
    pub created_at: u64,
}

impl StorageQuotaOffer {
    /// Create a new storage quota offer
    pub fn new(
        offer_id: Vec<u8>,
        provider_id: Vec<u8>,
        storage_bytes: u64,
        operations_per_gb_month: u64,
        min_trust_level: TrustLevel,
        created_at: u64,
    ) -> Self {
        Self {
            offer_id,
            provider_id,
            storage_bytes,
            operations_per_gb_month,
            min_trust_level,
            available: true,
            created_at,
        }
    }

    /// Calculate total operation cost
    pub fn calculate_operation_cost(&self) -> u64 {
        let gb = (self.storage_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
        (gb * self.operations_per_gb_month as f64) as u64
    }

    /// Mark offer as fulfilled
    pub fn fulfill(&mut self) {
        self.available = false;
    }
}

/// Trust level for service differentiation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Untrusted,
    Basic,
    Verified,
    Premium,
}

/// Service tier based on reputation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceTier {
    Free,
    Basic,
    Premium,
    Enterprise,
}

impl ServiceTier {
    /// Get storage quota for tier (bytes)
    pub fn storage_quota(&self) -> u64 {
        match self {
            ServiceTier::Free => 100 * 1024 * 1024,              // 100 MB
            ServiceTier::Basic => 1024 * 1024 * 1024,            // 1 GB
            ServiceTier::Premium => 10 * 1024 * 1024 * 1024,     // 10 GB
            ServiceTier::Enterprise => 100 * 1024 * 1024 * 1024, // 100 GB
        }
    }

    /// Get relay operation allocation per month
    pub fn monthly_relay_operations(&self) -> u64 {
        match self {
            ServiceTier::Free => 1000,
            ServiceTier::Basic => 10_000,
            ServiceTier::Premium => 100_000,
            ServiceTier::Enterprise => 1_000_000,
        }
    }

    /// Get priority level (higher is better)
    pub fn priority(&self) -> u32 {
        match self {
            ServiceTier::Free => 0,
            ServiceTier::Basic => 1,
            ServiceTier::Premium => 2,
            ServiceTier::Enterprise => 3,
        }
    }
}

/// Reputation score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScore {
    /// Account ID
    pub account_id: Vec<u8>,

    /// Overall reputation (0.0 - 1.0)
    pub score: f64,

    /// Number of successful interactions
    pub successful_interactions: u64,

    /// Number of failed interactions
    pub failed_interactions: u64,

    /// Account age in days
    pub account_age_days: u32,

    /// Last updated timestamp
    pub last_updated: u64,
}

impl ReputationScore {
    /// Create a new reputation score
    pub fn new(account_id: Vec<u8>, created_at: u64) -> Self {
        Self {
            account_id,
            score: 0.5, // Start with neutral reputation
            successful_interactions: 0,
            failed_interactions: 0,
            account_age_days: 0,
            last_updated: created_at,
        }
    }

    /// Record a successful interaction
    pub fn record_success(&mut self, current_time: u64) {
        self.successful_interactions += 1;
        self.update_score(current_time);
    }

    /// Record a failed interaction
    pub fn record_failure(&mut self, current_time: u64) {
        self.failed_interactions += 1;
        self.update_score(current_time);
    }

    /// Update reputation score
    fn update_score(&mut self, current_time: u64) {
        let total = self.successful_interactions + self.failed_interactions;
        if total == 0 {
            return;
        }

        // Base score from success rate
        let success_rate = self.successful_interactions as f64 / total as f64;

        // Age bonus (up to 0.2 boost at 1 year)
        let age_bonus = (self.account_age_days as f64 / 365.0).min(0.2);

        // Combine factors
        self.score = (success_rate + age_bonus).min(1.0);
        self.last_updated = current_time;
    }

    /// Get service tier based on reputation
    pub fn service_tier(&self) -> ServiceTier {
        if self.score >= 0.9 && self.account_age_days >= 90 {
            ServiceTier::Premium
        } else if self.score >= 0.7 && self.account_age_days >= 30 {
            ServiceTier::Basic
        } else {
            ServiceTier::Free
        }
    }

    /// Update account age
    pub fn update_age(&mut self, current_time: u64, created_at: u64) {
        let age_ms = current_time.saturating_sub(created_at);
        self.account_age_days = (age_ms / (24 * 3600 * 1000)) as u32;
    }
}

/// Resource exchange for service delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceExchange {
    /// Exchange ID
    pub exchange_id: Vec<u8>,

    /// Requestor account
    pub from_account: Vec<u8>,

    /// Provider account
    pub to_account: Vec<u8>,

    /// Operation count being exchanged
    pub operation_count: u64,

    /// Service being requested
    pub service_type: ServiceType,

    /// Exchange status
    pub status: ExchangeStatus,

    /// Created timestamp
    pub created_at: u64,

    /// Completed timestamp
    pub completed_at: Option<u64>,
}

impl ResourceExchange {
    /// Create a new resource exchange
    pub fn new(
        exchange_id: Vec<u8>,
        from_account: Vec<u8>,
        to_account: Vec<u8>,
        operation_count: u64,
        service_type: ServiceType,
        created_at: u64,
    ) -> Self {
        Self {
            exchange_id,
            from_account,
            to_account,
            operation_count,
            service_type,
            status: ExchangeStatus::Pending,
            created_at,
            completed_at: None,
        }
    }

    /// Mark exchange as completed
    pub fn complete(&mut self, current_time: u64) {
        self.status = ExchangeStatus::Completed;
        self.completed_at = Some(current_time);
    }

    /// Mark exchange as failed
    pub fn fail(&mut self, reason: String) {
        self.status = ExchangeStatus::Failed(reason);
    }
}

/// Service type for resource exchanges
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    Relay,
    Storage,
    Bandwidth,
    PriorityDelivery,
}

/// Exchange status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExchangeStatus {
    Pending,
    Completed,
    Failed(String),
}

/// Resource allocation manager
#[derive(Debug, Clone)]
pub struct ResourceAllocationManager {
    /// Relay operation quotas
    relay_quotas: HashMap<DeviceId, RelayQuota>,

    /// Storage quota offers
    storage_offers: HashMap<Vec<u8>, StorageQuotaOffer>,

    /// Reputation scores
    reputation_scores: HashMap<Vec<u8>, ReputationScore>,

    /// Pending resource exchanges
    pending_exchanges: HashMap<Vec<u8>, ResourceExchange>,
}

impl ResourceAllocationManager {
    /// Create a new resource allocation manager
    pub fn new() -> Self {
        Self {
            relay_quotas: HashMap::new(),
            storage_offers: HashMap::new(),
            reputation_scores: HashMap::new(),
            pending_exchanges: HashMap::new(),
        }
    }

    /// Allocate relay operations to an account
    pub fn allocate_relay_operations(
        &mut self,
        capability: CapabilityToken,
        operations: u64,
        current_time: u64,
    ) -> RelayQuota {
        let device_id = capability.authenticated_device;
        let quota = RelayQuota::new(capability, operations, current_time);
        self.relay_quotas.insert(device_id, quota.clone());
        quota
    }

    /// Transfer relay operations between accounts
    pub fn transfer_relay_operations(
        &mut self,
        from_device: &DeviceId,
        to_capability: CapabilityToken,
        amount: u64,
        current_time: u64,
    ) -> Result<(), ResourceError> {
        // Consume operations from source
        let quota = self
            .relay_quotas
            .get_mut(from_device)
            .ok_or(ResourceError::QuotaNotFound)?;
        quota.consume(amount)?;

        // Allocate operations to recipient
        self.allocate_relay_operations(to_capability, amount, current_time);

        Ok(())
    }

    /// Create a storage quota offer
    pub fn create_storage_offer(
        &mut self,
        provider_id: Vec<u8>,
        storage_bytes: u64,
        operations_per_gb_month: u64,
        min_trust_level: TrustLevel,
        current_time: u64,
    ) -> StorageQuotaOffer {
        let offer_id = self.generate_id(&provider_id, current_time);
        let offer = StorageQuotaOffer::new(
            offer_id.clone(),
            provider_id,
            storage_bytes,
            operations_per_gb_month,
            min_trust_level,
            current_time,
        );
        self.storage_offers.insert(offer_id, offer.clone());
        offer
    }

    /// Accept a storage offer
    pub fn accept_storage_offer(
        &mut self,
        offer_id: &[u8],
        requestor_id: Vec<u8>,
        current_time: u64,
    ) -> Result<ResourceExchange, ResourceError> {
        // Get offer details and validate
        let (operation_cost, provider_id) = {
            let offer = self
                .storage_offers
                .get(offer_id)
                .ok_or(ResourceError::OfferNotFound)?;

            if !offer.available {
                return Err(ResourceError::OfferUnavailable);
            }

            (offer.calculate_operation_cost(), offer.provider_id.clone())
        };

        // Create resource exchange
        let exchange_id = self.generate_id(&requestor_id, current_time);
        let exchange = ResourceExchange::new(
            exchange_id.clone(),
            requestor_id,
            provider_id,
            operation_cost,
            ServiceType::Storage,
            current_time,
        );

        // Fulfill offer (safe: verified exists above)
        if let Some(offer) = self.storage_offers.get_mut(offer_id) {
            offer.fulfill();
        }

        self.pending_exchanges.insert(exchange_id, exchange.clone());

        Ok(exchange)
    }

    /// Complete a resource exchange
    pub fn complete_exchange(
        &mut self,
        exchange_id: &[u8],
        from_capability: CapabilityToken,
        to_capability: CapabilityToken,
        current_time: u64,
    ) -> Result<(), ResourceError> {
        // Get exchange details first
        let operation_count = {
            let exchange = self
                .pending_exchanges
                .get(exchange_id)
                .ok_or(ResourceError::ExchangeNotFound)?;
            exchange.operation_count
        };

        // Transfer operations using the from_capability's device
        let from_device = &from_capability.authenticated_device;
        self.transfer_relay_operations(from_device, to_capability, operation_count, current_time)?;

        // Update exchange status
        let exchange = self
            .pending_exchanges
            .get_mut(exchange_id)
            .ok_or(ResourceError::ExchangeNotFound)?;
        exchange.complete(current_time);

        Ok(())
    }

    /// Get or create reputation score
    pub fn get_reputation(&mut self, account_id: &[u8], created_at: u64) -> &mut ReputationScore {
        self.reputation_scores
            .entry(account_id.to_vec())
            .or_insert_with(|| ReputationScore::new(account_id.to_vec(), created_at))
    }

    /// Record successful interaction
    pub fn record_success(&mut self, account_id: &[u8], current_time: u64, created_at: u64) {
        let reputation = self.get_reputation(account_id, created_at);
        reputation.record_success(current_time);
    }

    /// Record failed interaction
    pub fn record_failure(&mut self, account_id: &[u8], current_time: u64, created_at: u64) {
        let reputation = self.get_reputation(account_id, created_at);
        reputation.record_failure(current_time);
    }

    /// Get service tier for account
    pub fn get_service_tier(
        &mut self,
        account_id: &[u8],
        created_at: u64,
        current_time: u64,
    ) -> ServiceTier {
        let reputation = self.get_reputation(account_id, created_at);
        reputation.update_age(current_time, created_at);
        reputation.service_tier()
    }

    /// Generate a unique ID
    fn generate_id(&self, account_id: &[u8], timestamp: u64) -> Vec<u8> {
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(account_id);
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(&(self.relay_quotas.len() as u64).to_le_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}

impl Default for ResourceAllocationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource allocation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    InsufficientQuota,
    QuotaNotFound,
    OfferNotFound,
    OfferUnavailable,
    ExchangeNotFound,
    TransferFailed,
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceError::InsufficientQuota => write!(f, "Insufficient operation quota"),
            ResourceError::QuotaNotFound => write!(f, "Operation quota not found"),
            ResourceError::OfferNotFound => write!(f, "Storage offer not found"),
            ResourceError::OfferUnavailable => write!(f, "Storage offer no longer available"),
            ResourceError::ExchangeNotFound => write!(f, "Resource exchange not found"),
            ResourceError::TransferFailed => write!(f, "Operation transfer failed"),
        }
    }
}

impl std::error::Error for ResourceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::unified::{Permission, StorageOperation};
    use aura_types::DeviceId;
    use uuid::Uuid;

    fn test_capability() -> CapabilityToken {
        CapabilityToken {
            authenticated_device: DeviceId(Uuid::from_bytes([1; 16])),
            granted_permissions: vec![Permission::Storage {
                operation: StorageOperation::Write,
                resource: "test".to_string(),
            }],
            delegation_chain: Vec::new(),
            signature: vec![0; 64],
            issued_at: 1000,
            expires_at: None,
        }
    }

    #[test]
    fn test_relay_quota_creation() {
        let capability = test_capability();
        let quota = RelayQuota::new(capability, 1000, 1000);
        assert_eq!(quota.remaining_operations, 1000);
        assert!(!quota.is_expired(2000));
    }

    #[test]
    fn test_relay_quota_consumption() {
        let capability = test_capability();
        let mut quota = RelayQuota::new(capability, 1000, 1000);

        quota.consume(100).unwrap();
        assert_eq!(quota.remaining_operations, 900);

        assert!(quota.consume(1000).is_err());
    }

    #[test]
    fn test_relay_quota_expiration() {
        let capability = test_capability();
        let quota = RelayQuota::new(capability, 1000, 1000).with_expiration(5000);

        assert!(!quota.is_expired(4999));
        assert!(quota.is_expired(5001));
    }

    #[test]
    fn test_storage_quota_offer() {
        let offer = StorageQuotaOffer::new(
            vec![1],
            vec![2],
            5 * 1024 * 1024 * 1024, // 5 GB
            100,
            TrustLevel::Basic,
            1000,
        );

        assert_eq!(offer.calculate_operation_cost(), 500); // 5 GB * 100 operations/GB
        assert!(offer.available);
    }

    #[test]
    fn test_service_tier_quotas() {
        assert_eq!(ServiceTier::Free.storage_quota(), 100 * 1024 * 1024);
        assert_eq!(ServiceTier::Basic.storage_quota(), 1024 * 1024 * 1024);
        assert_eq!(
            ServiceTier::Premium.storage_quota(),
            10 * 1024 * 1024 * 1024
        );

        assert_eq!(ServiceTier::Free.monthly_relay_operations(), 1000);
        assert_eq!(ServiceTier::Premium.monthly_relay_operations(), 100_000);
    }

    #[test]
    fn test_reputation_score() {
        let mut reputation = ReputationScore::new(vec![1], 1000);
        assert_eq!(reputation.score, 0.5);

        for _ in 0..10 {
            reputation.record_success(2000);
        }
        assert!(reputation.score > 0.5);

        reputation.record_failure(3000);
        let score_after_failure = reputation.score;
        assert!(score_after_failure < 1.0);
    }

    #[test]
    fn test_reputation_service_tier() {
        let mut reputation = ReputationScore::new(vec![1], 1000);

        // New account gets Free tier
        assert_eq!(reputation.service_tier(), ServiceTier::Free);

        // Build reputation
        for _ in 0..100 {
            reputation.record_success(2000);
        }
        reputation.account_age_days = 90;
        reputation.update_score(2000);

        assert_eq!(reputation.service_tier(), ServiceTier::Premium);
    }

    #[test]
    fn test_manager_allocate_operations() {
        let mut manager = ResourceAllocationManager::new();
        let capability = test_capability();

        let quota = manager.allocate_relay_operations(capability, 1000, 1000);
        assert_eq!(quota.remaining_operations, 1000);
    }

    #[test]
    fn test_manager_create_offer() {
        let mut manager = ResourceAllocationManager::new();
        let provider_id = vec![1];

        let offer = manager.create_storage_offer(
            provider_id.clone(),
            1024 * 1024 * 1024,
            100,
            TrustLevel::Basic,
            1000,
        );

        assert_eq!(offer.provider_id, provider_id);
        assert!(offer.available);
    }

    #[test]
    fn test_manager_reputation() {
        let mut manager = ResourceAllocationManager::new();
        let account_id = vec![1];

        manager.record_success(&account_id, 2000, 1000);
        manager.record_success(&account_id, 3000, 1000);

        let reputation = manager.get_reputation(&account_id, 1000);
        assert!(reputation.successful_interactions >= 2);
        assert!(reputation.score > 0.5);
    }

    #[test]
    fn test_resource_exchange_lifecycle() {
        let exchange =
            ResourceExchange::new(vec![1], vec![2], vec![3], 100, ServiceType::Relay, 1000);

        assert_eq!(exchange.status, ExchangeStatus::Pending);
        assert_eq!(exchange.operation_count, 100);

        let mut exchange = exchange;
        exchange.complete(2000);
        assert_eq!(exchange.status, ExchangeStatus::Completed);
        assert_eq!(exchange.completed_at, Some(2000));
    }
}
