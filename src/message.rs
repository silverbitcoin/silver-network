use serde::{Deserialize, Serialize};
use silver_core::{Transaction, TransactionBatch};

/// Network message types for Proof-of-Work mining
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Transaction message
    Transaction(Transaction),

    /// Transaction batch message (mined block)
    Batch(TransactionBatch),

    /// Work package for mining pool
    WorkPackage {
        /// Work ID
        work_id: u64,
        /// Block header data
        header: Vec<u8>,
        /// Target difficulty
        target: u64,
        /// Chain ID
        chain_id: u32,
    },

    /// Miner share submission
    MinerShare {
        /// Work ID
        work_id: u64,
        /// Nonce found by miner
        nonce: u64,
        /// Miner address
        miner_address: Vec<u8>,
    },

    /// Request for batches
    BatchRequest {
        /// Starting batch sequence
        from_sequence: u64,
        /// Ending batch sequence
        to_sequence: u64,
    },

    /// Response to batch request
    BatchResponse {
        /// Requested batches
        batches: Vec<TransactionBatch>,
    },

    /// Ping message for keep-alive
    Ping {
        /// Timestamp
        timestamp: u64,
    },

    /// Pong response to ping
    Pong {
        /// Original timestamp from ping
        timestamp: u64,
    },
}

/// Message type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// Transaction
    Transaction,
    /// Batch
    Batch,
    /// Work package
    WorkPackage,
    /// Miner share
    MinerShare,
    /// Batch request
    BatchRequest,
    /// Batch response
    BatchResponse,
    /// Ping
    Ping,
    /// Pong
    Pong,
}

impl NetworkMessage {
    /// Get the message type
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Transaction(_) => MessageType::Transaction,
            Self::Batch(_) => MessageType::Batch,
            Self::WorkPackage { .. } => MessageType::WorkPackage,
            Self::MinerShare { .. } => MessageType::MinerShare,
            Self::BatchRequest { .. } => MessageType::BatchRequest,
            Self::BatchResponse { .. } => MessageType::BatchResponse,
            Self::Ping { .. } => MessageType::Ping,
            Self::Pong { .. } => MessageType::Pong,
        }
    }

    /// Serialize message to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize message from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }

    /// Get estimated size of message in bytes
    pub fn estimated_size(&self) -> usize {
        match self {
            Self::Transaction(_) => 1024,           // ~1KB per transaction
            Self::Batch(_) => 512 * 1024,           // ~512KB per batch
            Self::WorkPackage { .. } => 256,        // ~256 bytes per work package
            Self::MinerShare { .. } => 128,         // ~128 bytes per share
            Self::BatchRequest { .. } => 64,        // ~64 bytes
            Self::BatchResponse { batches } => batches.len() * 512 * 1024,
            Self::Ping { .. } => 32,
            Self::Pong { .. } => 32,
        }
    }
}
