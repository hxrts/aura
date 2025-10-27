//! Injectable effects for deterministic testing
//!
//! This module provides abstractions for side effects (time, randomness) that can be
//! swapped between real implementations and test/simulation implementations.
//!
//! This enables:
//! - Deterministic tests (same inputs → same outputs)
//! - Time travel debugging (step forward/backward through time)
//! - Reproducible simulations (with seed-based randomness)
//! - Fast-forward testing (skip ahead in logical time)

use crate::{CryptoError, Result};
use rand::rngs::StdRng;
use rand::{CryptoRng, RngCore, SeedableRng};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ========== Time Source Abstraction ==========

/// Abstract time source - can be real system time or simulated time
///
/// This trait allows injecting different time sources:
/// - Production: Real system time
/// - Testing: Simulated time that can be fast-forwarded
/// - Debugging: Time-travel capable source
pub trait TimeSource: Send + Sync {
    /// Get current Unix timestamp in seconds
    fn current_timestamp(&self) -> Result<u64>;

    /// Advance time by N seconds (no-op for real time, used in simulations)
    fn advance(&self, _seconds: u64) -> Result<()> {
        Ok(()) // Default: no-op for real time sources
    }

    /// Set absolute time (for time-travel debugging)
    fn set_time(&self, _timestamp: u64) -> Result<()> {
        Err(CryptoError::system_time_error(
            "Time travel not supported for this time source".to_string(),
        ))
    }

    /// Check if this is a simulated time source
    fn is_simulated(&self) -> bool {
        false // Default: real time sources
    }
}

/// Real system time source (production use)
#[derive(Debug, Clone, Default)]
pub struct SystemTimeSource;

impl SystemTimeSource {
    /// Create a new system time source
    pub fn new() -> Self {
        SystemTimeSource
    }
}

impl TimeSource for SystemTimeSource {
    fn current_timestamp(&self) -> Result<u64> {
        #[allow(clippy::disallowed_methods)]
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|e| {
                CryptoError::system_time_error(format!("System time is before UNIX epoch: {}", e))
            })
    }
}

/// Simulated time source (for testing and time-travel debugging)
///
/// Allows manual control of time progression for deterministic tests.
#[derive(Debug, Clone)]
pub struct SimulatedTimeSource {
    current_time: Arc<Mutex<u64>>,
}

impl SimulatedTimeSource {
    /// Create a new simulated time source starting at the given timestamp
    pub fn new(initial_timestamp: u64) -> Self {
        SimulatedTimeSource {
            current_time: Arc::new(Mutex::new(initial_timestamp)),
        }
    }

    /// Create starting at Unix epoch (1970-01-01 00:00:00)
    pub fn from_epoch() -> Self {
        Self::new(0)
    }

    /// Create starting at a recent time (for more realistic tests)
    pub fn from_recent() -> Self {
        // 2025-01-01 00:00:00 UTC
        Self::new(1735689600)
    }
}

impl TimeSource for SimulatedTimeSource {
    fn current_timestamp(&self) -> Result<u64> {
        let time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::system_time_error(format!("Lock poisoned: {}", e)))?;
        Ok(*time)
    }

    fn advance(&self, seconds: u64) -> Result<()> {
        let mut time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::system_time_error(format!("Lock poisoned: {}", e)))?;
        *time = time.saturating_add(seconds);
        Ok(())
    }

    fn set_time(&self, timestamp: u64) -> Result<()> {
        let mut time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::system_time_error(format!("Lock poisoned: {}", e)))?;
        *time = timestamp;
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }
}

// ========== Random Source Abstraction ==========

/// Abstract randomness source - can be real RNG or seeded/deterministic RNG
///
/// This trait allows injecting different randomness sources:
/// - Production: Cryptographically secure OS randomness
/// - Testing: Seeded deterministic RNG (reproducible)
/// - Debugging: Controllable randomness for specific scenarios
pub trait RandomSource: Send + Sync {
    /// Fill a byte buffer with random data
    fn fill_bytes(&self, dest: &mut [u8]);

    /// Generate a random u64
    fn gen_u64(&self) -> u64;

    /// Generate a UUID (v4 for production, deterministic for testing)
    fn gen_uuid(&self) -> Uuid;
}

/// Real randomness source using OS entropy (production use)
///
/// Uses `rand::thread_rng()` which provides cryptographically secure randomness.
#[derive(Debug, Clone, Default)]
pub struct OsRandomSource;

impl OsRandomSource {
    /// Create a new OS random source
    pub fn new() -> Self {
        OsRandomSource
    }
}

impl RandomSource for OsRandomSource {
    fn fill_bytes(&self, dest: &mut [u8]) {
        use rand::RngCore;
        #[allow(clippy::disallowed_methods)]
        rand::thread_rng().fill_bytes(dest);
    }

    fn gen_u64(&self) -> u64 {
        use rand::RngCore;
        #[allow(clippy::disallowed_methods)]
        rand::thread_rng().next_u64()
    }

    fn gen_uuid(&self) -> Uuid {
        #[allow(clippy::disallowed_methods)]
        Uuid::new_v4()
    }
}

/// Seeded deterministic RNG (for testing and reproducible simulations)
///
/// Uses ChaCha8 PRNG which is:
/// - Fast enough for simulations
/// - Deterministic (same seed → same sequence)
/// - Good statistical properties
#[derive(Debug, Clone)]
pub struct SeededRandomSource {
    // Interior mutability for RNG state
    rng: Arc<Mutex<StdRng>>,
}

impl SeededRandomSource {
    /// Create a new seeded RNG with the given seed
    pub fn new(seed: u64) -> Self {
        SeededRandomSource {
            rng: Arc::new(Mutex::new(StdRng::seed_from_u64(seed))),
        }
    }

    /// Create with seed 0 (default for reproducible tests)
    pub fn default_seed() -> Self {
        Self::new(0)
    }

    /// Create with a specific seed for test isolation
    pub fn from_test_name(test_name: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        test_name.hash(&mut hasher);
        Self::new(hasher.finish())
    }
}

impl RandomSource for SeededRandomSource {
    fn fill_bytes(&self, dest: &mut [u8]) {
        #[allow(clippy::expect_used)] // Mutex poisoning is unrecoverable
        let mut rng = self.rng.lock().expect("RNG lock poisoned");
        rng.fill_bytes(dest);
    }

    fn gen_u64(&self) -> u64 {
        #[allow(clippy::expect_used)] // Mutex poisoning is unrecoverable
        let mut rng = self.rng.lock().expect("RNG lock poisoned");
        rng.next_u64()
    }

    fn gen_uuid(&self) -> Uuid {
        // Generate deterministic UUID using seeded randomness
        let mut bytes = [0u8; 16];
        self.fill_bytes(&mut bytes);

        // Create UUID from random bytes (v4 format)
        #[allow(clippy::disallowed_methods)]
        Uuid::from_bytes(bytes)
    }
}

// ========== Helper Functions ==========

/// Generate random bytes into a fixed-size array from any RandomSource
///
/// This is a helper function since we can't have const generic methods in trait objects.
pub fn gen_random_bytes<const N: usize>(source: &dyn RandomSource) -> [u8; N] {
    let mut bytes = [0u8; N];
    source.fill_bytes(&mut bytes);
    bytes
}

// ========== RNG Adapter ==========

/// Adapter to make RandomSource compatible with rand crate traits
///
/// This struct wraps a RandomSource and implements RngCore + CryptoRng
/// so it can be used with functions that expect standard rand RNG types.
pub struct EffectsRng {
    source: Arc<dyn RandomSource>,
}

impl RngCore for EffectsRng {
    fn next_u32(&mut self) -> u32 {
        (self.source.gen_u64() >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.source.gen_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.source.fill_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EffectsRng {}

// ========== Effect Bundle ==========

/// Bundle of injectable effects
///
/// This struct holds all side effects that need to be controlled for testing.
/// Pass this to orchestrators and agents to enable deterministic behavior.
#[derive(Clone)]
pub struct Effects {
    /// Time source for timestamps
    pub time: Arc<dyn TimeSource>,
    /// Random number generator for cryptographic randomness
    pub random: Arc<dyn RandomSource>,
}

impl Effects {
    /// Create production effects (real time + OS randomness)
    pub fn production() -> Self {
        Effects {
            time: Arc::new(SystemTimeSource::new()),
            random: Arc::new(OsRandomSource::new()),
        }
    }

    /// Create deterministic test effects (simulated time + seeded RNG)
    pub fn deterministic(seed: u64, initial_time: u64) -> Self {
        Effects {
            time: Arc::new(SimulatedTimeSource::new(initial_time)),
            random: Arc::new(SeededRandomSource::new(seed)),
        }
    }

    /// Create test effects with default seed and recent time
    pub fn test() -> Self {
        Self::deterministic(0, 1735689600) // 2025-01-01
    }

    /// Create test effects isolated by test name
    pub fn for_test(test_name: &str) -> Self {
        Effects {
            time: Arc::new(SimulatedTimeSource::from_recent()),
            random: Arc::new(SeededRandomSource::from_test_name(test_name)),
        }
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self::production()
    }
}

// ========== Convenience Methods ==========

impl Effects {
    /// Get current timestamp
    pub fn now(&self) -> Result<u64> {
        self.time.current_timestamp()
    }

    /// Advance time by N seconds (simulation only)
    pub fn advance_time(&self, seconds: u64) -> Result<()> {
        self.time.advance(seconds)
    }

    /// Jump to specific time (time-travel debugging)
    pub fn set_time(&self, timestamp: u64) -> Result<()> {
        self.time.set_time(timestamp)
    }

    /// Generate random bytes
    pub fn random_bytes<const N: usize>(&self) -> [u8; N] {
        gen_random_bytes(self.random.as_ref())
    }

    /// Fill buffer with random bytes
    pub fn fill_random(&self, dest: &mut [u8]) {
        self.random.fill_bytes(dest);
    }

    /// Generate a UUID (deterministic in tests)
    pub fn gen_uuid(&self) -> Uuid {
        self.random.gen_uuid()
    }

    /// Generate session ID (convenience method)
    pub fn gen_session_id(&self) -> Uuid {
        self.gen_uuid()
    }

    /// Check if running in simulation mode
    pub fn is_simulated(&self) -> bool {
        self.time.is_simulated()
    }

    /// Get an RNG adapter that implements standard rand traits
    /// This is needed for functions that expect `impl Rng` parameters
    pub fn rng(&self) -> EffectsRng {
        EffectsRng {
            source: self.random.clone(),
        }
    }

    /// Async delay - replaces tokio::time::sleep in production
    /// In simulation, this should yield to the scheduler
    pub async fn delay(&self, duration: Duration) {
        if self.is_simulated() {
            // In simulation, advance simulated time instead of real sleep
            let _ = self.time.advance(duration.as_secs());
        } else {
            // In production, use real async sleep
            tokio::time::sleep(duration).await;
        }
    }
}

// ========== Monitoring Extensions ==========

/// Event types that can be recorded during execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    /// Time-related event
    TimeAdvanced {
        /// Starting timestamp
        from: u64,
        /// Ending timestamp
        to: u64,
    },
    /// Time was set to specific value
    TimeSet {
        /// The timestamp that was set
        timestamp: u64,
    },
    /// Random data was generated
    RandomGenerated {
        /// Number of bytes generated
        bytes_count: usize,
    },
    /// UUID was generated
    UuidGenerated {
        /// The generated UUID
        uuid: Uuid,
    },
    /// Custom protocol event
    ProtocolEvent {
        /// Type of the protocol event
        event_type: String,
        /// Additional details about the event
        details: String,
    },
    /// Property evaluation event
    PropertyEvaluated {
        /// Name of the property that was evaluated
        property_name: String,
        /// Result of the property evaluation
        result: bool,
    },
    /// Error occurred during operation
    ErrorOccurred {
        /// Error message
        error: String,
    },
}

/// Collects execution traces for debugging and analysis
#[derive(Debug, Clone)]
pub struct TraceCollector {
    events: Arc<Mutex<Vec<TraceEvent>>>,
    enabled: bool,
}

impl TraceCollector {
    /// Create a new trace collector
    pub fn new() -> Self {
        TraceCollector {
            events: Arc::new(Mutex::new(Vec::new())),
            enabled: true,
        }
    }

    /// Create a disabled trace collector (no-op)
    pub fn disabled() -> Self {
        TraceCollector {
            events: Arc::new(Mutex::new(Vec::new())),
            enabled: false,
        }
    }

    /// Record an event
    pub fn record(&self, event: TraceEvent) {
        if !self.enabled {
            return;
        }

        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    /// Get all recorded events
    pub fn get_events(&self) -> Vec<TraceEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    /// Clear all recorded events
    pub fn clear(&self) {
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
    }

    /// Get the number of recorded events
    pub fn event_count(&self) -> usize {
        self.events.lock().map(|events| events.len()).unwrap_or(0)
    }

    /// Check if trace collection is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable trace collection
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Property monitoring result
#[derive(Debug, Clone)]
pub struct PropertyResult {
    /// Name of the property
    pub property_name: String,
    /// Whether the property holds
    pub holds: bool,
    /// Timestamp when evaluated
    pub timestamp: u64,
    /// Additional details or error message
    pub details: Option<String>,
}

/// Basic property monitor for continuous validation
#[derive(Debug, Clone)]
pub struct PropertyMonitor {
    properties: Arc<Mutex<Vec<PropertyResult>>>,
    enabled: bool,
}

impl PropertyMonitor {
    /// Create a new property monitor
    pub fn new() -> Self {
        PropertyMonitor {
            properties: Arc::new(Mutex::new(Vec::new())),
            enabled: true,
        }
    }

    /// Create a disabled property monitor (no-op)
    pub fn disabled() -> Self {
        PropertyMonitor {
            properties: Arc::new(Mutex::new(Vec::new())),
            enabled: false,
        }
    }

    /// Check a property and record the result
    pub fn check_property<F>(&self, name: &str, timestamp: u64, checker: F) -> bool
    where
        F: FnOnce() -> bool,
    {
        if !self.enabled {
            return true; // Assume properties hold when monitoring is disabled
        }

        let holds = checker();
        let result = PropertyResult {
            property_name: name.to_string(),
            holds,
            timestamp,
            details: None,
        };

        if let Ok(mut properties) = self.properties.lock() {
            properties.push(result);
        }

        holds
    }

    /// Check a property with custom details
    pub fn check_property_with_details<F>(
        &self,
        name: &str,
        timestamp: u64,
        details: String,
        checker: F,
    ) -> bool
    where
        F: FnOnce() -> bool,
    {
        if !self.enabled {
            return true;
        }

        let holds = checker();
        let result = PropertyResult {
            property_name: name.to_string(),
            holds,
            timestamp,
            details: Some(details),
        };

        if let Ok(mut properties) = self.properties.lock() {
            properties.push(result);
        }

        holds
    }

    /// Get all property check results
    pub fn get_results(&self) -> Vec<PropertyResult> {
        self.properties
            .lock()
            .map(|properties| properties.clone())
            .unwrap_or_default()
    }

    /// Get failed property checks
    pub fn get_failures(&self) -> Vec<PropertyResult> {
        self.get_results()
            .into_iter()
            .filter(|result| !result.holds)
            .collect()
    }

    /// Clear all property results
    pub fn clear(&self) {
        if let Ok(mut properties) = self.properties.lock() {
            properties.clear();
        }
    }

    /// Get the number of property checks
    pub fn check_count(&self) -> usize {
        self.properties
            .lock()
            .map(|properties| properties.len())
            .unwrap_or(0)
    }

    /// Get the number of failed property checks
    pub fn failure_count(&self) -> usize {
        self.properties
            .lock()
            .map(|properties| properties.iter().filter(|r| !r.holds).count())
            .unwrap_or(0)
    }

    /// Check if monitoring is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable property monitoring
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl Default for PropertyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Monitored wrapper around the Effects bundle
///
/// This extends the basic Effects with monitoring capabilities including
/// trace collection and property checking for debugging and analysis.
#[derive(Clone)]
pub struct MonitoredEffects {
    /// Core effects bundle
    pub effects: Effects,
    /// Trace collector for execution events
    pub trace: TraceCollector,
    /// Property monitor for continuous validation
    pub monitor: PropertyMonitor,
}

impl MonitoredEffects {
    /// Create monitored production effects
    pub fn production() -> Self {
        MonitoredEffects {
            effects: Effects::production(),
            trace: TraceCollector::disabled(), // Disable tracing in production by default
            monitor: PropertyMonitor::disabled(), // Disable monitoring in production by default
        }
    }

    /// Create monitored deterministic test effects
    pub fn deterministic(seed: u64, initial_time: u64) -> Self {
        MonitoredEffects {
            effects: Effects::deterministic(seed, initial_time),
            trace: TraceCollector::new(),
            monitor: PropertyMonitor::new(),
        }
    }

    /// Create monitored test effects with default seed and recent time
    pub fn test() -> Self {
        Self::deterministic(0, 1735689600) // 2025-01-01
    }

    /// Create monitored test effects isolated by test name
    pub fn for_test(test_name: &str) -> Self {
        MonitoredEffects {
            effects: Effects::for_test(test_name),
            trace: TraceCollector::new(),
            monitor: PropertyMonitor::new(),
        }
    }

    /// Create from existing effects with monitoring enabled
    pub fn with_monitoring(effects: Effects) -> Self {
        MonitoredEffects {
            effects,
            trace: TraceCollector::new(),
            monitor: PropertyMonitor::new(),
        }
    }

    /// Create from existing effects with monitoring disabled
    pub fn without_monitoring(effects: Effects) -> Self {
        MonitoredEffects {
            effects,
            trace: TraceCollector::disabled(),
            monitor: PropertyMonitor::disabled(),
        }
    }
}

// Delegate all Effects methods with monitoring
impl MonitoredEffects {
    /// Get current timestamp (with trace recording)
    pub fn now(&self) -> Result<u64> {
        let timestamp = self.effects.now()?;
        // Don't trace every time read, as it's too noisy
        Ok(timestamp)
    }

    /// Advance time by N seconds (with trace recording)
    pub fn advance_time(&self, seconds: u64) -> Result<()> {
        let from = self.effects.now().unwrap_or(0);
        self.effects.advance_time(seconds)?;
        let to = self.effects.now().unwrap_or(0);

        self.trace.record(TraceEvent::TimeAdvanced { from, to });
        Ok(())
    }

    /// Jump to specific time (with trace recording)
    pub fn set_time(&self, timestamp: u64) -> Result<()> {
        self.effects.set_time(timestamp)?;
        self.trace.record(TraceEvent::TimeSet { timestamp });
        Ok(())
    }

    /// Generate random bytes (with trace recording)
    pub fn random_bytes<const N: usize>(&self) -> [u8; N] {
        let bytes = self.effects.random_bytes();
        self.trace
            .record(TraceEvent::RandomGenerated { bytes_count: N });
        bytes
    }

    /// Fill buffer with random bytes (with trace recording)
    pub fn fill_random(&self, dest: &mut [u8]) {
        self.effects.fill_random(dest);
        self.trace.record(TraceEvent::RandomGenerated {
            bytes_count: dest.len(),
        });
    }

    /// Generate a UUID (with trace recording)
    pub fn gen_uuid(&self) -> Uuid {
        let uuid = self.effects.gen_uuid();
        self.trace.record(TraceEvent::UuidGenerated { uuid });
        uuid
    }

    /// Generate session ID (convenience method with trace recording)
    pub fn gen_session_id(&self) -> Uuid {
        self.gen_uuid()
    }

    /// Check if running in simulation mode
    pub fn is_simulated(&self) -> bool {
        self.effects.is_simulated()
    }

    /// Get an RNG adapter that implements standard rand traits
    pub fn rng(&self) -> EffectsRng {
        self.effects.rng()
    }

    /// Async delay (with trace recording in simulation mode)
    pub async fn delay(&self, duration: Duration) {
        if self.is_simulated() {
            let from = self.now().unwrap_or(0);
            self.effects.delay(duration).await;
            let to = self.now().unwrap_or(0);
            self.trace.record(TraceEvent::TimeAdvanced { from, to });
        } else {
            self.effects.delay(duration).await;
        }
    }

    /// Record a custom protocol event
    pub fn record_protocol_event(&self, event_type: &str, details: &str) {
        self.trace.record(TraceEvent::ProtocolEvent {
            event_type: event_type.to_string(),
            details: details.to_string(),
        });
    }

    /// Record an error event
    pub fn record_error(&self, error: &str) {
        self.trace.record(TraceEvent::ErrorOccurred {
            error: error.to_string(),
        });
    }

    /// Check a property and record the result
    pub fn check_property<F>(&self, name: &str, checker: F) -> bool
    where
        F: FnOnce() -> bool,
    {
        let timestamp = self.now().unwrap_or(0);
        let result = self.monitor.check_property(name, timestamp, checker);

        self.trace.record(TraceEvent::PropertyEvaluated {
            property_name: name.to_string(),
            result,
        });

        result
    }

    /// Check a property with custom details
    pub fn check_property_with_details<F>(&self, name: &str, details: String, checker: F) -> bool
    where
        F: FnOnce() -> bool,
    {
        let timestamp = self.now().unwrap_or(0);
        let result = self
            .monitor
            .check_property_with_details(name, timestamp, details, checker);

        self.trace.record(TraceEvent::PropertyEvaluated {
            property_name: name.to_string(),
            result,
        });

        result
    }

    /// Get access to the underlying Effects bundle
    pub fn inner(&self) -> &Effects {
        &self.effects
    }

    /// Enable or disable trace collection
    pub fn set_trace_enabled(&mut self, enabled: bool) {
        self.trace.set_enabled(enabled);
    }

    /// Enable or disable property monitoring
    pub fn set_monitor_enabled(&mut self, enabled: bool) {
        self.monitor.set_enabled(enabled);
    }

    /// Get monitoring statistics
    pub fn get_stats(&self) -> MonitoringStats {
        MonitoringStats {
            trace_events: self.trace.event_count(),
            property_checks: self.monitor.check_count(),
            property_failures: self.monitor.failure_count(),
            trace_enabled: self.trace.is_enabled(),
            monitor_enabled: self.monitor.is_enabled(),
        }
    }
}

impl Default for MonitoredEffects {
    fn default() -> Self {
        Self::production()
    }
}

/// Statistics about monitoring activity
#[derive(Debug, Clone)]
pub struct MonitoringStats {
    /// Number of trace events recorded
    pub trace_events: usize,
    /// Number of property checks performed
    pub property_checks: usize,
    /// Number of property check failures
    pub property_failures: usize,
    /// Whether trace collection is enabled
    pub trace_enabled: bool,
    /// Whether property monitoring is enabled
    pub monitor_enabled: bool,
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    #[test]
    fn test_system_time_source() {
        let time_source = SystemTimeSource::new();
        let t1 = time_source.current_timestamp().unwrap();

        // Should be after 2020-01-01 (1577836800)
        assert!(t1 > 1577836800);

        // advance() should be no-op for real time
        assert!(time_source.advance(1000).is_ok());

        // Time should still be real time (not advanced)
        let t2 = time_source.current_timestamp().unwrap();
        assert!(t2 >= t1 && t2 < t1 + 100); // Allow some real time passage
    }

    #[test]
    fn test_simulated_time_source() {
        let time_source = SimulatedTimeSource::new(1000);

        assert_eq!(time_source.current_timestamp().unwrap(), 1000);

        // Advance time
        time_source.advance(500).unwrap();
        assert_eq!(time_source.current_timestamp().unwrap(), 1500);

        // Time travel
        time_source.set_time(2000).unwrap();
        assert_eq!(time_source.current_timestamp().unwrap(), 2000);
    }

    #[test]
    fn test_os_random_source() {
        let rng = OsRandomSource::new();

        let bytes1: [u8; 32] = gen_random_bytes(&rng);
        let bytes2: [u8; 32] = gen_random_bytes(&rng);

        // Should be different (with overwhelming probability)
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_seeded_random_source_deterministic() {
        let rng1 = SeededRandomSource::new(42);
        let rng2 = SeededRandomSource::new(42);

        let bytes1: [u8; 32] = gen_random_bytes(&rng1);
        let bytes2: [u8; 32] = gen_random_bytes(&rng2);

        // Same seed → same output
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_seeded_random_source_different_seeds() {
        let rng1 = SeededRandomSource::new(42);
        let rng2 = SeededRandomSource::new(43);

        let bytes1: [u8; 32] = gen_random_bytes(&rng1);
        let bytes2: [u8; 32] = gen_random_bytes(&rng2);

        // Different seeds → different output
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_effects_production() {
        let effects = Effects::production();

        let t1 = effects.now().unwrap();
        assert!(t1 > 1577836800); // After 2020

        let bytes: [u8; 16] = effects.random_bytes();
        assert_ne!(bytes, [0u8; 16]); // Should be random
    }

    #[test]
    fn test_effects_deterministic() {
        let effects1 = Effects::deterministic(123, 1000);
        let effects2 = Effects::deterministic(123, 1000);

        // Same seed + time → same behavior
        assert_eq!(effects1.now().unwrap(), effects2.now().unwrap());

        let bytes1: [u8; 32] = effects1.random_bytes();
        let bytes2: [u8; 32] = effects2.random_bytes();
        assert_eq!(bytes1, bytes2);

        // UUIDs should also be deterministic
        let uuid1 = effects1.gen_uuid();
        let uuid2 = effects2.gen_uuid();
        assert_eq!(uuid1, uuid2);
    }

    #[test]
    fn test_effects_time_travel() {
        let effects = Effects::test();

        let t1 = effects.now().unwrap();
        effects.advance_time(3600).unwrap(); // +1 hour
        let t2 = effects.now().unwrap();
        assert_eq!(t2, t1 + 3600);

        effects.set_time(1000).unwrap(); // Jump to specific time
        assert_eq!(effects.now().unwrap(), 1000);
    }

    #[test]
    fn test_from_test_name_isolation() {
        let effects1 = Effects::for_test("test_foo");
        let effects2 = Effects::for_test("test_bar");

        // Different test names → different seeds
        let bytes1: [u8; 32] = effects1.random_bytes();
        let bytes2: [u8; 32] = effects2.random_bytes();
        assert_ne!(bytes1, bytes2);

        // Same test name → same seed
        let effects3 = Effects::for_test("test_foo");
        let bytes3: [u8; 32] = effects3.random_bytes();
        assert_eq!(bytes1, bytes3);

        // UUIDs should also be isolated by test name
        let uuid1 = effects1.gen_uuid();
        let uuid2 = effects2.gen_uuid();
        assert_ne!(uuid1, uuid2);

        let uuid3 = effects3.gen_uuid();
        assert_eq!(uuid1, uuid3);
    }

    #[test]
    fn test_trace_collector() {
        let mut collector = TraceCollector::new();
        assert!(collector.is_enabled());
        assert_eq!(collector.event_count(), 0);

        // Record some events
        collector.record(TraceEvent::TimeAdvanced { from: 100, to: 200 });
        collector.record(TraceEvent::RandomGenerated { bytes_count: 32 });

        assert_eq!(collector.event_count(), 2);

        let events = collector.get_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], TraceEvent::TimeAdvanced { from: 100, to: 200 });
        assert_eq!(events[1], TraceEvent::RandomGenerated { bytes_count: 32 });

        // Clear events
        collector.clear();
        assert_eq!(collector.event_count(), 0);

        // Test disabled collector
        collector.set_enabled(false);
        collector.record(TraceEvent::TimeSet { timestamp: 300 });
        assert_eq!(collector.event_count(), 0); // Should not record when disabled
    }

    #[test]
    fn test_property_monitor() {
        let monitor = PropertyMonitor::new();
        assert!(monitor.is_enabled());
        assert_eq!(monitor.check_count(), 0);
        assert_eq!(monitor.failure_count(), 0);

        // Check properties
        let result1 = monitor.check_property("test_prop_1", 1000, || true);
        assert!(result1);

        let result2 = monitor.check_property("test_prop_2", 2000, || false);
        assert!(!result2);

        assert_eq!(monitor.check_count(), 2);
        assert_eq!(monitor.failure_count(), 1);

        let results = monitor.get_results();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].property_name, "test_prop_1");
        assert!(results[0].holds);
        assert_eq!(results[1].property_name, "test_prop_2");
        assert!(!results[1].holds);

        let failures = monitor.get_failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].property_name, "test_prop_2");

        // Test property with details
        let result3 = monitor.check_property_with_details(
            "test_prop_3",
            3000,
            "Custom details".to_string(),
            || true,
        );
        assert!(result3);
        assert_eq!(monitor.check_count(), 3);

        let all_results = monitor.get_results();
        assert_eq!(all_results[2].details, Some("Custom details".to_string()));
    }

    #[test]
    fn test_monitored_effects() {
        let effects = MonitoredEffects::test();

        // Test time operations with tracing
        let t1 = effects.now().unwrap();
        effects.advance_time(3600).unwrap(); // +1 hour
        let t2 = effects.now().unwrap();
        assert_eq!(t2, t1 + 3600);

        // Test random operations with tracing
        let _bytes: [u8; 32] = effects.random_bytes();
        let _uuid = effects.gen_uuid();

        // Test property checking
        let prop_result1 = effects.check_property("test_invariant", || true);
        assert!(prop_result1);

        let prop_result2 = effects.check_property("failing_property", || false);
        assert!(!prop_result2);

        // Test protocol event recording
        effects.record_protocol_event("dkd_start", "Starting DKD protocol");
        effects.record_error("Test error occurred");

        // Check statistics
        let stats = effects.get_stats();
        assert!(stats.trace_events > 0);
        assert_eq!(stats.property_checks, 2);
        assert_eq!(stats.property_failures, 1);
        assert!(stats.trace_enabled);
        assert!(stats.monitor_enabled);

        // Check trace events
        let events = effects.trace.get_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::TimeAdvanced { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::RandomGenerated { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::UuidGenerated { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::PropertyEvaluated { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::ProtocolEvent { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, TraceEvent::ErrorOccurred { .. })));

        // Check property results
        let prop_results = effects.monitor.get_results();
        assert_eq!(prop_results.len(), 2);
        assert_eq!(prop_results[0].property_name, "test_invariant");
        assert!(prop_results[0].holds);
        assert_eq!(prop_results[1].property_name, "failing_property");
        assert!(!prop_results[1].holds);
    }

    #[test]
    fn test_monitored_effects_disabled() {
        let effects = MonitoredEffects::production();

        // Production effects should have monitoring disabled by default
        let stats = effects.get_stats();
        assert!(!stats.trace_enabled);
        assert!(!stats.monitor_enabled);

        // Operations should still work but not record events
        let _t = effects.now().unwrap();
        let _bytes: [u8; 16] = effects.random_bytes();
        let _result = effects.check_property("test_prop", || true);

        let stats_after = effects.get_stats();
        assert_eq!(stats_after.trace_events, 0);
        assert_eq!(stats_after.property_checks, 0);
    }

    #[test]
    fn test_monitored_effects_enable_disable() {
        let mut effects = MonitoredEffects::test();

        // Perform some operations
        effects.advance_time(100).unwrap();
        effects.check_property("prop1", || true);

        let stats1 = effects.get_stats();
        assert!(stats1.trace_events > 0);
        assert_eq!(stats1.property_checks, 1);

        // Disable monitoring
        effects.set_trace_enabled(false);
        effects.set_monitor_enabled(false);

        // Perform more operations
        effects.advance_time(100).unwrap();
        effects.check_property("prop2", || true);

        let stats2 = effects.get_stats();
        assert_eq!(stats2.trace_events, stats1.trace_events); // No new trace events
        assert_eq!(stats2.property_checks, stats1.property_checks); // No new property checks
        assert!(!stats2.trace_enabled);
        assert!(!stats2.monitor_enabled);
    }

    #[test]
    fn test_monitored_effects_from_existing() {
        let base_effects = Effects::deterministic(42, 1000);

        let monitored_with = MonitoredEffects::with_monitoring(base_effects.clone());
        let monitored_without = MonitoredEffects::without_monitoring(base_effects);

        assert!(monitored_with.trace.is_enabled());
        assert!(monitored_with.monitor.is_enabled());

        assert!(!monitored_without.trace.is_enabled());
        assert!(!monitored_without.monitor.is_enabled());
    }
}

// ========== EffectsLike Trait Implementation ==========
// Implement aura_types::EffectsLike for Effects to support ID generation with Effects

impl aura_types::EffectsLike for Effects {
    fn gen_uuid(&self) -> Uuid {
        self.gen_uuid()
    }
}
