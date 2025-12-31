//! Demo Mode Configuration
//!
//! Portable configuration for simulated agent behavior in demo mode.
//! These constants and types ensure consistent demo behavior across frontends.

// ============================================================================
// Response Delay Defaults
// ============================================================================

/// Default minimum response delay in milliseconds.
pub const DEFAULT_RESPONSE_DELAY_MIN_MS: u64 = 1000;

/// Default maximum response delay in milliseconds.
pub const DEFAULT_RESPONSE_DELAY_MAX_MS: u64 = 5000;

/// Quick agent minimum delay (for faster demo pacing).
pub const QUICK_RESPONSE_DELAY_MIN_MS: u64 = 1500;

/// Quick agent maximum delay.
pub const QUICK_RESPONSE_DELAY_MAX_MS: u64 = 3000;

/// Deliberate agent minimum delay (for more thoughtful agents).
pub const DELIBERATE_RESPONSE_DELAY_MIN_MS: u64 = 2000;

/// Deliberate agent maximum delay.
pub const DELIBERATE_RESPONSE_DELAY_MAX_MS: u64 = 4000;

// ============================================================================
// Approval Probability Defaults
// ============================================================================

/// Default approval probability for recovery requests (0.0-1.0).
pub const DEFAULT_APPROVAL_PROBABILITY: f64 = 0.95;

/// High reliability approval probability.
pub const HIGH_APPROVAL_PROBABILITY: f64 = 0.98;

/// Moderate reliability approval probability.
pub const MODERATE_APPROVAL_PROBABILITY: f64 = 0.90;

// ============================================================================
// Message Frequency Defaults
// ============================================================================

/// Default message generation frequency in milliseconds.
pub const DEFAULT_MESSAGE_FREQUENCY_MS: u64 = 10_000;

/// Chatty agent message frequency.
pub const CHATTY_MESSAGE_FREQUENCY_MS: u64 = 15_000;

/// Quiet agent message frequency.
pub const QUIET_MESSAGE_FREQUENCY_MS: u64 = 20_000;

// ============================================================================
// Personality Defaults
// ============================================================================

/// Default chattiness level (0.0-1.0).
/// Represents probability of responding to a message.
pub const DEFAULT_CHATTINESS: f64 = 0.3;

/// Chatty personality chattiness level.
pub const CHATTY_CHATTINESS: f64 = 0.4;

/// Quiet personality chattiness level.
pub const QUIET_CHATTINESS: f64 = 0.25;

// ============================================================================
// Demo Seed
// ============================================================================

/// Default demo mode seed for deterministic behavior.
pub const DEFAULT_DEMO_SEED: u64 = 42;

/// Demo seed for the year 2024 scenarios.
pub const DEMO_SEED_2024: u64 = 2024;

// ============================================================================
// Recovery Threshold Defaults
// ============================================================================

/// Default threshold for demo mode (2-of-3).
pub const DEFAULT_DEMO_THRESHOLD: u32 = 2;

/// Default guardian count for demo mode.
pub const DEFAULT_DEMO_GUARDIAN_COUNT: u32 = 3;

// ============================================================================
// Demo Scenario Seeds
// ============================================================================

/// Seed for happy path demo scenario.
pub const DEMO_SEED_HAPPY_PATH: &str = "demo:bob:happy";

/// Seed for slow guardian demo scenario.
pub const DEMO_SEED_SLOW_GUARDIAN: &str = "demo:bob:slow";

/// Seed for failed recovery demo scenario.
pub const DEMO_SEED_FAILED_RECOVERY: &str = "demo:bob:failed";

/// Seed for interactive demo scenario.
pub const DEMO_SEED_INTERACTIVE: &str = "demo:bob:interactive";

/// Demo scenarios available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemoScenario {
    /// Normal recovery flow succeeds.
    HappyPath,
    /// One guardian is slow to respond.
    SlowGuardian,
    /// Recovery fails (insufficient approvals).
    FailedRecovery,
    /// Interactive step-by-step demo.
    Interactive,
}

impl DemoScenario {
    /// Get the seed string for this scenario.
    #[must_use]
    pub const fn seed(&self) -> &'static str {
        match self {
            Self::HappyPath => DEMO_SEED_HAPPY_PATH,
            Self::SlowGuardian => DEMO_SEED_SLOW_GUARDIAN,
            Self::FailedRecovery => DEMO_SEED_FAILED_RECOVERY,
            Self::Interactive => DEMO_SEED_INTERACTIVE,
        }
    }

    /// Get all available scenarios.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [
            Self::HappyPath,
            Self::SlowGuardian,
            Self::FailedRecovery,
            Self::Interactive,
        ]
    }
}

// ============================================================================
// Configuration Types
// ============================================================================

/// Response delay range configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseDelayRange {
    /// Minimum delay in milliseconds.
    pub min_ms: u64,
    /// Maximum delay in milliseconds.
    pub max_ms: u64,
}

impl ResponseDelayRange {
    /// Create a new delay range.
    #[must_use]
    pub const fn new(min_ms: u64, max_ms: u64) -> Self {
        Self { min_ms, max_ms }
    }

    /// Default delay range.
    #[must_use]
    pub const fn default_range() -> Self {
        Self::new(DEFAULT_RESPONSE_DELAY_MIN_MS, DEFAULT_RESPONSE_DELAY_MAX_MS)
    }

    /// Quick delay range for fast agents.
    #[must_use]
    pub const fn quick() -> Self {
        Self::new(QUICK_RESPONSE_DELAY_MIN_MS, QUICK_RESPONSE_DELAY_MAX_MS)
    }

    /// Deliberate delay range for thoughtful agents.
    #[must_use]
    pub const fn deliberate() -> Self {
        Self::new(
            DELIBERATE_RESPONSE_DELAY_MIN_MS,
            DELIBERATE_RESPONSE_DELAY_MAX_MS,
        )
    }

    /// Convert to a tuple (min, max).
    #[must_use]
    pub const fn as_tuple(&self) -> (u64, u64) {
        (self.min_ms, self.max_ms)
    }
}

impl Default for ResponseDelayRange {
    fn default() -> Self {
        Self::default_range()
    }
}

/// Demo agent personality traits.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoPersonality {
    /// How often to respond to messages (0.0-1.0).
    pub chattiness: f64,
    /// Response style keywords (e.g., "friendly", "formal").
    pub style: Vec<String>,
    /// Greeting phrases used by this agent.
    pub greetings: Vec<String>,
}

impl DemoPersonality {
    /// Create a new personality with the given chattiness.
    #[must_use]
    pub fn with_chattiness(chattiness: f64) -> Self {
        Self {
            chattiness,
            ..Default::default()
        }
    }

    /// Add a style descriptor.
    #[must_use]
    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style.push(style.into());
        self
    }

    /// Add a greeting phrase.
    #[must_use]
    pub fn with_greeting(mut self, greeting: impl Into<String>) -> Self {
        self.greetings.push(greeting.into());
        self
    }

    /// Create a chatty personality.
    #[must_use]
    pub fn chatty() -> Self {
        Self::with_chattiness(CHATTY_CHATTINESS)
            .with_style("friendly")
            .with_style("enthusiastic")
    }

    /// Create a quiet personality.
    #[must_use]
    pub fn quiet() -> Self {
        Self::with_chattiness(QUIET_CHATTINESS)
            .with_style("thoughtful")
            .with_style("concise")
    }
}

impl Default for DemoPersonality {
    fn default() -> Self {
        Self {
            chattiness: DEFAULT_CHATTINESS,
            style: vec!["friendly".to_string()],
            greetings: vec!["Hello!".to_string(), "Hi there!".to_string()],
        }
    }
}

/// Portable demo agent configuration.
///
/// This mirrors the terminal's AgentConfig but is frontend-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoAgentConfig {
    /// Simulation seed for deterministic behavior.
    pub seed: u64,
    /// Response delay range.
    pub response_delay: ResponseDelayRange,
    /// Approval probability for recovery requests (0.0-1.0).
    pub approval_probability: f64,
    /// Message generation frequency in milliseconds.
    pub message_frequency_ms: u64,
    /// Enable verbose logging.
    pub verbose_logging: bool,
    /// Personality traits.
    pub personality: DemoPersonality,
}

impl DemoAgentConfig {
    /// Create a builder for custom configuration.
    #[must_use]
    pub fn builder() -> DemoAgentConfigBuilder {
        DemoAgentConfigBuilder::default()
    }

    /// Create a quick, reliable agent configuration.
    #[must_use]
    pub fn quick_reliable() -> Self {
        Self {
            response_delay: ResponseDelayRange::quick(),
            approval_probability: HIGH_APPROVAL_PROBABILITY,
            message_frequency_ms: CHATTY_MESSAGE_FREQUENCY_MS,
            personality: DemoPersonality::chatty(),
            ..Default::default()
        }
    }

    /// Create a deliberate, thoughtful agent configuration.
    #[must_use]
    pub fn deliberate() -> Self {
        Self {
            response_delay: ResponseDelayRange::deliberate(),
            approval_probability: DEFAULT_APPROVAL_PROBABILITY,
            message_frequency_ms: QUIET_MESSAGE_FREQUENCY_MS,
            personality: DemoPersonality::quiet(),
            ..Default::default()
        }
    }
}

impl Default for DemoAgentConfig {
    fn default() -> Self {
        Self {
            seed: DEFAULT_DEMO_SEED,
            response_delay: ResponseDelayRange::default(),
            approval_probability: DEFAULT_APPROVAL_PROBABILITY,
            message_frequency_ms: DEFAULT_MESSAGE_FREQUENCY_MS,
            verbose_logging: false,
            personality: DemoPersonality::default(),
        }
    }
}

/// Builder for DemoAgentConfig.
#[derive(Debug, Clone, Default)]
pub struct DemoAgentConfigBuilder {
    seed: Option<u64>,
    response_delay: Option<ResponseDelayRange>,
    approval_probability: Option<f64>,
    message_frequency_ms: Option<u64>,
    verbose_logging: bool,
    personality: Option<DemoPersonality>,
}

impl DemoAgentConfigBuilder {
    /// Set the simulation seed.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set the response delay range.
    #[must_use]
    pub fn response_delay(mut self, range: ResponseDelayRange) -> Self {
        self.response_delay = Some(range);
        self
    }

    /// Set the approval probability.
    #[must_use]
    pub fn approval_probability(mut self, probability: f64) -> Self {
        self.approval_probability = Some(probability);
        self
    }

    /// Set the message frequency.
    #[must_use]
    pub fn message_frequency_ms(mut self, frequency_ms: u64) -> Self {
        self.message_frequency_ms = Some(frequency_ms);
        self
    }

    /// Enable verbose logging.
    #[must_use]
    pub fn verbose(mut self) -> Self {
        self.verbose_logging = true;
        self
    }

    /// Set the personality.
    #[must_use]
    pub fn personality(mut self, personality: DemoPersonality) -> Self {
        self.personality = Some(personality);
        self
    }

    /// Build the configuration.
    #[must_use]
    pub fn build(self) -> DemoAgentConfig {
        let defaults = DemoAgentConfig::default();
        DemoAgentConfig {
            seed: self.seed.unwrap_or(defaults.seed),
            response_delay: self.response_delay.unwrap_or(defaults.response_delay),
            approval_probability: self
                .approval_probability
                .unwrap_or(defaults.approval_probability),
            message_frequency_ms: self
                .message_frequency_ms
                .unwrap_or(defaults.message_frequency_ms),
            verbose_logging: self.verbose_logging,
            personality: self.personality.unwrap_or(defaults.personality),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DemoAgentConfig::default();
        assert_eq!(config.seed, DEFAULT_DEMO_SEED);
        assert_eq!(config.approval_probability, DEFAULT_APPROVAL_PROBABILITY);
        assert_eq!(config.message_frequency_ms, DEFAULT_MESSAGE_FREQUENCY_MS);
        assert!(!config.verbose_logging);
    }

    #[test]
    fn test_response_delay_range() {
        let range = ResponseDelayRange::new(100, 500);
        assert_eq!(range.min_ms, 100);
        assert_eq!(range.max_ms, 500);
        assert_eq!(range.as_tuple(), (100, 500));

        let default = ResponseDelayRange::default();
        assert_eq!(default.min_ms, DEFAULT_RESPONSE_DELAY_MIN_MS);
        assert_eq!(default.max_ms, DEFAULT_RESPONSE_DELAY_MAX_MS);
    }

    #[test]
    fn test_personality_builder() {
        let personality = DemoPersonality::with_chattiness(0.5)
            .with_style("formal")
            .with_greeting("Greetings!");

        assert_eq!(personality.chattiness, 0.5);
        assert!(personality.style.contains(&"formal".to_string()));
        assert!(personality.greetings.contains(&"Greetings!".to_string()));
    }

    #[test]
    fn test_config_builder() {
        let config = DemoAgentConfig::builder()
            .seed(123)
            .response_delay(ResponseDelayRange::quick())
            .approval_probability(0.99)
            .message_frequency_ms(5000)
            .verbose()
            .build();

        assert_eq!(config.seed, 123);
        assert_eq!(config.response_delay, ResponseDelayRange::quick());
        assert_eq!(config.approval_probability, 0.99);
        assert_eq!(config.message_frequency_ms, 5000);
        assert!(config.verbose_logging);
    }

    #[test]
    fn test_quick_reliable_preset() {
        let config = DemoAgentConfig::quick_reliable();
        assert_eq!(config.response_delay, ResponseDelayRange::quick());
        assert_eq!(config.approval_probability, HIGH_APPROVAL_PROBABILITY);
    }

    #[test]
    fn test_deliberate_preset() {
        let config = DemoAgentConfig::deliberate();
        assert_eq!(config.response_delay, ResponseDelayRange::deliberate());
        assert_eq!(config.message_frequency_ms, QUIET_MESSAGE_FREQUENCY_MS);
    }

    #[test]
    fn test_preset_personalities() {
        let chatty = DemoPersonality::chatty();
        assert_eq!(chatty.chattiness, CHATTY_CHATTINESS);

        let quiet = DemoPersonality::quiet();
        assert_eq!(quiet.chattiness, QUIET_CHATTINESS);
    }

    #[test]
    fn test_demo_threshold_defaults() {
        assert_eq!(DEFAULT_DEMO_THRESHOLD, 2);
        assert_eq!(DEFAULT_DEMO_GUARDIAN_COUNT, 3);
        // 2-of-3 is a valid threshold
        assert!(DEFAULT_DEMO_THRESHOLD <= DEFAULT_DEMO_GUARDIAN_COUNT);
    }

    #[test]
    fn test_demo_scenario_seeds() {
        assert_eq!(DemoScenario::HappyPath.seed(), DEMO_SEED_HAPPY_PATH);
        assert_eq!(DemoScenario::SlowGuardian.seed(), DEMO_SEED_SLOW_GUARDIAN);
        assert_eq!(DemoScenario::FailedRecovery.seed(), DEMO_SEED_FAILED_RECOVERY);
        assert_eq!(DemoScenario::Interactive.seed(), DEMO_SEED_INTERACTIVE);
    }

    #[test]
    fn test_demo_scenario_all() {
        let all = DemoScenario::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&DemoScenario::HappyPath));
        assert!(all.contains(&DemoScenario::Interactive));
    }
}
