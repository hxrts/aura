//! Progress reporting middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext};
use crate::CliError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Middleware for progress reporting and long-running operation tracking
pub struct ProgressReportingMiddleware {
    /// Progress tracking state
    progress_tracker: Arc<Mutex<ProgressTracker>>,
    /// Enable progress reporting
    enabled: bool,
    /// Progress update interval
    update_interval: Duration,
}

/// Progress tracking state
#[derive(Debug)]
struct ProgressTracker {
    /// Current operation
    current_operation: Option<String>,
    /// Progress percentage (0.0 - 100.0)
    progress: f64,
    /// Total items/steps
    total: Option<f64>,
    /// Completed items/steps
    completed: f64,
    /// Start time
    start_time: Option<Instant>,
    /// Last update time
    last_update: Option<Instant>,
    /// ETA estimation
    eta: Option<Duration>,
    /// Operation history
    history: Vec<ProgressEntry>,
}

/// Progress entry for history tracking
#[derive(Debug, Clone)]
struct ProgressEntry {
    operation: String,
    progress: f64,
    timestamp: Instant,
    message: Option<String>,
}

impl ProgressReportingMiddleware {
    /// Create new progress reporting middleware
    pub fn new() -> Self {
        Self {
            progress_tracker: Arc::new(Mutex::new(ProgressTracker::new())),
            enabled: true,
            update_interval: Duration::from_millis(100),
        }
    }
    
    /// Enable or disable progress reporting
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    
    /// Set progress update interval
    pub fn with_update_interval(mut self, interval: Duration) -> Self {
        self.update_interval = interval;
        self
    }
    
    /// Start tracking an operation
    fn start_operation(&self, operation_name: &str) -> Result<(), CliError> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut tracker = self.progress_tracker.lock().map_err(|_| {
            CliError::OperationFailed("Failed to acquire progress tracker lock".to_string())
        })?;
        
        tracker.start_operation(operation_name);
        Ok(())
    }
    
    /// Update progress for current operation
    fn update_progress(&self, progress: f64, total: Option<f64>, message: Option<String>) -> Result<(), CliError> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut tracker = self.progress_tracker.lock().map_err(|_| {
            CliError::OperationFailed("Failed to acquire progress tracker lock".to_string())
        })?;
        
        tracker.update_progress(progress, total, message);
        Ok(())
    }
    
    /// Complete current operation
    fn complete_operation(&self) -> Result<(), CliError> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut tracker = self.progress_tracker.lock().map_err(|_| {
            CliError::OperationFailed("Failed to acquire progress tracker lock".to_string())
        })?;
        
        tracker.complete_operation();
        Ok(())
    }
    
    /// Get current progress status
    fn get_progress_status(&self) -> Result<Value, CliError> {
        let tracker = self.progress_tracker.lock().map_err(|_| {
            CliError::OperationFailed("Failed to acquire progress tracker lock".to_string())
        })?;
        
        Ok(tracker.to_json())
    }
    
    /// Should update progress based on interval
    fn should_update_progress(&self) -> bool {
        if !self.enabled {
            return false;
        }
        
        if let Ok(tracker) = self.progress_tracker.lock() {
            if let Some(last_update) = tracker.last_update {
                return last_update.elapsed() >= self.update_interval;
            }
        }
        
        true
    }
}

impl ProgressTracker {
    fn new() -> Self {
        Self {
            current_operation: None,
            progress: 0.0,
            total: None,
            completed: 0.0,
            start_time: None,
            last_update: None,
            eta: None,
            history: Vec::new(),
        }
    }
    
    fn start_operation(&mut self, operation_name: &str) {
        let now = Instant::now();
        
        self.current_operation = Some(operation_name.to_string());
        self.progress = 0.0;
        self.total = None;
        self.completed = 0.0;
        self.start_time = Some(now);
        self.last_update = Some(now);
        self.eta = None;
        
        self.history.push(ProgressEntry {
            operation: operation_name.to_string(),
            progress: 0.0,
            timestamp: now,
            message: Some("Started".to_string()),
        });
    }
    
    fn update_progress(&mut self, progress: f64, total: Option<f64>, message: Option<String>) {
        let now = Instant::now();
        
        self.progress = progress.max(0.0).min(100.0);
        self.completed = progress;
        self.total = total;
        self.last_update = Some(now);
        
        // Calculate ETA if we have start time and progress > 0
        if let Some(start_time) = self.start_time {
            if progress > 0.0 {
                let elapsed = now.duration_since(start_time);
                let rate = progress / elapsed.as_secs_f64();
                if rate > 0.0 {
                    let remaining = 100.0 - progress;
                    let eta_seconds = remaining / rate;
                    self.eta = Some(Duration::from_secs_f64(eta_seconds));
                }
            }
        }
        
        // Add to history (keep last 100 entries)
        self.history.push(ProgressEntry {
            operation: self.current_operation.clone().unwrap_or_default(),
            progress,
            timestamp: now,
            message,
        });
        
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }
    
    fn complete_operation(&mut self) {
        let now = Instant::now();
        
        self.progress = 100.0;
        self.last_update = Some(now);
        
        if let Some(operation) = &self.current_operation {
            self.history.push(ProgressEntry {
                operation: operation.clone(),
                progress: 100.0,
                timestamp: now,
                message: Some("Completed".to_string()),
            });
        }
        
        // Calculate total duration
        if let Some(start_time) = self.start_time {
            let duration = now.duration_since(start_time);
            self.eta = Some(Duration::ZERO);
            
            // Log completion
            eprintln!("Operation completed in {:.2}s", duration.as_secs_f64());
        }
    }
    
    fn to_json(&self) -> Value {
        let duration = if let Some(start_time) = self.start_time {
            Some(start_time.elapsed().as_secs_f64())
        } else {
            None
        };
        
        let eta_seconds = self.eta.map(|eta| eta.as_secs_f64());
        
        json!({
            "current_operation": self.current_operation,
            "progress": self.progress,
            "total": self.total,
            "completed": self.completed,
            "duration_seconds": duration,
            "eta_seconds": eta_seconds,
            "history_count": self.history.len()
        })
    }
}

impl Default for ProgressReportingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for ProgressReportingMiddleware {
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        match &operation {
            CliOperation::ReportProgress { message, progress, total } => {
                // Handle progress reporting
                self.update_progress(*progress, *total, Some(message.clone()))?;
                
                // Return progress status
                self.get_progress_status()
            }
            
            CliOperation::Command { .. } => {
                // Start tracking for command operations
                self.start_operation(&context.command)?;
                
                // Execute the command
                let result = next.handle(operation, context);
                
                // Complete tracking
                if result.is_ok() {
                    self.complete_operation()?;
                }
                
                result
            }
            
            _ => {
                // For other operations, pass through
                next.handle(operation, context)
            }
        }
    }
    
    fn name(&self) -> &str {
        "progress_reporting"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    
    #[test]
    fn test_progress_tracking() {
        let middleware = ProgressReportingMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("test_command".to_string(), vec![]);
        
        // Start a command (should start progress tracking)
        let result = middleware.process(
            CliOperation::Command { args: vec!["test".to_string()] },
            &context,
            &handler,
        );
        assert!(result.is_ok());
        
        // Check progress status
        let status = middleware.get_progress_status().unwrap();
        assert_eq!(status["progress"], 100.0); // Should be completed
    }
    
    #[test]
    fn test_progress_reporting() {
        let middleware = ProgressReportingMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("progress_test".to_string(), vec![]);
        
        // Report progress
        let result = middleware.process(
            CliOperation::ReportProgress {
                message: "Processing...".to_string(),
                progress: 50.0,
                total: Some(100.0),
            },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status["progress"], 50.0);
        assert_eq!(status["total"], 100.0);
    }
    
    #[test]
    fn test_disabled_progress() {
        let middleware = ProgressReportingMiddleware::new().with_enabled(false);
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        // Progress reporting should still work but not track internally
        let result = middleware.process(
            CliOperation::ReportProgress {
                message: "Test".to_string(),
                progress: 25.0,
                total: None,
            },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
    }
}