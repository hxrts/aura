//! Economic Incentives for Storage and Relay Services
//!
//! Implements capability-based tokens for relay credits, storage quota marketplace,
//! and reputation-based service differentiation. Economic logic is separated from
//! protocol logic to maintain clean architecture.
//!
//! Reference: docs/041_rendezvous.md Post-MVP Roadmap Phase 2
//!          work/ssb_storage.md Phase 6.4

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Relay credit token (Biscuit-based capability token)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayCredit {
    /// Token ID
    pub token_id: Vec<u8>,

    /// Account holding the credit
    pub account_id: Vec<u8>,

    /// Number of relay operations available
    pub credits: u64,

    /// Issued timestamp
    pub issued_at: u64,

    /// Expiration timestamp (optional)
    pub expires_at: Option<u64>,

    /// Whether this token is transferable
    pub transferable: bool,
}

impl RelayCredit {
    /// Create new relay credit token
    pub fn new(token_id: Vec<u8>, account_id: Vec<u8>, credits: u64, issued_at: u64) -> Self {
        Self {
            token_id,
            account_id,
            credits,
            issued_at,
            expires_at: None,
            transferable: false,
        }
    }

    /// Check if token is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expires_at
            .map_or(false, |expiry| current_time > expiry)
    }

    /// Consume credits
    pub fn consume(&mut self, amount: u64) -> Result<(), EconomicError> {
        if self.credits < amount {
            return Err(EconomicError::InsufficientCredits);
        }
        self.credits -= amount;
        Ok(())
    }

    /// Make token transferable
    pub fn make_transferable(mut self) -> Self {
        self.transferable = true;
        self
    }

    /// Set expiration
    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }
}

/// Storage quota offer in the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageQuotaOffer {
    /// Offer ID
    pub offer_id: Vec<u8>,

    /// Account offering storage
    pub provider_id: Vec<u8>,

    /// Amount of storage in bytes
    pub storage_bytes: u64,

    /// Price per GB per month (in credits)
    pub price_per_gb_month: u64,

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
        price_per_gb_month: u64,
        min_trust_level: TrustLevel,
        created_at: u64,
    ) -> Self {
        Self {
            offer_id,
            provider_id,
            storage_bytes,
            price_per_gb_month,
            min_trust_level,
            available: true,
            created_at,
        }
    }

    /// Calculate total cost
    pub fn calculate_cost(&self) -> u64 {
        let gb = (self.storage_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
        (gb * self.price_per_gb_month as f64) as u64
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

    /// Get relay credit allocation per month
    pub fn monthly_relay_credits(&self) -> u64 {
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

/// Micropayment for service delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Micropayment {
    /// Payment ID
    pub payment_id: Vec<u8>,

    /// Payer account
    pub from_account: Vec<u8>,

    /// Payee account
    pub to_account: Vec<u8>,

    /// Amount in credits
    pub amount: u64,

    /// Service being paid for
    pub service_type: ServiceType,

    /// Payment status
    pub status: PaymentStatus,

    /// Created timestamp
    pub created_at: u64,

    /// Completed timestamp
    pub completed_at: Option<u64>,
}

impl Micropayment {
    /// Create a new micropayment
    pub fn new(
        payment_id: Vec<u8>,
        from_account: Vec<u8>,
        to_account: Vec<u8>,
        amount: u64,
        service_type: ServiceType,
        created_at: u64,
    ) -> Self {
        Self {
            payment_id,
            from_account,
            to_account,
            amount,
            service_type,
            status: PaymentStatus::Pending,
            created_at,
            completed_at: None,
        }
    }

    /// Mark payment as completed
    pub fn complete(&mut self, current_time: u64) {
        self.status = PaymentStatus::Completed;
        self.completed_at = Some(current_time);
    }

    /// Mark payment as failed
    pub fn fail(&mut self, reason: String) {
        self.status = PaymentStatus::Failed(reason);
    }
}

/// Service type for payments
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    Relay,
    Storage,
    Bandwidth,
    PriorityDelivery,
}

/// Payment status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed(String),
}

/// Economic manager for the system
#[derive(Debug, Clone)]
pub struct EconomicManager {
    /// Relay credit balances
    relay_credits: HashMap<Vec<u8>, RelayCredit>,

    /// Storage quota offers
    storage_offers: HashMap<Vec<u8>, StorageQuotaOffer>,

    /// Reputation scores
    reputation_scores: HashMap<Vec<u8>, ReputationScore>,

    /// Pending micropayments
    pending_payments: HashMap<Vec<u8>, Micropayment>,
}

impl EconomicManager {
    /// Create a new economic manager
    pub fn new() -> Self {
        Self {
            relay_credits: HashMap::new(),
            storage_offers: HashMap::new(),
            reputation_scores: HashMap::new(),
            pending_payments: HashMap::new(),
        }
    }

    /// Issue relay credits to an account
    pub fn issue_relay_credits(
        &mut self,
        account_id: Vec<u8>,
        credits: u64,
        current_time: u64,
    ) -> RelayCredit {
        let token_id = self.generate_token_id(&account_id, current_time);
        let token = RelayCredit::new(token_id.clone(), account_id.clone(), credits, current_time);
        self.relay_credits.insert(token_id, token.clone());
        token
    }

    /// Transfer relay credits between accounts
    pub fn transfer_relay_credits(
        &mut self,
        from_account: &[u8],
        to_account: Vec<u8>,
        amount: u64,
        current_time: u64,
    ) -> Result<(), EconomicError> {
        // Find token for from_account
        let token_key = self
            .relay_credits
            .iter()
            .find(|(_, token)| token.account_id == from_account && token.transferable)
            .map(|(k, _)| k.clone())
            .ok_or(EconomicError::TokenNotFound)?;

        // Consume credits from source
        let token = self
            .relay_credits
            .get_mut(&token_key)
            .ok_or(EconomicError::TokenNotFound)?;
        token.consume(amount)?;

        // Issue new token to recipient
        self.issue_relay_credits(to_account, amount, current_time);

        Ok(())
    }

    /// Create a storage quota offer
    pub fn create_storage_offer(
        &mut self,
        provider_id: Vec<u8>,
        storage_bytes: u64,
        price_per_gb_month: u64,
        min_trust_level: TrustLevel,
        current_time: u64,
    ) -> StorageQuotaOffer {
        let offer_id = self.generate_token_id(&provider_id, current_time);
        let offer = StorageQuotaOffer::new(
            offer_id.clone(),
            provider_id,
            storage_bytes,
            price_per_gb_month,
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
        buyer_id: Vec<u8>,
        current_time: u64,
    ) -> Result<Micropayment, EconomicError> {
        // Get offer details and validate
        let (cost, provider_id) = {
            let offer = self
                .storage_offers
                .get(offer_id)
                .ok_or(EconomicError::OfferNotFound)?;

            if !offer.available {
                return Err(EconomicError::OfferUnavailable);
            }

            (offer.calculate_cost(), offer.provider_id.clone())
        };

        // Create micropayment
        let payment_id = self.generate_token_id(&buyer_id, current_time);
        let payment = Micropayment::new(
            payment_id.clone(),
            buyer_id,
            provider_id,
            cost,
            ServiceType::Storage,
            current_time,
        );

        // Fulfill offer
        let offer = self.storage_offers.get_mut(offer_id).unwrap();
        offer.fulfill();

        self.pending_payments.insert(payment_id, payment.clone());

        Ok(payment)
    }

    /// Complete a micropayment
    pub fn complete_payment(
        &mut self,
        payment_id: &[u8],
        current_time: u64,
    ) -> Result<(), EconomicError> {
        // Get payment details first
        let (from_account, to_account, amount) = {
            let payment = self
                .pending_payments
                .get(payment_id)
                .ok_or(EconomicError::PaymentNotFound)?;
            (
                payment.from_account.clone(),
                payment.to_account.clone(),
                payment.amount,
            )
        };

        // Transfer credits
        self.transfer_relay_credits(&from_account, to_account, amount, current_time)?;

        // Update payment status
        let payment = self
            .pending_payments
            .get_mut(payment_id)
            .ok_or(EconomicError::PaymentNotFound)?;
        payment.complete(current_time);

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

    /// Generate a unique token ID
    fn generate_token_id(&self, account_id: &[u8], timestamp: u64) -> Vec<u8> {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(account_id);
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(&(self.relay_credits.len() as u64).to_le_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}

impl Default for EconomicManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Economic system errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomicError {
    InsufficientCredits,
    TokenNotFound,
    OfferNotFound,
    OfferUnavailable,
    PaymentNotFound,
    TransferFailed,
}

impl std::fmt::Display for EconomicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EconomicError::InsufficientCredits => write!(f, "Insufficient relay credits"),
            EconomicError::TokenNotFound => write!(f, "Relay credit token not found"),
            EconomicError::OfferNotFound => write!(f, "Storage offer not found"),
            EconomicError::OfferUnavailable => write!(f, "Storage offer no longer available"),
            EconomicError::PaymentNotFound => write!(f, "Micropayment not found"),
            EconomicError::TransferFailed => write!(f, "Credit transfer failed"),
        }
    }
}

impl std::error::Error for EconomicError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_credit_creation() {
        let token = RelayCredit::new(vec![1], vec![2], 1000, 1000);
        assert_eq!(token.credits, 1000);
        assert!(!token.is_expired(2000));
        assert!(!token.transferable);
    }

    #[test]
    fn test_relay_credit_consumption() {
        let mut token = RelayCredit::new(vec![1], vec![2], 1000, 1000);

        token.consume(100).unwrap();
        assert_eq!(token.credits, 900);

        assert!(token.consume(1000).is_err());
    }

    #[test]
    fn test_relay_credit_expiration() {
        let token = RelayCredit::new(vec![1], vec![2], 1000, 1000).with_expiration(5000);

        assert!(!token.is_expired(4999));
        assert!(token.is_expired(5001));
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

        assert_eq!(offer.calculate_cost(), 500); // 5 GB * 100 credits/GB
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

        assert_eq!(ServiceTier::Free.monthly_relay_credits(), 1000);
        assert_eq!(ServiceTier::Premium.monthly_relay_credits(), 100_000);
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
    fn test_economic_manager_issue_credits() {
        let mut manager = EconomicManager::new();
        let account_id = vec![1];

        let token = manager.issue_relay_credits(account_id.clone(), 1000, 1000);
        assert_eq!(token.credits, 1000);
        assert_eq!(token.account_id, account_id);
    }

    #[test]
    fn test_economic_manager_create_offer() {
        let mut manager = EconomicManager::new();
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
    fn test_economic_manager_accept_offer() {
        let mut manager = EconomicManager::new();
        let provider_id = vec![1];
        let buyer_id = vec![2];

        // Issue credits to buyer
        let mut token = manager.issue_relay_credits(buyer_id.clone(), 1000, 1000);
        token.transferable = true;
        manager.relay_credits.insert(token.token_id.clone(), token);

        // Create offer
        let offer = manager.create_storage_offer(
            provider_id,
            1024 * 1024 * 1024,
            100,
            TrustLevel::Basic,
            1000,
        );

        // Accept offer
        let payment = manager
            .accept_storage_offer(&offer.offer_id, buyer_id, 1500)
            .unwrap();
        assert_eq!(payment.service_type, ServiceType::Storage);
        assert_eq!(payment.status, PaymentStatus::Pending);
    }

    #[test]
    fn test_economic_manager_reputation() {
        let mut manager = EconomicManager::new();
        let account_id = vec![1];

        manager.record_success(&account_id, 2000, 1000);
        manager.record_success(&account_id, 3000, 1000);

        let reputation = manager.get_reputation(&account_id, 1000);
        assert!(reputation.successful_interactions >= 2);
        assert!(reputation.score > 0.5);
    }

    #[test]
    fn test_micropayment_lifecycle() {
        let payment = Micropayment::new(vec![1], vec![2], vec![3], 100, ServiceType::Relay, 1000);

        assert_eq!(payment.status, PaymentStatus::Pending);
        assert_eq!(payment.amount, 100);

        let mut payment = payment;
        payment.complete(2000);
        assert_eq!(payment.status, PaymentStatus::Completed);
        assert_eq!(payment.completed_at, Some(2000));
    }
}
