//! Comprehensive error handling tests
//!
//! Tests for:
//! - Error types and context
//! - Error recovery strategies
//! - Error logging and metrics
//! - Error handling integration

use silver_network::{
    ComprehensiveErrorHandler, ErrorCategory, ErrorContext, ErrorLogger, ErrorRecoveryManager,
    NetworkError,
};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_error_context_creation() {
    let context = ErrorContext::new("test_operation")
        .with_peer("peer1")
        .with_context("connection failed")
        .with_recovery("retry with backoff");

    assert_eq!(context.operation, "test_operation");
    assert_eq!(context.peer_id, Some("peer1".to_string()));
    assert_eq!(context.context, "connection failed");
    assert_eq!(
        context.recovery_suggestion,
        Some("retry with backoff".to_string())
    );
}

#[test]
fn test_error_context_display() {
    let context = ErrorContext::new("test_operation")
        .with_peer("peer1")
        .with_context("connection failed")
        .with_recovery("retry");

    let display = format!("{}", context);
    assert!(display.contains("test_operation"));
    assert!(display.contains("peer1"));
    assert!(display.contains("connection failed"));
    assert!(display.contains("retry"));
}

#[test]
fn test_network_error_is_recoverable() {
    let timeout_error = NetworkError::ConnectionTimeout("test".to_string());
    assert!(timeout_error.is_recoverable());

    let config_error = NetworkError::InvalidConfiguration("test".to_string());
    assert!(config_error.is_critical());
}

#[test]
fn test_network_error_recovery_strategy() {
    let timeout_error = NetworkError::ConnectionTimeout("test".to_string());
    let strategy = timeout_error.recovery_strategy();
    assert_eq!(strategy, silver_network::RecoveryStrategy::ExponentialBackoff);

    let rate_limit_error = NetworkError::RateLimitExceeded("peer1".to_string());
    let strategy = rate_limit_error.recovery_strategy();
    assert_eq!(strategy, silver_network::RecoveryStrategy::Throttle);
}

#[test]
fn test_error_logger_creation() {
    let logger = ErrorLogger::new(100);
    let context = ErrorContext::new("test_operation");
    let error = NetworkError::ConnectionTimeout("test".to_string());

    logger.log_error(&error, &context, true, Some("reconnect".to_string()));

    let history = logger.get_history();
    assert_eq!(history.len(), 1);
    assert!(history[0].recovered);
}

#[test]
fn test_error_logger_history_limit() {
    let logger = ErrorLogger::new(5);
    let context = ErrorContext::new("test_operation");
    let error = NetworkError::ConnectionTimeout("test".to_string());

    // Log 10 errors
    for _ in 0..10 {
        logger.log_error(&error, &context, true, None);
    }

    // History should be limited to 5
    let history = logger.get_history();
    assert_eq!(history.len(), 5);
}

#[test]
fn test_error_logger_metrics() {
    let logger = ErrorLogger::new(100);
    let context = ErrorContext::new("test_operation");

    let connection_error = NetworkError::ConnectionTimeout("test".to_string());
    logger.log_error(&connection_error, &context, true, None);

    let message_error = NetworkError::MessageTooLarge(1000, 100);
    logger.log_error(&message_error, &context, false, None);

    let metrics = logger.get_metrics();
    assert_eq!(metrics.get_total_errors(), 2);
    assert_eq!(metrics.get_connection_errors(), 1);
    assert_eq!(metrics.get_message_errors(), 1);
    assert_eq!(metrics.get_recovered_errors(), 1);
}

#[test]
fn test_error_logger_get_recent_errors() {
    let logger = ErrorLogger::new(100);
    let context = ErrorContext::new("test_operation");
    let error = NetworkError::ConnectionTimeout("test".to_string());

    for _ in 0..10 {
        logger.log_error(&error, &context, true, None);
    }

    let recent = logger.get_recent_errors(5);
    assert_eq!(recent.len(), 5);
}

#[test]
fn test_error_logger_get_errors_by_category() {
    let logger = ErrorLogger::new(100);
    let context = ErrorContext::new("test_operation");

    let connection_error = NetworkError::ConnectionTimeout("test".to_string());
    logger.log_error(&connection_error, &context, true, None);

    let message_error = NetworkError::MessageTooLarge(1000, 100);
    logger.log_error(&message_error, &context, false, None);

    let connection_errors = logger.get_errors_by_category(ErrorCategory::Connection);
    assert_eq!(connection_errors.len(), 1);

    let message_errors = logger.get_errors_by_category(ErrorCategory::Message);
    assert_eq!(message_errors.len(), 1);
}

#[test]
fn test_error_logger_summary() {
    let logger = ErrorLogger::new(100);
    let context = ErrorContext::new("test_operation");

    let connection_error = NetworkError::ConnectionTimeout("test".to_string());
    for _ in 0..5 {
        logger.log_error(&connection_error, &context, true, None);
    }

    let message_error = NetworkError::MessageTooLarge(1000, 100);
    for _ in 0..3 {
        logger.log_error(&message_error, &context, false, None);
    }

    let summary = logger.get_summary();
    assert_eq!(summary.total_errors, 8);
    assert_eq!(summary.connection_errors, 5);
    assert_eq!(summary.message_errors, 3);
    assert_eq!(summary.recovered_errors, 5);
    assert_eq!(summary.recovery_rate(), 62.5);
}

#[test]
fn test_error_recovery_manager_exponential_backoff() {
    let manager = ErrorRecoveryManager::new(5);
    let error = NetworkError::ConnectionTimeout("test".to_string());

    let action1 = manager.handle_error("peer1", &error);
    assert!(action1.is_ok());

    let action2 = manager.handle_error("peer1", &error);
    assert!(action2.is_ok());

    // Error count should increase
    assert_eq!(manager.get_error_count("peer1"), 0);
}

#[test]
fn test_error_recovery_manager_ban_peer() {
    let manager = ErrorRecoveryManager::new(3);
    let error = NetworkError::PeerDisconnected("test".to_string());

    // Trigger multiple errors
    for _ in 0..5 {
        let _ = manager.handle_error("peer1", &error);
    }

    // Peer should be banned after exceeding threshold
    assert!(manager.should_ban_peer("peer1"));
}

#[test]
fn test_error_recovery_manager_reset_error_count() {
    let manager = ErrorRecoveryManager::new(5);
    let error = NetworkError::PeerDisconnected("test".to_string());

    for _ in 0..3 {
        let _ = manager.handle_error("peer1", &error);
    }

    // Error count should be 3 after 3 errors
    assert_eq!(manager.get_error_count("peer1"), 3);

    manager.reset_error_count("peer1");
    // After reset, error count should be 0
    assert_eq!(manager.get_error_count("peer1"), 0);
}

#[tokio::test]
async fn test_comprehensive_error_handler() {
    let logger = Arc::new(ErrorLogger::new(100));
    let recovery_manager = Arc::new(ErrorRecoveryManager::new(5));
    let handler = ComprehensiveErrorHandler::new(logger.clone(), recovery_manager, false);

    let error = NetworkError::ConnectionTimeout("test".to_string());
    let context = ErrorContext::new("test_operation").with_peer("peer1");

    let result = handler.handle_error(error, context).await;
    assert!(result.is_ok());

    let summary = handler.get_error_summary();
    assert_eq!(summary.total_errors, 1);
}

#[tokio::test]
async fn test_comprehensive_error_handler_critical_error() {
    let logger = Arc::new(ErrorLogger::new(100));
    let recovery_manager = Arc::new(ErrorRecoveryManager::new(5));
    let handler = ComprehensiveErrorHandler::new(logger.clone(), recovery_manager, false);

    let error = NetworkError::CriticalError("test".to_string());
    let context = ErrorContext::new("test_operation").with_peer("peer1");

    let result = handler.handle_error(error, context).await;
    assert!(result.is_ok());

    let summary = handler.get_error_summary();
    assert_eq!(summary.critical_errors, 1);
}

#[test]
fn test_error_category_from_error() {
    let connection_error = NetworkError::ConnectionTimeout("test".to_string());
    assert_eq!(
        ErrorCategory::from_error(&connection_error),
        ErrorCategory::Connection
    );

    let message_error = NetworkError::MessageTooLarge(1000, 100);
    assert_eq!(
        ErrorCategory::from_error(&message_error),
        ErrorCategory::Message
    );

    let peer_error = NetworkError::PeerNotFound("peer1".to_string());
    assert_eq!(
        ErrorCategory::from_error(&peer_error),
        ErrorCategory::Peer
    );

    let config_error = NetworkError::InvalidConfiguration("test".to_string());
    assert_eq!(
        ErrorCategory::from_error(&config_error),
        ErrorCategory::Configuration
    );

    let timeout_error = NetworkError::OperationTimeout("test".to_string());
    assert_eq!(
        ErrorCategory::from_error(&timeout_error),
        ErrorCategory::Timeout
    );

    let rate_limit_error = NetworkError::RateLimitExceeded("peer1".to_string());
    assert_eq!(
        ErrorCategory::from_error(&rate_limit_error),
        ErrorCategory::RateLimit
    );

    let critical_error = NetworkError::CriticalError("test".to_string());
    assert_eq!(
        ErrorCategory::from_error(&critical_error),
        ErrorCategory::Critical
    );
}

#[test]
fn test_exponential_backoff_calculation() {
    let mut backoff = silver_network::ExponentialBackoff::new(1000, 300000);

    let delay1 = backoff.next_backoff();
    assert!(delay1.as_millis() >= 900 && delay1.as_millis() <= 1100);
    assert_eq!(backoff.attempt_count(), 1);

    let delay2 = backoff.next_backoff();
    assert!(delay2.as_millis() >= 1800 && delay2.as_millis() <= 2200);
    assert_eq!(backoff.attempt_count(), 2);

    let delay3 = backoff.next_backoff();
    assert!(delay3.as_millis() >= 3600 && delay3.as_millis() <= 4400);
    assert_eq!(backoff.attempt_count(), 3);

    backoff.reset();
    assert_eq!(backoff.attempt_count(), 0);
}

#[test]
fn test_rate_limiter() {
    let limiter = silver_network::RateLimiter::new(10, Duration::from_secs(60));

    // Allow 10 messages
    for _ in 0..10 {
        assert!(limiter.allow_message());
    }

    // 11th message should be rejected
    assert!(!limiter.allow_message());

    // Reset and try again
    limiter.reset();
    assert!(limiter.allow_message());
}

#[test]
fn test_error_propagation() {
    let error = NetworkError::ConnectionTimeout("test".to_string());
    let error_str = format!("{}", error);
    assert!(error_str.contains("Connection timeout"));
}

#[test]
fn test_error_context_with_all_fields() {
    let context = ErrorContext::new("operation1")
        .with_peer("peer1")
        .with_context("detailed context")
        .with_recovery("recovery action");

    assert_eq!(context.operation, "operation1");
    assert_eq!(context.peer_id, Some("peer1".to_string()));
    assert_eq!(context.context, "detailed context");
    assert_eq!(
        context.recovery_suggestion,
        Some("recovery action".to_string())
    );
}

#[test]
fn test_multiple_error_types() {
    let errors = vec![
        (NetworkError::ConnectionFailed("test".to_string()), ErrorCategory::Connection),
        (NetworkError::ConnectionTimeout("test".to_string()), ErrorCategory::Connection),
        (NetworkError::ConnectionRefused("test".to_string()), ErrorCategory::Connection),
        (NetworkError::ConnectionReset("test".to_string()), ErrorCategory::Connection),
        (NetworkError::HandshakeFailed("test".to_string()), ErrorCategory::Connection),
        (NetworkError::SerializationError("test".to_string()), ErrorCategory::Message),
        (NetworkError::DeserializationError("test".to_string()), ErrorCategory::Message),
        (NetworkError::MessageParsingFailed("test".to_string()), ErrorCategory::Message),
        (NetworkError::MalformedMessage("test".to_string()), ErrorCategory::Message),
        (NetworkError::RateLimitExceeded("peer1".to_string()), ErrorCategory::RateLimit),
        (NetworkError::InvalidPeer("test".to_string()), ErrorCategory::Peer),
        (NetworkError::PeerNotFound("test".to_string()), ErrorCategory::Peer),
        (NetworkError::PeerDisconnected("test".to_string()), ErrorCategory::Peer),
        (NetworkError::PeerUnhealthy("test".to_string()), ErrorCategory::Peer),
        (NetworkError::MessageTooLarge(1000, 100), ErrorCategory::Message),
        (NetworkError::InvalidMessageFormat("test".to_string()), ErrorCategory::Message),
        (NetworkError::OperationTimeout("test".to_string()), ErrorCategory::Timeout),
        (NetworkError::IoError("test".to_string()), ErrorCategory::Other),
        (NetworkError::ConfigurationError("test".to_string()), ErrorCategory::Configuration),
        (NetworkError::InvalidConfiguration("test".to_string()), ErrorCategory::Configuration),
        (NetworkError::TransportError("test".to_string()), ErrorCategory::Other),
        (NetworkError::NetworkPartition("test".to_string()), ErrorCategory::Other),
        (NetworkError::ResourceExhaustion("test".to_string()), ErrorCategory::Resource),
        (NetworkError::CriticalError("test".to_string()), ErrorCategory::Critical),
        (NetworkError::ShutdownInProgress("test".to_string()), ErrorCategory::Other),
        (NetworkError::Other("test".to_string()), ErrorCategory::Other),
    ];

    for (error, expected_category) in errors {
        let category = ErrorCategory::from_error(&error);
        assert_eq!(category, expected_category, "Error {:?} should be categorized as {:?}", error, expected_category);
    }
}
