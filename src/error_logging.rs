//! Comprehensive error logging with context
//!
//! Provides structured error logging with:
//! - Error context tracking
//! - Error categorization
//! - Error metrics collection
//! - Error history tracking
//! - Operator alerts

use crate::error::{ErrorContext, NetworkError};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{error, warn};

/// Error log entry
#[derive(Debug, Clone)]
pub struct ErrorLogEntry {
    /// Error that occurred
    pub error: String,

    /// Error context
    pub context: ErrorContext,

    /// Error category
    pub category: ErrorCategory,

    /// Timestamp
    pub timestamp: SystemTime,

    /// Whether error was recovered
    pub recovered: bool,

    /// Recovery action taken
    pub recovery_action: Option<String>,
}

/// Error category for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Connection-related errors
    Connection,

    /// Message-related errors
    Message,

    /// Peer-related errors
    Peer,

    /// Configuration errors
    Configuration,

    /// Timeout errors
    Timeout,

    /// Rate limiting errors
    RateLimit,

    /// Resource errors
    Resource,

    /// Critical errors
    Critical,

    /// Other errors
    Other,
}

impl ErrorCategory {
    /// Get category from error
    pub fn from_error(error: &NetworkError) -> Self {
        match error {
            NetworkError::ConnectionFailed(_)
            | NetworkError::ConnectionTimeout(_)
            | NetworkError::ConnectionRefused(_)
            | NetworkError::ConnectionReset(_)
            | NetworkError::HandshakeFailed(_) => ErrorCategory::Connection,

            NetworkError::SerializationError(_)
            | NetworkError::DeserializationError(_)
            | NetworkError::MessageParsingFailed(_)
            | NetworkError::MalformedMessage(_)
            | NetworkError::InvalidMessageFormat(_)
            | NetworkError::MessageTooLarge(_, _) => ErrorCategory::Message,

            NetworkError::InvalidPeer(_)
            | NetworkError::PeerNotFound(_)
            | NetworkError::PeerDisconnected(_)
            | NetworkError::PeerUnhealthy(_) => ErrorCategory::Peer,

            NetworkError::ConfigurationError(_) | NetworkError::InvalidConfiguration(_) => {
                ErrorCategory::Configuration
            }

            NetworkError::OperationTimeout(_) => ErrorCategory::Timeout,

            NetworkError::RateLimitExceeded(_) => ErrorCategory::RateLimit,

            NetworkError::ResourceExhaustion(_) => ErrorCategory::Resource,

            NetworkError::CriticalError(_) => ErrorCategory::Critical,

            _ => ErrorCategory::Other,
        }
    }
}

/// Error logger for tracking and reporting errors
pub struct ErrorLogger {
    /// Error history (circular buffer)
    history: Arc<parking_lot::Mutex<VecDeque<ErrorLogEntry>>>,

    /// Maximum history size
    max_history_size: usize,

    /// Error metrics
    metrics: Arc<ErrorMetrics>,
}

/// Error metrics
#[derive(Debug, Default)]
pub struct ErrorMetrics {
    /// Total errors
    pub total_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Connection errors
    pub connection_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Message errors
    pub message_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Peer errors
    pub peer_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Timeout errors
    pub timeout_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Rate limit errors
    pub rate_limit_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Critical errors
    pub critical_errors: Arc<std::sync::atomic::AtomicU64>,

    /// Recovered errors
    pub recovered_errors: Arc<std::sync::atomic::AtomicU64>,
}

impl ErrorMetrics {
    /// Record error
    pub fn record_error(&self, category: ErrorCategory) {
        self.total_errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        match category {
            ErrorCategory::Connection => {
                self.connection_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ErrorCategory::Message => {
                self.message_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ErrorCategory::Peer => {
                self.peer_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ErrorCategory::Timeout => {
                self.timeout_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ErrorCategory::RateLimit => {
                self.rate_limit_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            ErrorCategory::Critical => {
                self.critical_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Record recovered error
    pub fn record_recovered(&self) {
        self.recovered_errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get total errors
    pub fn get_total_errors(&self) -> u64 {
        self.total_errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get connection errors
    pub fn get_connection_errors(&self) -> u64 {
        self.connection_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get message errors
    pub fn get_message_errors(&self) -> u64 {
        self.message_errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get peer errors
    pub fn get_peer_errors(&self) -> u64 {
        self.peer_errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get timeout errors
    pub fn get_timeout_errors(&self) -> u64 {
        self.timeout_errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get rate limit errors
    pub fn get_rate_limit_errors(&self) -> u64 {
        self.rate_limit_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get critical errors
    pub fn get_critical_errors(&self) -> u64 {
        self.critical_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get recovered errors
    pub fn get_recovered_errors(&self) -> u64 {
        self.recovered_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl ErrorLogger {
    /// Create a new error logger
    pub fn new(max_history_size: usize) -> Self {
        Self {
            history: Arc::new(parking_lot::Mutex::new(VecDeque::with_capacity(
                max_history_size,
            ))),
            max_history_size,
            metrics: Arc::new(ErrorMetrics::default()),
        }
    }

    /// Log an error
    pub fn log_error(
        &self,
        error: &NetworkError,
        context: &ErrorContext,
        recovered: bool,
        recovery_action: Option<String>,
    ) {
        let category = ErrorCategory::from_error(error);

        // Record metrics
        self.metrics.record_error(category);
        if recovered {
            self.metrics.record_recovered();
        }

        // Create log entry
        let entry = ErrorLogEntry {
            error: error.to_string(),
            context: context.clone(),
            category,
            timestamp: SystemTime::now(),
            recovered,
            recovery_action: recovery_action.clone(),
        };

        // Add to history
        let mut history = self.history.lock();
        if history.len() >= self.max_history_size {
            history.pop_front();
        }
        history.push_back(entry.clone());

        // Log based on error severity
        if error.is_critical() {
            error!(
                error = %error,
                operation = %context.operation,
                peer_id = ?context.peer_id,
                context = %context.context,
                recovery = ?recovery_action,
                "CRITICAL ERROR - operator intervention required"
            );
        } else if error.is_recoverable() {
            warn!(
                error = %error,
                operation = %context.operation,
                peer_id = ?context.peer_id,
                context = %context.context,
                recovery = ?recovery_action,
                "Recoverable error - attempting recovery"
            );
        } else {
            error!(
                error = %error,
                operation = %context.operation,
                peer_id = ?context.peer_id,
                context = %context.context,
                "Network error occurred"
            );
        }
    }

    /// Get error history
    pub fn get_history(&self) -> Vec<ErrorLogEntry> {
        self.history.lock().iter().cloned().collect()
    }

    /// Get recent errors (last N)
    pub fn get_recent_errors(&self, count: usize) -> Vec<ErrorLogEntry> {
        let history = self.history.lock();
        history
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Get errors by category
    pub fn get_errors_by_category(&self, category: ErrorCategory) -> Vec<ErrorLogEntry> {
        self.history
            .lock()
            .iter()
            .filter(|entry| entry.category == category)
            .cloned()
            .collect()
    }

    /// Get metrics
    pub fn get_metrics(&self) -> Arc<ErrorMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Clear history
    pub fn clear_history(&self) {
        self.history.lock().clear();
    }

    /// Get error summary
    pub fn get_summary(&self) -> ErrorSummary {
        ErrorSummary {
            total_errors: self.metrics.get_total_errors(),
            connection_errors: self.metrics.get_connection_errors(),
            message_errors: self.metrics.get_message_errors(),
            peer_errors: self.metrics.get_peer_errors(),
            timeout_errors: self.metrics.get_timeout_errors(),
            rate_limit_errors: self.metrics.get_rate_limit_errors(),
            critical_errors: self.metrics.get_critical_errors(),
            recovered_errors: self.metrics.get_recovered_errors(),
        }
    }
}

/// Error summary statistics
#[derive(Debug, Clone)]
pub struct ErrorSummary {
    /// Total errors
    pub total_errors: u64,

    /// Connection errors
    pub connection_errors: u64,

    /// Message errors
    pub message_errors: u64,

    /// Peer errors
    pub peer_errors: u64,

    /// Timeout errors
    pub timeout_errors: u64,

    /// Rate limit errors
    pub rate_limit_errors: u64,

    /// Critical errors
    pub critical_errors: u64,

    /// Recovered errors
    pub recovered_errors: u64,
}

impl ErrorSummary {
    /// Get recovery rate
    pub fn recovery_rate(&self) -> f64 {
        if self.total_errors == 0 {
            0.0
        } else {
            (self.recovered_errors as f64 / self.total_errors as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_logger() {
        let logger = ErrorLogger::new(100);
        let context = ErrorContext::new("test_operation");
        let error = NetworkError::ConnectionTimeout("test".to_string());

        logger.log_error(&error, &context, true, Some("reconnect".to_string()));

        let history = logger.get_history();
        assert_eq!(history.len(), 1);
        assert!(history[0].recovered);
    }

    #[test]
    fn test_error_metrics() {
        let metrics = ErrorMetrics::default();
        metrics.record_error(ErrorCategory::Connection);
        metrics.record_recovered();

        assert_eq!(metrics.get_total_errors(), 1);
        assert_eq!(metrics.get_connection_errors(), 1);
        assert_eq!(metrics.get_recovered_errors(), 1);
    }

    #[test]
    fn test_error_summary() {
        let summary = ErrorSummary {
            total_errors: 100,
            connection_errors: 50,
            message_errors: 30,
            peer_errors: 10,
            timeout_errors: 5,
            rate_limit_errors: 3,
            critical_errors: 2,
            recovered_errors: 80,
        };

        assert_eq!(summary.recovery_rate(), 80.0);
    }
}
