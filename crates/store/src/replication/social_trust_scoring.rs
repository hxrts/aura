//! Trust Score Computation for Peers
//!
//! Maintains and computes trust scores based on peer interaction history.
//! Scores are time-weighted to prioritize recent interactions and
//! gradually forget past failures.
//!
//! Reference: work/ssb_storage.md Phase 6.1

use serde::{Deserialize, Serialize};

/// Trust score based on peer interaction history
///
/// Represents cumulative reliability assessment of a peer based on:
/// - Successful storage operations
/// - Failed operations
/// - Response time performance
/// - Uptime measurements
/// - Relationship age
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    /// Number of successful storage operations
    pub successful_operations: u64,

    /// Number of failed storage operations
    pub failed_operations: u64,

    /// Average response time in milliseconds
    pub avg_response_time_ms: u32,

    /// Uptime percentage (0.0-1.0)
    pub uptime_ratio: f64,

    /// Time-weighted reliability score (0.0-1.0)
    /// Recent interactions weighted more heavily
    pub reliability_score: f64,

    /// Last successful interaction timestamp
    pub last_success_timestamp: u64,

    /// Last failure timestamp
    pub last_failure_timestamp: u64,

    /// Relationship age in days
    pub relationship_age_days: u32,

    /// Creation timestamp for decay calculations
    created_at: u64,
}

impl TrustScore {
    /// Create a new trust score for a new peer
    pub fn new(current_time: u64) -> Self {
        Self {
            successful_operations: 0,
            failed_operations: 0,
            avg_response_time_ms: 0,
            uptime_ratio: 1.0,
            reliability_score: 1.0, // New peers start with benefit of doubt
            last_success_timestamp: current_time,
            last_failure_timestamp: 0,
            relationship_age_days: 0,
            created_at: current_time,
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self, timestamp: u64) {
        self.successful_operations += 1;
        self.last_success_timestamp = timestamp;
        self.update_reliability_score(timestamp);
    }

    /// Record a failed operation
    pub fn record_failure(&mut self, timestamp: u64) {
        self.failed_operations += 1;
        self.last_failure_timestamp = timestamp;
        self.update_reliability_score(timestamp);
    }

    /// Record response time sample (in milliseconds)
    pub fn record_response_time(&mut self, response_time_ms: u32) {
        if self.avg_response_time_ms == 0 {
            self.avg_response_time_ms = response_time_ms;
        } else {
            // Exponential moving average
            self.avg_response_time_ms =
                (self.avg_response_time_ms as u64 * 3 + response_time_ms as u64) as u32 / 4;
        }
    }

    /// Update reliability score based on history
    ///
    /// Score factors:
    /// - Success rate (base)
    /// - Time decay (weight recent interactions more)
    /// - Failure penalty (penalize recent failures more heavily)
    fn update_reliability_score(&mut self, current_time: u64) {
        let total = self.successful_operations + self.failed_operations;

        if total == 0 {
            // New peer - use optimistic initial score
            self.reliability_score = 0.8;
            return;
        }

        // Base success rate
        let success_rate = self.successful_operations as f64 / total as f64;

        // Failure recency penalty
        let time_since_failure = if self.last_failure_timestamp == 0 {
            u64::MAX
        } else {
            current_time.saturating_sub(self.last_failure_timestamp)
        };

        let failure_penalty = if time_since_failure < 3600 {
            // Recent failure (< 1 hour): harsh penalty
            0.5
        } else if time_since_failure < 86400 {
            // Recent failure (< 1 day): moderate penalty
            0.3
        } else if time_since_failure < 604800 {
            // Recent failure (< 1 week): small penalty
            0.1
        } else {
            // Older failure: minimal penalty
            0.01
        };

        // Success recency bonus
        let time_since_success = current_time.saturating_sub(self.last_success_timestamp);
        let recency_bonus = if time_since_success < 3600 {
            // Recent success: bonus
            0.2
        } else if time_since_success < 86400 {
            // Success within a day: small bonus
            0.1
        } else {
            0.0
        };

        // Compute reliability score
        self.reliability_score = (success_rate * (1.0 - failure_penalty) + recency_bonus)
            .min(1.0)
            .max(0.0);

        // Update relationship age
        let age_seconds = current_time.saturating_sub(self.created_at);
        self.relationship_age_days = (age_seconds / 86400) as u32;
    }

    /// Get success rate as percentage (0.0-1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.successful_operations + self.failed_operations;
        if total == 0 {
            1.0
        } else {
            self.successful_operations as f64 / total as f64
        }
    }

    /// Get failure rate as percentage (0.0-1.0)
    pub fn failure_rate(&self) -> f64 {
        1.0 - self.success_rate()
    }

    /// Check if peer is in acceptable state (reliability > threshold)
    pub fn is_acceptable(&self, threshold: f64) -> bool {
        self.reliability_score >= threshold
    }

    /// Get summary statistics
    pub fn summary(&self) -> TrustSummary {
        TrustSummary {
            successful_operations: self.successful_operations,
            failed_operations: self.failed_operations,
            success_rate: self.success_rate(),
            reliability_score: self.reliability_score,
            avg_response_time_ms: self.avg_response_time_ms,
            uptime_ratio: self.uptime_ratio,
            relationship_age_days: self.relationship_age_days,
        }
    }
}

/// Summary of trust score metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustSummary {
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub success_rate: f64,
    pub reliability_score: f64,
    pub avg_response_time_ms: u32,
    pub uptime_ratio: f64,
    pub relationship_age_days: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_score_creation() {
        let score = TrustScore::new(1000);
        assert_eq!(score.successful_operations, 0);
        assert_eq!(score.failed_operations, 0);
        assert!(score.reliability_score > 0.0);
    }

    #[test]
    fn test_record_success() {
        let mut score = TrustScore::new(1000);
        score.record_success(1000);

        assert_eq!(score.successful_operations, 1);
        assert_eq!(score.last_success_timestamp, 1000);
    }

    #[test]
    fn test_record_failure() {
        let mut score = TrustScore::new(1000);
        score.record_failure(1000);

        assert_eq!(score.failed_operations, 1);
        assert_eq!(score.last_failure_timestamp, 1000);
    }

    #[test]
    fn test_success_rate() {
        let mut score = TrustScore::new(1000);

        score.record_success(1000);
        score.record_success(1001);
        score.record_failure(1002);

        assert!((score.success_rate() - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_reliability_score_with_recent_success() {
        let mut score = TrustScore::new(1000);
        score.record_success(2000);
        score.record_success(2100);

        assert!(score.reliability_score > 0.8);
    }

    #[test]
    fn test_reliability_score_with_recent_failure() {
        let mut score = TrustScore::new(1000);
        score.record_success(1000);
        score.record_success(1001);
        score.record_failure(2000); // Recent failure

        assert!(score.reliability_score < 0.8);
    }

    #[test]
    fn test_record_response_time() {
        let mut score = TrustScore::new(1000);

        score.record_response_time(100);
        assert_eq!(score.avg_response_time_ms, 100);

        score.record_response_time(200);
        // EMA: (100 * 3 + 200) / 4 = 125
        assert_eq!(score.avg_response_time_ms, 125);
    }

    #[test]
    fn test_is_acceptable() {
        let mut score = TrustScore::new(1000);
        score.record_success(1000);

        // After one success, reliability score should be high (around 1.0)
        assert!(score.is_acceptable(0.5));
        assert!(score.is_acceptable(0.99));
        
        // Test with a peer that has failures
        let mut failing_score = TrustScore::new(2000);
        failing_score.record_success(2000);
        failing_score.record_failure(2001);
        failing_score.record_failure(2002);
        
        // Should still be acceptable at low threshold but not high threshold
        assert!(failing_score.is_acceptable(0.3));
        assert!(!failing_score.is_acceptable(0.9));
    }

    #[test]
    fn test_summary() {
        let mut score = TrustScore::new(1000);
        score.record_success(1000);
        score.record_failure(1001);

        let summary = score.summary();
        assert_eq!(summary.successful_operations, 1);
        assert_eq!(summary.failed_operations, 1);
        assert!(summary.success_rate > 0.0);
    }

    #[test]
    fn test_relationship_age() {
        let mut score = TrustScore::new(1000);
        score.record_success(1000 + 86400 * 7); // 7 days later

        assert!(score.relationship_age_days >= 6); // May be 6 or 7 depending on rounding
    }
}
