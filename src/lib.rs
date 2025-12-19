//! # SilverBitcoin Network
//!
//! P2P networking layer using libp2p for Proof-of-Work mining.
//!
//! This crate provides:
//! - Peer discovery (DHT)
//! - Message propagation (gossipsub)
//! - Connection management
//! - Rate limiting and security
//! - Mining pool support
//! - Work distribution and share collection

#![warn(missing_docs, rust_2018_idioms)]
#![forbid(unsafe_code)]

/// Network behaviour implementation for libp2p
mod behaviour;

/// Peer management and information tracking
pub mod peer;

/// Gossip protocol for message propagation
pub mod gossip;

/// Peer discovery using DHT
pub mod discovery;

/// State synchronization protocol
pub mod sync;

/// Security features including rate limiting and reputation
pub mod security;

/// Network configuration
pub mod config;

/// Error types for network operations
pub mod error;

/// Error recovery strategies and mechanisms
pub mod error_recovery;

/// Comprehensive error logging with context
pub mod error_logging;

/// Comprehensive error handling integration
pub mod error_handling;

/// Network message types and serialization
pub mod message;

/// Message compression and batching optimization
pub mod compression;

/// Mining pool support and work distribution
pub mod mining_pool;

pub use behaviour::SilverBehaviour;
pub use compression::{BatchStats, CompressionStats, MessageBatcher, MessageCompressor};
pub use config::NetworkConfig;
pub use discovery::PeerDiscovery;
pub use error::{
    DefaultErrorHandler, ErrorContext, ErrorHandler, NetworkError, RecoveryStrategy, Result,
};
pub use error_logging::{ErrorCategory, ErrorLogger, ErrorLogEntry, ErrorMetrics, ErrorSummary};
pub use error_recovery::{
    ErrorRecoveryManager, ExponentialBackoff, RateLimiter, RecoveryAction,
};
pub use error_handling::{ComprehensiveErrorHandler, ErrorHandlingResult};
pub use gossip::GossipProtocol;
pub use message::{MessageType, NetworkMessage};
pub use peer::{PeerId, PeerInfo, PeerManager};
pub use security::{PeerReputation, RateLimiter as SecurityRateLimiter};
pub use sync::StateSync;
pub use mining_pool::{MiningPoolManager, WorkPackage, MinerShare};

/// Network handle for broadcasting and communication.
///
/// Provides high-level interface for:
/// - Broadcasting transactions
/// - Broadcasting blocks
/// - Mining pool communication
/// - Peer management
pub struct NetworkHandle {
    /// Gossip protocol for message propagation
    gossip: GossipProtocol,
    /// Peer manager for connection management
    #[allow(dead_code)]
    peer_manager: PeerManager,
    /// Mining pool manager
    pool_manager: Option<MiningPoolManager>,
}

impl NetworkHandle {
    /// Create a new network handle
    pub fn new(gossip: GossipProtocol, peer_manager: PeerManager) -> Self {
        Self {
            gossip,
            peer_manager,
            pool_manager: None,
        }
    }

    /// Create a network handle with mining pool support
    pub fn with_mining_pool(
        gossip: GossipProtocol,
        peer_manager: PeerManager,
        pool_manager: MiningPoolManager,
    ) -> Self {
        Self {
            gossip,
            peer_manager,
            pool_manager: Some(pool_manager),
        }
    }

    /// Broadcast a transaction to all peers
    pub async fn broadcast_transaction(&self, tx: &silver_core::Transaction) -> Result<()> {
        let data =
            bincode::serialize(tx).map_err(|e| NetworkError::Serialization(e.to_string()))?;
        self.gossip.broadcast(MessageType::Transaction, data).await
    }

    /// Broadcast a batch (mined block) to all peers
    pub async fn broadcast_batch(&self, batch: &silver_core::TransactionBatch) -> Result<()> {
        let data =
            bincode::serialize(batch).map_err(|e| NetworkError::Serialization(e.to_string()))?;
        self.gossip.broadcast(MessageType::Batch, data).await
    }

    /// Get mining pool manager if available
    pub fn mining_pool(&self) -> Option<&MiningPoolManager> {
        self.pool_manager.as_ref()
    }
}
