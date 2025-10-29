//! Metric type definitions and time series data structures

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Metric value types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    /// Counter - monotonically increasing value
    Counter(u64),
    /// Gauge - arbitrary value that can go up or down
    Gauge(f64),
    /// Histogram - distribution of values
    Histogram(Vec<f64>),
    /// Timer - duration measurement in milliseconds
    Timer(u64),
    /// Set - collection of unique values
    Set(Vec<String>),
}

/// Metric types for categorization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricType {
    /// Performance metrics
    Performance,
    /// Business logic metrics
    Business,
    /// System resource metrics
    System,
    /// Network metrics
    Network,
    /// Security metrics
    Security,
    /// Custom application metrics
    Custom,
}

/// Metric categories for organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricCategory {
    /// Core simulation metrics
    Simulation,
    /// Property monitoring metrics
    PropertyMonitoring,
    /// Protocol execution metrics
    Protocol,
    /// Network simulation metrics
    Network,
    /// Performance and resource metrics
    Performance,
    /// Byzantine adversary metrics
    Byzantine,
    /// Custom user-defined metrics
    Custom(String),
}

/// Performance counter for tracking performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceCounter {
    /// Counter name
    pub name: String,
    /// Current value
    pub value: u64,
    /// Time series of historical values
    pub history: TimeSeries<u64>,
}

impl PerformanceCounter {
    /// Create a new performance counter
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            value: 0,
            history: TimeSeries::new(),
        }
    }

    /// Increment the counter
    pub fn increment(&mut self) {
        self.value += 1;
        self.history
            .add_point(crate::utils::current_unix_timestamp_secs(), self.value);
    }

    /// Add a value to the counter
    pub fn add(&mut self, value: u64) {
        self.value += value;
        self.history
            .add_point(crate::utils::current_unix_timestamp_secs(), self.value);
    }

    /// Reset the counter
    pub fn reset(&mut self) {
        self.value = 0;
        self.history
            .add_point(crate::utils::current_unix_timestamp_secs(), self.value);
    }
}

/// Time series data structure for tracking metrics over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeries<T> {
    /// Data points with timestamps
    points: VecDeque<TimePoint<T>>,
    /// Maximum number of points to keep
    max_points: usize,
}

/// Individual time series data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimePoint<T> {
    /// Timestamp (seconds since epoch)
    pub timestamp: u64,
    /// The value at this timestamp
    pub value: T,
}

impl<T> TimeSeries<T> {
    /// Create a new time series with default capacity
    pub fn new() -> Self {
        Self::with_capacity(1000)
    }

    /// Create a new time series with specified capacity
    pub fn with_capacity(max_points: usize) -> Self {
        Self {
            points: VecDeque::with_capacity(max_points),
            max_points,
        }
    }

    /// Add a new data point
    pub fn add_point(&mut self, timestamp: u64, value: T) {
        self.points.push_back(TimePoint { timestamp, value });

        // Remove oldest points if we exceed capacity
        while self.points.len() > self.max_points {
            self.points.pop_front();
        }
    }

    /// Get the current value (most recent)
    pub fn current(&self) -> Option<&T> {
        self.points.back().map(|p| &p.value)
    }

    /// Get all data points
    pub fn points(&self) -> &VecDeque<TimePoint<T>> {
        &self.points
    }

    /// Get number of data points
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Check if time series is empty
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Get points within a time range
    pub fn points_in_range(&self, start: u64, end: u64) -> Vec<&TimePoint<T>> {
        self.points
            .iter()
            .filter(|p| p.timestamp >= start && p.timestamp <= end)
            .collect()
    }

    /// Clear all data points
    pub fn clear(&mut self) {
        self.points.clear();
    }
}

impl<T> TimeSeries<T>
where
    T: Clone + std::ops::Add<Output = T> + Default,
{
    /// Calculate the total sum of all values
    pub fn total(&self) -> T {
        self.points
            .iter()
            .fold(T::default(), |acc, p| acc + p.value.clone())
    }
}

impl<T> TimeSeries<T>
where
    T: Clone + std::ops::Add<Output = T> + std::ops::Div<f64, Output = T> + Into<f64> + Default,
{
    /// Calculate the average value
    pub fn average(&self) -> Option<T> {
        if self.points.is_empty() {
            return None;
        }

        let sum = self.total();
        let count = self.points.len() as f64;
        Some(sum / count)
    }
}

impl<T> TimeSeries<T>
where
    T: Clone + PartialOrd + Ord,
{
    /// Get the minimum value
    pub fn min(&self) -> Option<&T> {
        self.points.iter().map(|p| &p.value).min()
    }

    /// Get the maximum value
    pub fn max(&self) -> Option<&T> {
        self.points.iter().map(|p| &p.value).max()
    }
}

impl<T> Default for TimeSeries<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricValue {
    /// Get the numeric value if possible
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Counter(v) => Some(*v as f64),
            MetricValue::Gauge(v) => Some(*v),
            MetricValue::Timer(v) => Some(*v as f64),
            MetricValue::Histogram(values) => {
                if values.is_empty() {
                    None
                } else {
                    Some(values.iter().sum::<f64>() / values.len() as f64)
                }
            }
            MetricValue::Set(values) => Some(values.len() as f64),
        }
    }

    /// Check if metric value represents a counter
    pub fn is_counter(&self) -> bool {
        matches!(self, MetricValue::Counter(_))
    }

    /// Check if metric value represents a gauge
    pub fn is_gauge(&self) -> bool {
        matches!(self, MetricValue::Gauge(_))
    }

    /// Check if metric value represents a histogram
    pub fn is_histogram(&self) -> bool {
        matches!(self, MetricValue::Histogram(_))
    }

    /// Check if metric value represents a timer
    pub fn is_timer(&self) -> bool {
        matches!(self, MetricValue::Timer(_))
    }

    /// Get metric type
    pub fn metric_type(&self) -> &'static str {
        match self {
            MetricValue::Counter(_) => "counter",
            MetricValue::Gauge(_) => "gauge",
            MetricValue::Histogram(_) => "histogram",
            MetricValue::Timer(_) => "timer",
            MetricValue::Set(_) => "set",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_series_basic_operations() {
        let mut ts = TimeSeries::new();
        assert!(ts.is_empty());

        ts.add_point(1000, 10u64);
        ts.add_point(2000, 20u64);
        ts.add_point(3000, 30u64);

        assert_eq!(ts.len(), 3);
        assert_eq!(ts.current(), Some(&30u64));
        assert_eq!(ts.total(), 60u64);
    }

    #[test]
    fn test_time_series_capacity_limit() {
        let mut ts = TimeSeries::with_capacity(2);

        ts.add_point(1000, 10u64);
        ts.add_point(2000, 20u64);
        ts.add_point(3000, 30u64); // Should evict the first point

        assert_eq!(ts.len(), 2);
        assert_eq!(ts.points().front().unwrap().value, 20u64);
        assert_eq!(ts.current(), Some(&30u64));
    }

    #[test]
    fn test_time_series_range_query() {
        let mut ts = TimeSeries::new();
        ts.add_point(1000, 10u64);
        ts.add_point(2000, 20u64);
        ts.add_point(3000, 30u64);
        ts.add_point(4000, 40u64);

        let points_in_range = ts.points_in_range(1500, 3500);
        assert_eq!(points_in_range.len(), 2);
        assert_eq!(points_in_range[0].value, 20u64);
        assert_eq!(points_in_range[1].value, 30u64);
    }

    #[test]
    fn test_time_series_statistics() {
        let mut ts = TimeSeries::new();
        ts.add_point(1000, 10i64);
        ts.add_point(2000, 20i64);
        ts.add_point(3000, 30i64);

        assert_eq!(ts.min(), Some(&10i64));
        assert_eq!(ts.max(), Some(&30i64));
        assert_eq!(ts.total(), 60i64);
    }

    #[test]
    fn test_metric_value_conversions() {
        let counter = MetricValue::Counter(42);
        assert_eq!(counter.as_f64(), Some(42.0));
        assert!(counter.is_counter());
        assert_eq!(counter.metric_type(), "counter");

        let gauge = MetricValue::Gauge(3.14);
        assert_eq!(gauge.as_f64(), Some(3.14));
        assert!(gauge.is_gauge());

        let histogram = MetricValue::Histogram(vec![1.0, 2.0, 3.0]);
        assert_eq!(histogram.as_f64(), Some(2.0)); // Average
        assert!(histogram.is_histogram());

        let timer = MetricValue::Timer(1000);
        assert_eq!(timer.as_f64(), Some(1000.0));
        assert!(timer.is_timer());
    }

    #[test]
    fn test_empty_time_series_operations() {
        let ts: TimeSeries<u64> = TimeSeries::new();
        assert!(ts.is_empty());
        assert_eq!(ts.current(), None);
        assert_eq!(ts.total(), 0u64);
        assert_eq!(ts.min(), None);
        assert_eq!(ts.max(), None);
    }

    #[test]
    fn test_time_series_clear() {
        let mut ts = TimeSeries::new();
        ts.add_point(1000, 10u64);
        ts.add_point(2000, 20u64);

        assert_eq!(ts.len(), 2);

        ts.clear();
        assert!(ts.is_empty());
        assert_eq!(ts.current(), None);
    }
}
