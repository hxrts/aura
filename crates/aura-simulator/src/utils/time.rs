//! Time and timestamp utilities

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds
#[allow(clippy::disallowed_methods)]
pub fn current_unix_timestamp_secs() -> u64 {
    // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
    #[allow(clippy::expect_used)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_secs()
}

/// Get current Unix timestamp in milliseconds
#[allow(clippy::disallowed_methods, clippy::expect_used)]
pub fn current_unix_timestamp_millis() -> u64 {
    // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_millis() as u64
}

/// Get current Unix timestamp in microseconds
#[allow(clippy::disallowed_methods, clippy::expect_used)]
pub fn current_unix_timestamp_micros() -> u64 {
    // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX epoch")
        .as_micros() as u64
}

/// Execute a function and measure its execution time in milliseconds
pub fn time_elapsed_millis<F, T>(f: F) -> (T, u64)
where
    F: FnOnce() -> T,
{
    let start_time = current_unix_timestamp_millis();
    let result = f();
    let end_time = current_unix_timestamp_millis();
    (result, end_time - start_time)
}

/// Execute a function and measure its execution time in seconds
pub fn time_elapsed_secs<F, T>(f: F) -> (T, u64)
where
    F: FnOnce() -> T,
{
    let start_time = current_unix_timestamp_secs();
    let result = f();
    let end_time = current_unix_timestamp_secs();
    (result, end_time - start_time)
}

/// Convert milliseconds to a human-readable duration string
pub fn format_duration_ms(millis: u64) -> String {
    if millis < 1000 {
        format!("{}ms", millis)
    } else if millis < 60_000 {
        format!("{:.1}s", millis as f64 / 1000.0)
    } else if millis < 3_600_000 {
        let mins = millis / 60_000;
        let secs = (millis % 60_000) / 1000;
        format!("{}m{}s", mins, secs)
    } else {
        let hours = millis / 3_600_000;
        let mins = (millis % 3_600_000) / 60_000;
        format!("{}h{}m", hours, mins)
    }
}

/// Convert timestamp to ISO 8601 string representation
pub fn timestamp_to_iso8601(timestamp_secs: u64) -> String {
    let datetime = UNIX_EPOCH + std::time::Duration::from_secs(timestamp_secs);
    // Note: This is a simplified implementation
    // For production use, consider using chrono crate
    format!("{:?}", datetime)
}

/// Get elapsed time since a reference timestamp in milliseconds
pub fn elapsed_since_millis(reference_timestamp_millis: u64) -> u64 {
    current_unix_timestamp_millis().saturating_sub(reference_timestamp_millis)
}

/// Get elapsed time since a reference timestamp in seconds
pub fn elapsed_since_secs(reference_timestamp_secs: u64) -> u64 {
    current_unix_timestamp_secs().saturating_sub(reference_timestamp_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_current_timestamps() {
        let secs = current_unix_timestamp_secs();
        let millis = current_unix_timestamp_millis();
        let micros = current_unix_timestamp_micros();

        assert!(secs > 0);
        assert!(millis > secs * 1000);
        assert!(micros > millis * 1000);
    }

    #[test]
    fn test_time_elapsed_millis() {
        let (result, elapsed) = time_elapsed_millis(|| {
            sleep(Duration::from_millis(10));
            42
        });

        assert_eq!(result, 42);
        assert!(elapsed >= 10);
        assert!(elapsed < 100); // Should not take more than 100ms
    }

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(1500), "1.5s");
        assert_eq!(format_duration_ms(65000), "1m5s");
        assert_eq!(format_duration_ms(3665000), "1h1m");
    }

    #[test]
    fn test_elapsed_since() {
        let now_secs = current_unix_timestamp_secs();
        let now_millis = current_unix_timestamp_millis();

        sleep(Duration::from_millis(10));

        let elapsed_secs = elapsed_since_secs(now_secs);
        let elapsed_millis = elapsed_since_millis(now_millis);

        assert!(elapsed_secs >= 0);
        assert!(elapsed_millis >= 10);
    }
}
