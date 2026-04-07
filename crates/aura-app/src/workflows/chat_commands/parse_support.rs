use std::time::Duration;

/// Typed errors for slash-command duration parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseDurationError {
    /// The input was empty after trimming.
    Empty,
    /// The numeric portion could not be parsed.
    InvalidNumber(String),
    /// The parsed duration overflowed `u64` seconds conversion.
    Overflow {
        /// Parsed numeric value before unit conversion.
        value: u64,
        /// Multiplier applied for the requested unit.
        factor: u64,
    },
    /// The unit suffix is not supported.
    UnknownUnit(char),
}

impl std::fmt::Display for ParseDurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty duration"),
            Self::InvalidNumber(number) => write!(f, "invalid number: {number}"),
            Self::Overflow { value, factor } => {
                write!(f, "duration overflow: {value} * {factor}")
            }
            Self::UnknownUnit(unit) => write!(f, "unknown unit: {unit}"),
        }
    }
}

impl std::error::Error for ParseDurationError {}

/// Parse a duration string (e.g., "5m", "1h", "30s", "1d")
///
/// Supported formats:
/// - `Ns` - N seconds
/// - `Nm` - N minutes
/// - `Nh` - N hours
/// - `Nd` - N days
/// - `N`  - N minutes (default unit)
pub fn parse_duration(s: &str) -> Result<Duration, ParseDurationError> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return Err(ParseDurationError::Empty);
    }

    let (num_str, unit) = if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else if s.ends_with('d') {
        (&s[..s.len() - 1], 'd')
    } else {
        (s.as_str(), 'm')
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| ParseDurationError::InvalidNumber(num_str.to_string()))?;

    let checked_mul = |value: u64, factor: u64| {
        value
            .checked_mul(factor)
            .ok_or(ParseDurationError::Overflow { value, factor })
    };

    let secs = match unit {
        's' => num,
        'm' => checked_mul(num, 60)?,
        'h' => checked_mul(num, 3600)?,
        'd' => checked_mul(num, 86400)?,
        _ => return Err(ParseDurationError::UnknownUnit(unit)),
    };

    Ok(Duration::from_secs(secs))
}

/// Normalize a channel name by stripping leading # characters.
#[must_use]
pub fn normalize_channel_name(name: &str) -> String {
    name.trim_start_matches('#').trim().to_string()
}

/// Check if input looks like a command (starts with /).
#[must_use]
pub fn is_command(input: &str) -> bool {
    input.trim().starts_with('/')
}
