//! # SilverBitcoin Network
//!
//! P2P networking layer using libp2p.
//!
//! This crate provides:
//! - Peer discovery (DHT)
//! - Message propagation (gossipsub)
//! - Connection management
//! - Rate limiting and security
//! - State synchronization protocol

#![warn(missing_docs, rust_2018_idioms)]
#![forbid(unsafe_code)]

mod behaviour;
pub mod peer;
pub mod gossip;
pub mod discovery;
pub mod sync;
pub mod security;
pub mod config;
pub mod error;
pub mod message;

pub use behaviour::SilverBehaviour;
pub use peer::{PeerManager, PeerInfo, PeerId};
pub use gossip::GossipProtocol;
pub use discovery::PeerDiscovery;
pub use sync::StateSync;
pub use security::{RateLimiter, PeerReputation};
pub use config::NetworkConfig;
pub use error::{NetworkError, Result};
pub use message::{NetworkMessage, MessageType};

/// Network handle for broadcasting and communication
pub struct NetworkHandle {
    gossip: GossipProtocol,
    peer_manager: PeerManager,
}

impl NetworkHandle {
    /// Create a new network handle
    pub fn new(gossip: GossipProtocol, peer_manager: PeerManager) -> Self {
        Self {
            gossip,
            peer_manager,
        }
    }

    /// Broadcast a batch to all validators
    pub async fn broadcast_batch(&self, batch: &silver_core::TransactionBatch) -> Result<()> {
        // Serialize batch
        let data = bincode::serialize(batch)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;

        // Broadcast via gossip
        self.gossip.broadcast(MessageType::Batch, data).await
    }

    /// Broadcast a certificate to all validators
    pub async fn broadcast_certificate(&self, certificate: &silver_core::Certificate) -> Result<()> {
        // Serialize certificate
        let data = bincode::serialize(certificate)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;

        // Broadcast via gossip
        self.gossip.broadcast(MessageType::Certificate, data).await
    }
}
