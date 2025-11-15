use thiserror::Error;

/// Network layer errors
#[derive(Error, Debug)]
pub enum NetworkError {
    /// libp2p transport error
    #[error("Transport error: {0}")]
    Transport(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Peer discovery error
    #[error("Peer discovery error: {0}")]
    Discovery(String),

    /// Message serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Message deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded for peer {0}")]
    RateLimitExceeded(String),

    /// Invalid peer
    #[error("Invalid peer: {0}")]
    InvalidPeer(String),

    /// Peer not found
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    /// Message too large
    #[error("Message too large: {0} bytes (max: {1})")]
    MessageTooLarge(usize, usize),

    /// Invalid message
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Timeout
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Other error
    #[error("Network error: {0}")]
    Other(String),
}

/// Result type for network operations
pub type Result<T> = std::result::Result<T, NetworkError>;
