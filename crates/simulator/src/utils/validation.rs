//! Validation utilities

use crate::AuraError;

/// Result type for validation operations
pub type ValidationResult<T = ()> = Result<T, AuraError>;

/// Validate that a value is positive
pub fn validate_positive<T>(value: T, field_name: &str) -> ValidationResult
where
    T: PartialOrd + Default + Copy + std::fmt::Display,
{
    if value > T::default() {
        Ok(())
    } else {
        Err(AuraError::configuration_error(format!(
            "{} must be positive, got {}",
            field_name, value
        )))
    }
}

/// Validate that a value is non-negative
pub fn validate_non_negative<T>(value: T, field_name: &str) -> ValidationResult
where
    T: PartialOrd + Default + Copy + std::fmt::Display,
{
    if value >= T::default() {
        Ok(())
    } else {
        Err(AuraError::configuration_error(format!(
            "{} must be non-negative, got {}",
            field_name, value
        )))
    }
}

/// Validate that a value is within an inclusive range
pub fn validate_range_inclusive<T>(value: T, min: T, max: T, field_name: &str) -> ValidationResult
where
    T: PartialOrd + Copy + std::fmt::Display,
{
    if value >= min && value <= max {
        Ok(())
    } else {
        Err(AuraError::configuration_error(format!(
            "{} must be between {} and {} (inclusive), got {}",
            field_name, min, max, value
        )))
    }
}

/// Validate that a value is within an exclusive range
pub fn validate_range_exclusive<T>(value: T, min: T, max: T, field_name: &str) -> ValidationResult
where
    T: PartialOrd + Copy + std::fmt::Display,
{
    if value > min && value < max {
        Ok(())
    } else {
        Err(AuraError::configuration_error(format!(
            "{} must be between {} and {} (exclusive), got {}",
            field_name, min, max, value
        )))
    }
}

/// Validate that a fraction/percentage is between 0.0 and 1.0 (inclusive)
pub fn validate_fraction(value: f64, field_name: &str) -> ValidationResult {
    validate_range_inclusive(value, 0.0, 1.0, field_name)
}

/// Validate that a percentage is between 0.0 and 100.0 (inclusive)
pub fn validate_percentage(value: f64, field_name: &str) -> ValidationResult {
    validate_range_inclusive(value, 0.0, 100.0, field_name)
}

/// Validate that a string is not empty
pub fn validate_non_empty_string(value: &str, field_name: &str) -> ValidationResult {
    if value.is_empty() {
        Err(AuraError::configuration_error(format!(
            "{} cannot be empty",
            field_name
        )))
    } else {
        Ok(())
    }
}

/// Validate that a collection is not empty
pub fn validate_non_empty_collection<T>(collection: &[T], field_name: &str) -> ValidationResult {
    if collection.is_empty() {
        Err(AuraError::configuration_error(format!(
            "{} cannot be empty",
            field_name
        )))
    } else {
        Ok(())
    }
}

/// Validate that a collection size is within bounds
pub fn validate_collection_size<T>(
    collection: &[T],
    min_size: usize,
    max_size: usize,
    field_name: &str,
) -> ValidationResult {
    let size = collection.len();
    if size >= min_size && size <= max_size {
        Ok(())
    } else {
        Err(AuraError::configuration_error(format!(
            "{} size must be between {} and {}, got {}",
            field_name, min_size, max_size, size
        )))
    }
}

/// Validate that a timeout value is reasonable
pub fn validate_timeout_ms(timeout_ms: u64, field_name: &str) -> ValidationResult {
    const MIN_TIMEOUT_MS: u64 = 1;
    const MAX_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000; // 24 hours

    validate_range_inclusive(timeout_ms, MIN_TIMEOUT_MS, MAX_TIMEOUT_MS, field_name)
}

/// Validate that a tick count is reasonable
pub fn validate_tick_count(ticks: u64, field_name: &str) -> ValidationResult {
    const MAX_TICKS: u64 = 1_000_000; // Reasonable maximum

    validate_range_inclusive(ticks, 1, MAX_TICKS, field_name)
}

/// Validate that a participant count is valid for threshold protocols
pub fn validate_participant_count(count: usize, field_name: &str) -> ValidationResult {
    const MIN_PARTICIPANTS: usize = 1;
    const MAX_PARTICIPANTS: usize = 1000; // Reasonable maximum

    validate_range_inclusive(count, MIN_PARTICIPANTS, MAX_PARTICIPANTS, field_name)
}

/// Validate threshold parameters (M-of-N)
pub fn validate_threshold(threshold: usize, total: usize, field_name: &str) -> ValidationResult {
    if threshold == 0 {
        return Err(AuraError::configuration_error(format!(
            "{} threshold must be at least 1",
            field_name
        )));
    }

    if threshold > total {
        return Err(AuraError::configuration_error(format!(
            "{} threshold ({}) cannot exceed total participants ({})",
            field_name, threshold, total
        )));
    }

    Ok(())
}

/// Validate network drop rate
pub fn validate_drop_rate(drop_rate: f64, field_name: &str) -> ValidationResult {
    validate_fraction(drop_rate, field_name)
}

/// Validate network latency range
pub fn validate_latency_range(
    min_latency_ms: u64,
    max_latency_ms: u64,
    field_name: &str,
) -> ValidationResult {
    if min_latency_ms > max_latency_ms {
        return Err(AuraError::configuration_error(format!(
            "{} min latency ({}) cannot exceed max latency ({})",
            field_name, min_latency_ms, max_latency_ms
        )));
    }

    const MAX_REASONABLE_LATENCY_MS: u64 = 60_000; // 1 minute

    validate_range_inclusive(
        max_latency_ms,
        0,
        MAX_REASONABLE_LATENCY_MS,
        &format!("{} max latency", field_name),
    )
}

/// Validate a seed value
pub fn validate_seed(seed: u64, field_name: &str) -> ValidationResult {
    // Any u64 value is valid for a seed, but 0 might indicate uninitialized
    if seed == 0 {
        eprintln!(
            "Warning: {} is 0, which may indicate an uninitialized seed",
            field_name
        );
    }
    Ok(())
}

/// Combined validation for common configuration patterns
pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate all common simulation configuration fields
    pub fn validate_simulation_config(
        max_ticks: u64,
        max_time_ms: u64,
        seed: u64,
    ) -> ValidationResult {
        validate_tick_count(max_ticks, "max_ticks")?;
        validate_timeout_ms(max_time_ms, "max_time_ms")?;
        validate_seed(seed, "seed")?;
        Ok(())
    }

    /// Validate network configuration
    pub fn validate_network_config(
        drop_rate: f64,
        min_latency_ms: u64,
        max_latency_ms: u64,
    ) -> ValidationResult {
        validate_drop_rate(drop_rate, "drop_rate")?;
        validate_latency_range(min_latency_ms, max_latency_ms, "latency")?;
        Ok(())
    }

    /// Validate threshold configuration
    pub fn validate_threshold_config(
        threshold: usize,
        total_participants: usize,
    ) -> ValidationResult {
        validate_participant_count(total_participants, "total_participants")?;
        validate_threshold(threshold, total_participants, "threshold")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_positive() {
        assert!(validate_positive(1, "test").is_ok());
        assert!(validate_positive(0, "test").is_err());
        assert!(validate_positive(-1, "test").is_err());
    }

    #[test]
    fn test_validate_non_negative() {
        assert!(validate_non_negative(1, "test").is_ok());
        assert!(validate_non_negative(0, "test").is_ok());
        assert!(validate_non_negative(-1, "test").is_err());
    }

    #[test]
    fn test_validate_range_inclusive() {
        assert!(validate_range_inclusive(5, 1, 10, "test").is_ok());
        assert!(validate_range_inclusive(1, 1, 10, "test").is_ok());
        assert!(validate_range_inclusive(10, 1, 10, "test").is_ok());
        assert!(validate_range_inclusive(0, 1, 10, "test").is_err());
        assert!(validate_range_inclusive(11, 1, 10, "test").is_err());
    }

    #[test]
    fn test_validate_fraction() {
        assert!(validate_fraction(0.0, "test").is_ok());
        assert!(validate_fraction(0.5, "test").is_ok());
        assert!(validate_fraction(1.0, "test").is_ok());
        assert!(validate_fraction(-0.1, "test").is_err());
        assert!(validate_fraction(1.1, "test").is_err());
    }

    #[test]
    fn test_validate_non_empty_string() {
        assert!(validate_non_empty_string("test", "field").is_ok());
        assert!(validate_non_empty_string("", "field").is_err());
    }

    #[test]
    fn test_validate_threshold() {
        assert!(validate_threshold(2, 3, "test").is_ok());
        assert!(validate_threshold(3, 3, "test").is_ok());
        assert!(validate_threshold(0, 3, "test").is_err());
        assert!(validate_threshold(4, 3, "test").is_err());
    }

    #[test]
    fn test_validate_latency_range() {
        assert!(validate_latency_range(100, 1000, "test").is_ok());
        assert!(validate_latency_range(1000, 100, "test").is_err());
        assert!(validate_latency_range(0, 100_000, "test").is_err()); // Too high
    }

    #[test]
    fn test_config_validator() {
        assert!(ConfigValidator::validate_simulation_config(1000, 5000, 42).is_ok());
        assert!(ConfigValidator::validate_network_config(0.1, 10, 100).is_ok());
        assert!(ConfigValidator::validate_threshold_config(2, 3).is_ok());
    }
}
