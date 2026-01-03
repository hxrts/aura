//! Threshold helpers for shared defaults and validation.
//!
//! This module provides the canonical threshold logic for all frontends:
//! - Channel thresholds (BFT 2f+1)
//! - Guardian thresholds (majority with FROST minimum)
//! - Recovery thresholds
//! - Validation constants

use aura_core::AuraError;

// ============================================================================
// Constants
// ============================================================================

/// Minimum number of guardians required for FROST threshold signing.
pub const MIN_GUARDIANS: usize = 2;

/// Minimum threshold for guardian ceremonies (at least 2 required).
pub const MIN_THRESHOLD: u32 = 2;

// ============================================================================
// Channel Thresholds (BFT)
// ============================================================================

/// Default group channel threshold (2f+1) for total participants (n).
pub fn default_channel_threshold(total_n: u8) -> u8 {
    if total_n <= 1 {
        return 1;
    }
    let f = total_n.saturating_sub(1) / 3;
    let k = (2 * f) + 1;
    k.clamp(1, total_n)
}

/// Normalize a requested channel threshold given total participants.
pub fn normalize_channel_threshold(requested: u8, total_n: u8) -> u8 {
    let k = if requested == 0 {
        default_channel_threshold(total_n)
    } else {
        requested
    };
    k.clamp(1, total_n.max(1))
}

/// Default guardian threshold (majority) with FROST minimum 2.
pub fn default_guardian_threshold(total_n: u8) -> u8 {
    if total_n >= 2 {
        (total_n / 2) + 1
    } else {
        2
    }
}

/// Normalize a requested guardian threshold given total guardians.
pub fn normalize_guardian_threshold(requested: u8, total_n: u8) -> u8 {
    let mut k = requested;
    if total_n >= 2 {
        if k < 2 {
            k = 2;
        }
        if k > total_n {
            k = total_n;
        }
    } else if k < 2 {
        k = 2;
    }
    k
}

/// Normalize a recovery threshold given total guardians.
pub fn normalize_recovery_threshold(requested: u8, total_n: u8) -> u8 {
    let k = if requested == 0 { 1 } else { requested };
    k.clamp(1, total_n.max(1))
}

// ============================================================================
// Guardian Set Validation
// ============================================================================

/// Validate guardian set parameters for a recovery ceremony.
///
/// # Arguments
/// * `guardian_count` - Number of guardians in the set
/// * `threshold` - Required threshold for approval
///
/// # Returns
/// Ok(()) if valid, Err with detailed message if invalid.
///
/// # Validation Rules
/// - Guardian count must be at least `MIN_GUARDIANS` (2)
/// - Threshold must be at least `MIN_THRESHOLD` (2)
/// - Threshold cannot exceed guardian count
pub fn validate_guardian_set(guardian_count: usize, threshold: u32) -> Result<(), AuraError> {
    if guardian_count < MIN_GUARDIANS {
        return Err(AuraError::invalid(format!(
            "At least {MIN_GUARDIANS} guardians required for threshold signing, got {guardian_count}"
        )));
    }

    if threshold < MIN_THRESHOLD {
        return Err(AuraError::invalid(format!(
            "Threshold must be at least {MIN_THRESHOLD}, got {threshold}"
        )));
    }

    if threshold as usize > guardian_count {
        return Err(AuraError::invalid(format!(
            "Threshold {threshold} exceeds guardian count {guardian_count}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_channel_threshold() {
        assert_eq!(default_channel_threshold(1), 1);
        assert_eq!(default_channel_threshold(3), 1); // f=0, 2*0+1=1
        assert_eq!(default_channel_threshold(4), 3); // f=1, 2*1+1=3
        assert_eq!(default_channel_threshold(7), 5); // f=2, 2*2+1=5
    }

    #[test]
    fn test_normalize_channel_threshold() {
        // Default when 0
        assert_eq!(
            normalize_channel_threshold(0, 4),
            default_channel_threshold(4)
        );
        // Clamped to total
        assert_eq!(normalize_channel_threshold(10, 4), 4);
        // Pass-through when valid
        assert_eq!(normalize_channel_threshold(2, 4), 2);
    }

    #[test]
    fn test_default_guardian_threshold() {
        assert_eq!(default_guardian_threshold(1), 2); // min 2
        assert_eq!(default_guardian_threshold(2), 2); // majority of 2
        assert_eq!(default_guardian_threshold(3), 2); // majority of 3
        assert_eq!(default_guardian_threshold(4), 3); // majority of 4
        assert_eq!(default_guardian_threshold(5), 3); // majority of 5
    }

    #[test]
    fn test_normalize_guardian_threshold() {
        // Minimum 2
        assert_eq!(normalize_guardian_threshold(1, 3), 2);
        // Clamped to total
        assert_eq!(normalize_guardian_threshold(5, 3), 3);
        // Pass-through when valid
        assert_eq!(normalize_guardian_threshold(2, 3), 2);
    }

    #[test]
    fn test_validate_guardian_set_valid() {
        assert!(validate_guardian_set(2, 2).is_ok());
        assert!(validate_guardian_set(3, 2).is_ok());
        assert!(validate_guardian_set(5, 3).is_ok());
    }

    #[test]
    fn test_validate_guardian_set_too_few_guardians() {
        let err = validate_guardian_set(1, 2).unwrap_err();
        assert!(err.to_string().contains("At least 2 guardians"));
    }

    #[test]
    fn test_validate_guardian_set_threshold_too_low() {
        let err = validate_guardian_set(3, 1).unwrap_err();
        assert!(err.to_string().contains("Threshold must be at least 2"));
    }

    #[test]
    fn test_validate_guardian_set_threshold_exceeds_count() {
        let err = validate_guardian_set(2, 3).unwrap_err();
        assert!(err.to_string().contains("exceeds guardian count"));
    }
}
