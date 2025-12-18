use std::fmt;
use std::time::SystemTime;
use thiserror::Error;
use tracing;

/// Error context for network operations
///
/// Provides detailed context about where and when an error occurred,
/// enabling better debugging and recovery strategies.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Operation that was being performed
    pub operation: String,

    /// Peer ID involved (if applicable)
    pub peer_id: Option<String>,

    /// Additional context information
    pub context: String,

    /// Timestamp when error occurred
    pub timestamp: SystemTime,

    /// Error recovery suggestion
    pub recovery_suggestion: Option<String>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            peer_id: None,
            context: String::new(),
            timestamp: SystemTime::now(),
            recovery_suggestion: None,
        }
    }

    /// Add peer ID to context
    pub fn with_peer(mut self, peer_id: impl Into<String>) -> Self {
        self.peer_id = Some(peer_id.into());
        self
    }

    /// Add context information
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    /// Add recovery suggestion
    pub fn with_recovery(mut self, suggestion: impl Into<String>) -> Self {
        self.recovery_suggestion = Some(suggestion.into());
        self
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Operation: {}, Context: {}",
            self.operation, self.context
        )?;
        if let Some(peer_id) = &self.peer_id {
            write!(f, ", Peer: {}", peer_id)?;
        }
        if let Some(suggestion) = &self.recovery_suggestion {
            write!(f, ", Recovery: {}", suggestion)?;
        }
        Ok(())
    }
}

/// Network layer errors with comprehensive context
#[derive(Error, Debug)]
pub enum NetworkError {
    /// Connection establishment failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Connection timeout
    #[error("Connection timeout: {0}")]
    ConnectionTimeout(String),

    /// Connection refused by peer
    #[error("Connection refused: {0}")]
    ConnectionRefused(String),

    /// Connection reset by peer
    #[error("Connection reset: {0}")]
    ConnectionReset(String),

    /// Handshake failed
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// Peer discovery error (backward compatible)
    #[error("Peer discovery error: {0}")]
    Discovery(String),

    /// Peer discovery error
    #[error("Peer discovery error: {0}")]
    DiscoveryError(String),

    /// Message serialization error (backward compatible)
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Message serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Message deserialization error (backward compatible)
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Message deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// Message parsing failed
    #[error("Message parsing failed: {0}")]
    MessageParsingFailed(String),

    /// Malformed message received
    #[error("Malformed message: {0}")]
    MalformedMessage(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded for peer {0}")]
    RateLimitExceeded(String),

    /// Invalid peer
    #[error("Invalid peer: {0}")]
    InvalidPeer(String),

    /// Peer not found
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    /// Peer disconnected
    #[error("Peer disconnected: {0}")]
    PeerDisconnected(String),

    /// Peer marked as unhealthy
    #[error("Peer unhealthy: {0}")]
    PeerUnhealthy(String),

    /// Message too large
    #[error("Message too large: {0} bytes (max: {1})")]
    MessageTooLarge(usize, usize),

    /// Invalid message (backward compatible)
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Invalid message format
    #[error("Invalid message format: {0}")]
    InvalidMessageFormat(String),

    /// Operation timeout
    #[error("Operation timed out: {0}")]
    OperationTimeout(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Transport error
    #[error("Transport error: {0}")]
    TransportError(String),

    /// Network partition detected
    #[error("Network partition: {0}")]
    NetworkPartition(String),

    /// Resource exhaustion
    #[error("Resource exhaustion: {0}")]
    ResourceExhaustion(String),

    /// Critical error requiring operator intervention
    #[error("Critical error: {0}")]
    CriticalError(String),

    /// Shutdown in progress
    #[error("Shutdown in progress: {0}")]
    ShutdownInProgress(String),

    /// Other network error
    #[error("Network error: {0}")]
    Other(String),
}

impl NetworkError {
    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            NetworkError::ConnectionTimeout(_)
                | NetworkError::ConnectionReset(_)
                | NetworkError::OperationTimeout(_)
                | NetworkError::RateLimitExceeded(_)
                | NetworkError::PeerDisconnected(_)
                | NetworkError::NetworkPartition(_)
        )
    }

    /// Check if error is critical
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            NetworkError::CriticalError(_)
                | NetworkError::InvalidConfiguration(_)
                | NetworkError::ResourceExhaustion(_)
        )
    }

    /// Get recovery strategy for this error
    pub fn recovery_strategy(&self) -> RecoveryStrategy {
        match self {
            NetworkError::ConnectionTimeout(_) | NetworkError::ConnectionReset(_) => {
                RecoveryStrategy::ExponentialBackoff
            }
            NetworkError::RateLimitExceeded(_) => RecoveryStrategy::Throttle,
            NetworkError::PeerDisconnected(_) => RecoveryStrategy::Reconnect,
            NetworkError::OperationTimeout(_) => RecoveryStrategy::Retry,
            NetworkError::NetworkPartition(_) => RecoveryStrategy::WaitAndRetry,
            NetworkError::CriticalError(_) => RecoveryStrategy::Alert,
            NetworkError::InvalidConfiguration(_) => RecoveryStrategy::FailStartup,
            _ => RecoveryStrategy::Log,
        }
    }
}

/// Recovery strategy for different error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Retry with exponential backoff
    ExponentialBackoff,

    /// Throttle operations
    Throttle,

    /// Attempt to reconnect
    Reconnect,

    /// Retry operation
    Retry,

    /// Wait and retry
    WaitAndRetry,

    /// Alert operator
    Alert,

    /// Fail startup
    FailStartup,

    /// Just log the error
    Log,
}

/// Result type for network operations
pub type Result<T> = std::result::Result<T, NetworkError>;

/// Error handler trait for custom error handling
pub trait ErrorHandler: Send + Sync {
    /// Handle a network error
    fn handle_error(&self, error: &NetworkError, context: &ErrorContext);

    /// Handle a critical error
    fn handle_critical_error(&self, error: &NetworkError, context: &ErrorContext);

    /// Handle a recoverable error
    fn handle_recoverable_error(&self, error: &NetworkError, context: &ErrorContext);
}

/// Default error handler implementation
pub struct DefaultErrorHandler;

impl ErrorHandler for DefaultErrorHandler {
    fn handle_error(&self, error: &NetworkError, context: &ErrorContext) {
        tracing::error!(
            error = %error,
            operation = %context.operation,
            peer_id = ?context.peer_id,
            context = %context.context,
            "Network error occurred"
        );
    }

    fn handle_critical_error(&self, error: &NetworkError, context: &ErrorContext) {
        tracing::error!(
            error = %error,
            operation = %context.operation,
            peer_id = ?context.peer_id,
            context = %context.context,
            recovery = ?context.recovery_suggestion,
            "CRITICAL network error - operator intervention required"
        );
    }

    fn handle_recoverable_error(&self, error: &NetworkError, context: &ErrorContext) {
        tracing::warn!(
            error = %error,
            operation = %context.operation,
            peer_id = ?context.peer_id,
            context = %context.context,
            recovery = ?context.recovery_suggestion,
            "Recoverable network error - attempting recovery"
        );
    }
}
