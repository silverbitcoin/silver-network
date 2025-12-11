use crate::{NetworkError, Result};
use libp2p::PeerId;
use silver_core::{Snapshot, Transaction, TransactionDigest};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// State synchronization protocol
pub struct StateSync {
    /// Current sync state
    state: SyncState,

    /// Pending snapshot requests
    pending_snapshot_requests: HashMap<u64, SnapshotRequest>,

    /// Pending transaction requests
    pending_transaction_requests: HashMap<TransactionRequestId, TransactionRequest>,

    /// Downloaded snapshots awaiting verification
    pending_snapshots: HashMap<u64, PendingSnapshot>,

    /// Downloaded transactions
    downloaded_transactions: HashMap<TransactionDigest, Transaction>,

    /// Sync timeout
    timeout: Duration,

    /// Maximum concurrent requests
    max_concurrent_requests: usize,

    /// Request ID counter
    next_request_id: u64,
}

/// Synchronization state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    /// Not syncing
    Idle,

    /// Downloading snapshot
    DownloadingSnapshot {
        /// Target snapshot sequence number
        target_sequence: u64,
        /// Started at
        started_at: Instant,
    },

    /// Verifying snapshot
    VerifyingSnapshot {
        /// Snapshot sequence number
        sequence: u64,
    },

    /// Downloading transactions
    DownloadingTransactions {
        /// From sequence number
        from_sequence: u64,
        /// To sequence number
        to_sequence: u64,
        /// Started at
        started_at: Instant,
    },

    /// Applying transactions
    ApplyingTransactions {
        /// Number of transactions to apply
        count: usize,
    },

    /// Synced
    Synced {
        /// Current sequence number
        sequence: u64,
    },
}

/// Snapshot request
#[derive(Debug, Clone)]
struct SnapshotRequest {
    /// Sequence number (kept for request tracking)
    #[allow(dead_code)]
    sequence: u64,

    /// Requested from peer
    peer_id: PeerId,

    /// Request timestamp
    requested_at: Instant,

    /// Number of retries
    retries: u32,
}

/// Transaction request ID
type TransactionRequestId = u64;

/// Transaction request
#[derive(Debug, Clone)]
struct TransactionRequest {
    /// Request ID (kept for request tracking)
    #[allow(dead_code)]
    id: TransactionRequestId,

    /// From sequence number
    from_sequence: u64,

    /// To sequence number
    to_sequence: u64,

    /// Requested from peer
    peer_id: PeerId,

    /// Request timestamp
    requested_at: Instant,

    /// Number of retries
    retries: u32,
}

/// Pending snapshot awaiting verification
#[derive(Debug, Clone)]
struct PendingSnapshot {
    /// Snapshot
    snapshot: Snapshot,

    /// Received from peer (kept for peer reputation tracking)
    #[allow(dead_code)]
    peer_id: PeerId,

    /// Received at (kept for timeout tracking)
    #[allow(dead_code)]
    received_at: Instant,
}

impl StateSync {
    /// Create a new StateSync
    pub fn new() -> Self {
        Self {
            state: SyncState::Idle,
            pending_snapshot_requests: HashMap::new(),
            pending_transaction_requests: HashMap::new(),
            pending_snapshots: HashMap::new(),
            downloaded_transactions: HashMap::new(),
            timeout: Duration::from_secs(30),
            max_concurrent_requests: 10,
            next_request_id: 0,
        }
    }

    /// Create with custom configuration
    pub fn with_config(timeout: Duration, max_concurrent_requests: usize) -> Self {
        Self {
            state: SyncState::Idle,
            pending_snapshot_requests: HashMap::new(),
            pending_transaction_requests: HashMap::new(),
            pending_snapshots: HashMap::new(),
            downloaded_transactions: HashMap::new(),
            timeout,
            max_concurrent_requests,
            next_request_id: 0,
        }
    }

    /// Get current sync state
    pub fn state(&self) -> &SyncState {
        &self.state
    }

    /// Check if currently syncing
    pub fn is_syncing(&self) -> bool {
        !matches!(self.state, SyncState::Idle | SyncState::Synced { .. })
    }

    /// Start snapshot download
    pub fn start_snapshot_download(&mut self, target_sequence: u64, peer_id: PeerId) -> Result<()> {
        if self.is_syncing() {
            return Err(NetworkError::Other("Already syncing".to_string()));
        }

        info!(
            "Starting snapshot download for sequence {}",
            target_sequence
        );

        self.state = SyncState::DownloadingSnapshot {
            target_sequence,
            started_at: Instant::now(),
        };

        self.request_snapshot(target_sequence, peer_id)?;

        Ok(())
    }

    /// Request a snapshot from a peer
    fn request_snapshot(&mut self, sequence: u64, peer_id: PeerId) -> Result<()> {
        if self.pending_snapshot_requests.len() >= self.max_concurrent_requests {
            return Err(NetworkError::Other(
                "Too many concurrent requests".to_string(),
            ));
        }

        let request = SnapshotRequest {
            sequence,
            peer_id,
            requested_at: Instant::now(),
            retries: 0,
        };

        self.pending_snapshot_requests.insert(sequence, request);
        debug!("Requested snapshot {} from peer {}", sequence, peer_id);

        Ok(())
    }

    /// Handle received snapshot
    pub fn handle_snapshot(&mut self, snapshot: Snapshot, peer_id: PeerId) -> Result<SyncAction> {
        let sequence = snapshot.sequence_number;

        // Remove from pending requests
        if let Some(request) = self.pending_snapshot_requests.remove(&sequence) {
            if request.peer_id != peer_id {
                warn!(
                    "Received snapshot {} from unexpected peer {}",
                    sequence, peer_id
                );
            }
        }

        // Store for verification
        let pending = PendingSnapshot {
            snapshot: snapshot.clone(),
            peer_id,
            received_at: Instant::now(),
        };

        self.pending_snapshots.insert(sequence, pending);

        // Update state
        self.state = SyncState::VerifyingSnapshot { sequence };

        info!(
            "Received snapshot {} from peer {}, verifying...",
            sequence, peer_id
        );

        Ok(SyncAction::VerifySnapshot { snapshot })
    }

    /// Complete snapshot verification
    pub fn complete_snapshot_verification(
        &mut self,
        sequence: u64,
        valid: bool,
    ) -> Result<SyncAction> {
        if !valid {
            warn!("Snapshot {} verification failed", sequence);
            self.pending_snapshots.remove(&sequence);
            self.state = SyncState::Idle;
            return Ok(SyncAction::None);
        }

        let pending = self
            .pending_snapshots
            .remove(&sequence)
            .ok_or_else(|| NetworkError::Other(format!("Snapshot {} not found", sequence)))?;

        info!("Snapshot {} verified successfully", sequence);

        // Check if we need to download transactions
        // Verify that all transactions in the snapshot are available
        let snapshot = &pending.snapshot;
        let mut missing_transactions = Vec::new();
        
        for tx_digest in &snapshot.transactions {
            // Check if transaction is available locally
            if let Some(_tx) = self.downloaded_transactions.get(tx_digest) {
                // Transaction is available locally
                debug!("Transaction {} is available locally", tx_digest);
            } else {
                // Transaction is missing
                missing_transactions.push(*tx_digest);
                debug!("Transaction {} is missing", tx_digest);
            }
        }
        
        if !missing_transactions.is_empty() {
            // We need to download missing transactions
            debug!(
                "Snapshot {} has {} missing transactions, starting download",
                sequence,
                missing_transactions.len()
            );
            self.state = SyncState::DownloadingTransactions {
                from_sequence: sequence,
                to_sequence: sequence,
                started_at: std::time::Instant::now(),
            };
        } else {
            // All transactions are available, we're synced
            debug!("Snapshot {} is fully synced", sequence);
            self.state = SyncState::Synced { sequence };
        }

        Ok(SyncAction::ApplySnapshot {
            snapshot: pending.snapshot,
        })
    }

    /// Start transaction download
    pub fn start_transaction_download(
        &mut self,
        from_sequence: u64,
        to_sequence: u64,
        peer_id: PeerId,
    ) -> Result<()> {
        info!(
            "Starting transaction download from {} to {}",
            from_sequence, to_sequence
        );

        self.state = SyncState::DownloadingTransactions {
            from_sequence,
            to_sequence,
            started_at: Instant::now(),
        };

        self.request_transactions(from_sequence, to_sequence, peer_id)?;

        Ok(())
    }

    /// Request transactions from a peer
    fn request_transactions(
        &mut self,
        from_sequence: u64,
        to_sequence: u64,
        peer_id: PeerId,
    ) -> Result<()> {
        if self.pending_transaction_requests.len() >= self.max_concurrent_requests {
            return Err(NetworkError::Other(
                "Too many concurrent requests".to_string(),
            ));
        }

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let request = TransactionRequest {
            id: request_id,
            from_sequence,
            to_sequence,
            peer_id,
            requested_at: Instant::now(),
            retries: 0,
        };

        self.pending_transaction_requests
            .insert(request_id, request);
        debug!(
            "Requested transactions {}-{} from peer {}",
            from_sequence, to_sequence, peer_id
        );

        Ok(())
    }

    /// Handle received transactions
    pub fn handle_transactions(
        &mut self,
        transactions: Vec<Transaction>,
        peer_id: PeerId,
    ) -> Result<SyncAction> {
        info!(
            "Received {} transactions from peer {}",
            transactions.len(),
            peer_id
        );

        // Store transactions
        for tx in &transactions {
            let digest = tx.digest();
            self.downloaded_transactions.insert(digest, tx.clone());
        }

        // Check if we have all transactions we need
        if let SyncState::DownloadingTransactions {
            from_sequence,
            to_sequence,
            ..
        } = self.state
        {
            let expected_count = (to_sequence - from_sequence + 1) as usize;

            if self.downloaded_transactions.len() >= expected_count {
                // All transactions downloaded
                self.state = SyncState::ApplyingTransactions {
                    count: self.downloaded_transactions.len(),
                };

                let txs: Vec<Transaction> =
                    self.downloaded_transactions.values().cloned().collect();
                self.downloaded_transactions.clear();

                return Ok(SyncAction::ApplyTransactions { transactions: txs });
            }
        }

        Ok(SyncAction::None)
    }

    /// Complete transaction application
    pub fn complete_transaction_application(&mut self, sequence: u64) -> Result<()> {
        info!(
            "Completed transaction application, now at sequence {}",
            sequence
        );
        self.state = SyncState::Synced { sequence };
        Ok(())
    }

    /// Handle timeout for pending requests
    pub fn handle_timeouts(&mut self) -> Vec<SyncAction> {
        let now = Instant::now();
        let mut actions = Vec::new();

        // Check snapshot request timeouts
        let mut timed_out_snapshots = Vec::new();
        for (sequence, request) in &self.pending_snapshot_requests {
            if now.duration_since(request.requested_at) > self.timeout {
                timed_out_snapshots.push(*sequence);
            }
        }

        for sequence in timed_out_snapshots {
            if let Some(mut request) = self.pending_snapshot_requests.remove(&sequence) {
                warn!(
                    "Snapshot request {} timed out (retry {})",
                    sequence, request.retries
                );

                if request.retries < 3 {
                    request.retries += 1;
                    request.requested_at = Instant::now();
                    self.pending_snapshot_requests
                        .insert(sequence, request.clone());
                    actions.push(SyncAction::RetrySnapshotRequest {
                        sequence,
                        peer_id: request.peer_id,
                    });
                } else {
                    warn!(
                        "Snapshot request {} failed after {} retries",
                        sequence, request.retries
                    );
                    self.state = SyncState::Idle;
                }
            }
        }

        // Check transaction request timeouts
        let mut timed_out_transactions = Vec::new();
        for (id, request) in &self.pending_transaction_requests {
            if now.duration_since(request.requested_at) > self.timeout {
                timed_out_transactions.push(*id);
            }
        }

        for id in timed_out_transactions {
            if let Some(mut request) = self.pending_transaction_requests.remove(&id) {
                warn!(
                    "Transaction request {} timed out (retry {})",
                    id, request.retries
                );

                if request.retries < 3 {
                    request.retries += 1;
                    request.requested_at = Instant::now();
                    self.pending_transaction_requests
                        .insert(id, request.clone());
                    actions.push(SyncAction::RetryTransactionRequest {
                        from_sequence: request.from_sequence,
                        to_sequence: request.to_sequence,
                        peer_id: request.peer_id,
                    });
                } else {
                    warn!(
                        "Transaction request {} failed after {} retries",
                        id, request.retries
                    );
                    self.state = SyncState::Idle;
                }
            }
        }

        actions
    }

    /// Get sync progress
    pub fn get_progress(&self) -> SyncProgress {
        match &self.state {
            SyncState::Idle => SyncProgress {
                state: "idle".to_string(),
                progress: 0.0,
                pending_requests: 0,
            },
            SyncState::DownloadingSnapshot { .. } => SyncProgress {
                state: "downloading_snapshot".to_string(),
                progress: 0.5,
                pending_requests: self.pending_snapshot_requests.len(),
            },
            SyncState::VerifyingSnapshot { .. } => SyncProgress {
                state: "verifying_snapshot".to_string(),
                progress: 0.75,
                pending_requests: 0,
            },
            SyncState::DownloadingTransactions {
                from_sequence,
                to_sequence,
                ..
            } => {
                let expected = (to_sequence - from_sequence + 1) as f64;
                let downloaded = self.downloaded_transactions.len() as f64;
                let progress = (downloaded / expected).min(1.0);

                SyncProgress {
                    state: "downloading_transactions".to_string(),
                    progress,
                    pending_requests: self.pending_transaction_requests.len(),
                }
            }
            SyncState::ApplyingTransactions { .. } => SyncProgress {
                state: "applying_transactions".to_string(),
                progress: 0.95,
                pending_requests: 0,
            },
            SyncState::Synced { sequence } => SyncProgress {
                state: format!("synced (sequence: {})", sequence),
                progress: 1.0,
                pending_requests: 0,
            },
        }
    }

    /// Reset sync state
    pub fn reset(&mut self) {
        self.state = SyncState::Idle;
        self.pending_snapshot_requests.clear();
        self.pending_transaction_requests.clear();
        self.pending_snapshots.clear();
        self.downloaded_transactions.clear();
        info!("Reset sync state");
    }
}

impl Default for StateSync {
    fn default() -> Self {
        Self::new()
    }
}

/// Action to take after sync event
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// No action needed
    None,

    /// Verify a snapshot
    VerifySnapshot {
        /// Snapshot to verify
        snapshot: Snapshot,
    },

    /// Apply a verified snapshot
    ApplySnapshot {
        /// Snapshot to apply
        snapshot: Snapshot,
    },

    /// Apply downloaded transactions
    ApplyTransactions {
        /// Transactions to apply
        transactions: Vec<Transaction>,
    },

    /// Retry snapshot request
    RetrySnapshotRequest {
        /// Sequence number
        sequence: u64,
        /// Peer to request from
        peer_id: PeerId,
    },

    /// Retry transaction request
    RetryTransactionRequest {
        /// From sequence
        from_sequence: u64,
        /// To sequence
        to_sequence: u64,
        /// Peer to request from
        peer_id: PeerId,
    },
}

/// Sync progress information
#[derive(Debug, Clone)]
pub struct SyncProgress {
    /// Current state description
    pub state: String,

    /// Progress (0.0 to 1.0)
    pub progress: f64,

    /// Number of pending requests
    pub pending_requests: usize,
}
