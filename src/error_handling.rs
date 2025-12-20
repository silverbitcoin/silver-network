//! Comprehensive error handling integration
//!
//! Provides unified error handling across the network layer with:
//! - Centralized error handling
//! - Error propagation
//! - Error recovery coordination
//! - Error logging and metrics
//! - Operator alerts

use crate::error::{ErrorContext, ErrorHandler, NetworkError, RecoveryStrategy};
use crate::error_logging::{ErrorCategory, ErrorLogger};
use crate::error_recovery::{ErrorRecoveryManager, RecoveryAction};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Comprehensive error handler
///
/// Coordinates error handling, recovery, and logging across the network layer.
pub struct ComprehensiveErrorHandler {
    /// Error logger
    logger: Arc<ErrorLogger>,

    /// Error recovery manager
    recovery_manager: Arc<ErrorRecoveryManager>,

    /// Custom error handler
    custom_handler: Option<Arc<dyn ErrorHandler>>,

    /// Enable operator alerts
    enable_alerts: bool,
}

impl ComprehensiveErrorHandler {
    /// Create a new comprehensive error handler
    pub fn new(
        logger: Arc<ErrorLogger>,
        recovery_manager: Arc<ErrorRecoveryManager>,
        enable_alerts: bool,
    ) -> Self {
        Self {
            logger,
            recovery_manager,
            custom_handler: None,
            enable_alerts,
        }
    }

    /// Set custom error handler
    pub fn with_custom_handler(mut self, handler: Arc<dyn ErrorHandler>) -> Self {
        self.custom_handler = Some(handler);
        self
    }

    /// Handle error with full recovery pipeline
    pub async fn handle_error(
        &self,
        error: NetworkError,
        context: ErrorContext,
    ) -> Result<RecoveryAction, NetworkError> {
        let peer_id = context.peer_id.clone().unwrap_or_else(|| "unknown".to_string());

        // Determine if error is critical
        let is_critical = error.is_critical();
        let is_recoverable = error.is_recoverable();

        // Call custom handler if provided
        if let Some(handler) = &self.custom_handler {
            if is_critical {
                handler.handle_critical_error(&error, &context);
            } else if is_recoverable {
                handler.handle_recoverable_error(&error, &context);
            } else {
                handler.handle_error(&error, &context);
            }
        }

        // Get recovery strategy
        let strategy = error.recovery_strategy();

        // Handle error based on strategy
        let recovery_action = match strategy {
            RecoveryStrategy::ExponentialBackoff => {
                self.handle_connection_error(&peer_id, &error, &context)
                    .await
            }
            RecoveryStrategy::Throttle => {
                self.handle_rate_limit_error(&peer_id, &error, &context)
                    .await
            }
            RecoveryStrategy::Reconnect => {
                self.handle_peer_error(&peer_id, &error, &context).await
            }
            RecoveryStrategy::Retry => {
                self.handle_retry_error(&peer_id, &error, &context).await
            }
            RecoveryStrategy::WaitAndRetry => {
                self.handle_network_partition(&peer_id, &error, &context)
                    .await
            }
            RecoveryStrategy::Alert => {
                self.handle_critical_error(&peer_id, &error, &context)
                    .await
            }
            RecoveryStrategy::FailStartup => {
                self.handle_startup_error(&peer_id, &error, &context)
                    .await
            }
            RecoveryStrategy::Log => {
                self.handle_log_only_error(&peer_id, &error, &context)
                    .await
            }
        };

        // Log the error with recovery action
        let recovery_action_str = format!("{:?}", recovery_action);
        self.logger.log_error(
            &error,
            &context,
            is_recoverable,
            Some(recovery_action_str),
        );

        Ok(recovery_action)
    }

    /// Handle connection error
    async fn handle_connection_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        warn!(
            peer_id = peer_id,
            error = %error,
            "Connection error - scheduling exponential backoff"
        );

        match self.recovery_manager.handle_error(peer_id, error) {
            Ok(action) => action,
            Err(e) => {
                error!(
                    peer_id = peer_id,
                    error = %e,
                    "Failed to handle connection error"
                );
                RecoveryAction::Log
            }
        }
    }

    /// Handle rate limit error
    async fn handle_rate_limit_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        warn!(
            peer_id = peer_id,
            error = %error,
            "Rate limit exceeded - throttling peer"
        );

        match self.recovery_manager.handle_error(peer_id, error) {
            Ok(action) => action,
            Err(e) => {
                error!(
                    peer_id = peer_id,
                    error = %e,
                    "Failed to handle rate limit error"
                );
                RecoveryAction::Log
            }
        }
    }

    /// Handle peer error
    async fn handle_peer_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        warn!(
            peer_id = peer_id,
            error = %error,
            "Peer error - attempting reconnection"
        );

        match self.recovery_manager.handle_error(peer_id, error) {
            Ok(action) => action,
            Err(e) => {
                error!(
                    peer_id = peer_id,
                    error = %e,
                    "Failed to handle peer error"
                );
                RecoveryAction::Log
            }
        }
    }

    /// Handle retry error
    async fn handle_retry_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        info!(
            peer_id = peer_id,
            error = %error,
            "Retryable error - scheduling retry"
        );

        match self.recovery_manager.handle_error(peer_id, error) {
            Ok(action) => action,
            Err(e) => {
                error!(
                    peer_id = peer_id,
                    error = %e,
                    "Failed to handle retry error"
                );
                RecoveryAction::Log
            }
        }
    }

    /// Handle network partition
    async fn handle_network_partition(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        error!(
            peer_id = peer_id,
            error = %error,
            "Network partition detected - waiting before retry"
        );

        match self.recovery_manager.handle_error(peer_id, error) {
            Ok(action) => action,
            Err(e) => {
                error!(
                    peer_id = peer_id,
                    error = %e,
                    "Failed to handle network partition"
                );
                RecoveryAction::Log
            }
        }
    }

    /// Handle critical error
    async fn handle_critical_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        context: &ErrorContext,
    ) -> RecoveryAction {
        error!(
            peer_id = peer_id,
            error = %error,
            context = %context,
            "CRITICAL ERROR - operator intervention required"
        );

        if self.enable_alerts {
            self.send_operator_alert(peer_id, error, context).await;
        }

        RecoveryAction::Alert
    }

    /// Handle startup error
    async fn handle_startup_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        context: &ErrorContext,
    ) -> RecoveryAction {
        error!(
            peer_id = peer_id,
            error = %error,
            context = %context,
            "Startup error - configuration invalid"
        );

        RecoveryAction::FailStartup
    }

    /// Handle log-only error
    async fn handle_log_only_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
        _context: &ErrorContext,
    ) -> RecoveryAction {
        info!(
            peer_id = peer_id,
            error = %error,
            "Logging error without recovery action"
        );

        RecoveryAction::Log
    }

    /// Send operator alert
    async fn send_operator_alert(
        &self,
        peer_id: &str,
        error: &NetworkError,
        context: &ErrorContext,
    ) {
        error!(
            peer_id = peer_id,
            error = %error,
            context = %context,
            "ALERT: Critical network error - operator intervention required"
        );

        // Log the critical error with full context
        let recovery_action = self.recovery_manager.handle_error(
            &context.peer_id.clone().unwrap_or_else(|| "unknown".to_string()),
            error,
        ).ok().map(|action| format!("{:?}", action));
        
        self.logger.log_error(
            error,
            context,
            true,
            recovery_action.clone(),
        );

        // Trigger recovery action for critical errors
        if self.enable_alerts {
            // Notify recovery manager to take action
            info!(
                "Critical error recovery action: {:?}",
                recovery_action
            );
        }
    }

    /// Get error logger
    pub fn logger(&self) -> Arc<ErrorLogger> {
        Arc::clone(&self.logger)
    }

    /// Get recovery manager
    pub fn recovery_manager(&self) -> Arc<ErrorRecoveryManager> {
        Arc::clone(&self.recovery_manager)
    }

    /// Get error summary
    pub fn get_error_summary(&self) -> crate::error_logging::ErrorSummary {
        self.logger.get_summary()
    }

    /// Get recent errors
    pub fn get_recent_errors(&self, count: usize) -> Vec<crate::error_logging::ErrorLogEntry> {
        self.logger.get_recent_errors(count)
    }

    /// Get errors by category
    pub fn get_errors_by_category(
        &self,
        category: ErrorCategory,
    ) -> Vec<crate::error_logging::ErrorLogEntry> {
        self.logger.get_errors_by_category(category)
    }
}

/// Error handling result
pub type ErrorHandlingResult<T> = std::result::Result<T, NetworkError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_comprehensive_error_handler() {
        let logger = Arc::new(ErrorLogger::new(100));
        let recovery_manager = Arc::new(ErrorRecoveryManager::new(5));
        let handler = ComprehensiveErrorHandler::new(logger, recovery_manager, false);

        let error = NetworkError::ConnectionTimeout("test".to_string());
        let context = ErrorContext::new("test_operation").with_peer("peer1");

        let result = handler.handle_error(error, context).await;
        assert!(result.is_ok());
    }
}
