use serde::{Deserialize, Serialize};
use silver_core::{Certificate, Snapshot, Transaction, TransactionBatch};

/// Network message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Transaction message
    Transaction(Transaction),

    /// Transaction batch message
    Batch(TransactionBatch),

    /// Certificate message
    Certificate(Certificate),

    /// Snapshot message
    Snapshot(Snapshot),

    /// Request for snapshot
    SnapshotRequest {
        /// Sequence number of requested snapshot
        sequence_number: u64,
    },

    /// Response to snapshot request
    SnapshotResponse {
        /// Requested snapshot (None if not found)
        snapshot: Option<Snapshot>,
    },

    /// Request for transactions
    TransactionRequest {
        /// Starting sequence number
        from_sequence: u64,
        /// Ending sequence number
        to_sequence: u64,
    },

    /// Response to transaction request
    TransactionResponse {
        /// Requested transactions
        transactions: Vec<Transaction>,
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
    /// Certificate
    Certificate,
    /// Snapshot
    Snapshot,
    /// Snapshot request
    SnapshotRequest,
    /// Snapshot response
    SnapshotResponse,
    /// Transaction request
    TransactionRequest,
    /// Transaction response
    TransactionResponse,
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
            Self::Certificate(_) => MessageType::Certificate,
            Self::Snapshot(_) => MessageType::Snapshot,
            Self::SnapshotRequest { .. } => MessageType::SnapshotRequest,
            Self::SnapshotResponse { .. } => MessageType::SnapshotResponse,
            Self::TransactionRequest { .. } => MessageType::TransactionRequest,
            Self::TransactionResponse { .. } => MessageType::TransactionResponse,
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
        // This is an approximation; actual size may vary
        match self {
            Self::Transaction(_) => 1024,      // ~1KB per transaction
            Self::Batch(_) => 512 * 1024,      // ~512KB per batch
            Self::Certificate(_) => 10 * 1024, // ~10KB per certificate
            Self::Snapshot(_) => 100 * 1024,   // ~100KB per snapshot
            Self::SnapshotRequest { .. } => 64,
            Self::SnapshotResponse { snapshot } => {
                if snapshot.is_some() {
                    100 * 1024
                } else {
                    64
                }
            }
            Self::TransactionRequest { .. } => 64,
            Self::TransactionResponse { transactions } => transactions.len() * 1024,
            Self::Ping { .. } => 32,
            Self::Pong { .. } => 32,
        }
    }
}
