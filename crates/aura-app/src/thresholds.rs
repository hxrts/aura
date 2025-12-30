//! Threshold helpers for shared defaults and validation.

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
