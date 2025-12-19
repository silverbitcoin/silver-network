//! Mining pool support for Proof-of-Work mining
//!
//! This module provides:
//! - Work package distribution to miners
//! - Miner share collection and validation
//! - Pool statistics and monitoring
//! - Miner account management

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

/// Work package for miners
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPackage {
    /// Unique work ID
    pub work_id: u64,
    /// Block header data
    pub header: Vec<u8>,
    /// Target difficulty
    pub target: u64,
    /// Chain ID
    pub chain_id: u32,
    /// Creation timestamp
    pub created_at: u64,
}

/// Miner share submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerShare {
    /// Work ID
    pub work_id: u64,
    /// Nonce found by miner
    pub nonce: u64,
    /// Miner address
    pub miner_address: Vec<u8>,
    /// Submission timestamp
    pub submitted_at: u64,
}

/// Miner account information
#[derive(Debug, Clone)]
pub struct MinerAccount {
    /// Miner address
    pub address: Vec<u8>,
    /// Total shares submitted
    pub total_shares: u64,
    /// Valid shares
    pub valid_shares: u64,
    /// Invalid shares
    pub invalid_shares: u64,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Estimated hash rate (hashes per second)
    pub hash_rate: f64,
}

impl MinerAccount {
    /// Create a new miner account
    pub fn new(address: Vec<u8>) -> Self {
        Self {
            address,
            total_shares: 0,
            valid_shares: 0,
            invalid_shares: 0,
            last_activity: Instant::now(),
            hash_rate: 0.0,
        }
    }

    /// Record a valid share
    pub fn record_valid_share(&mut self) {
        self.total_shares += 1;
        self.valid_shares += 1;
        self.last_activity = Instant::now();
    }

    /// Record an invalid share
    pub fn record_invalid_share(&mut self) {
        self.total_shares += 1;
        self.invalid_shares += 1;
        self.last_activity = Instant::now();
    }

    /// Get acceptance rate
    pub fn acceptance_rate(&self) -> f64 {
        if self.total_shares == 0 {
            0.0
        } else {
            self.valid_shares as f64 / self.total_shares as f64
        }
    }
}

/// Mining pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total work packages distributed
    pub total_work_packages: u64,
    /// Total shares received
    pub total_shares: u64,
    /// Valid shares
    pub valid_shares: u64,
    /// Invalid shares
    pub invalid_shares: u64,
    /// Number of active miners
    pub active_miners: usize,
    /// Pool hash rate (hashes per second)
    pub pool_hash_rate: f64,
}

/// Mining pool manager
pub struct MiningPoolManager {
    /// Active work packages
    work_packages: Arc<DashMap<u64, WorkPackage>>,
    /// Miner accounts
    miners: Arc<DashMap<Vec<u8>, MinerAccount>>,
    /// Pool statistics
    stats: Arc<parking_lot::RwLock<PoolStats>>,
    /// Next work ID
    next_work_id: Arc<std::sync::atomic::AtomicU64>,
}

impl MiningPoolManager {
    /// Create a new mining pool manager
    pub fn new() -> Self {
        Self {
            work_packages: Arc::new(DashMap::new()),
            miners: Arc::new(DashMap::new()),
            stats: Arc::new(parking_lot::RwLock::new(PoolStats {
                total_work_packages: 0,
                total_shares: 0,
                valid_shares: 0,
                invalid_shares: 0,
                active_miners: 0,
                pool_hash_rate: 0.0,
            })),
            next_work_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Distribute a work package to miners
    pub fn distribute_work(&self, header: Vec<u8>, target: u64, chain_id: u32) -> WorkPackage {
        let work_id = self
            .next_work_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let work = WorkPackage {
            work_id,
            header,
            target,
            chain_id,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        self.work_packages.insert(work_id, work.clone());

        let mut stats = self.stats.write();
        stats.total_work_packages += 1;

        debug!("Distributed work package {}", work_id);
        work
    }

    /// Submit a miner share
    pub fn submit_share(&self, share: MinerShare) -> bool {
        // Verify work package exists
        if !self.work_packages.contains_key(&share.work_id) {
            warn!("Share submitted for unknown work package {}", share.work_id);
            return false;
        }

        // Get or create miner account
        let mut miner = self
            .miners
            .entry(share.miner_address.clone())
            .or_insert_with(|| MinerAccount::new(share.miner_address.clone()));

        // Record share
        miner.record_valid_share();

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_shares += 1;
        stats.valid_shares += 1;
        stats.active_miners = self.miners.len();

        debug!(
            "Recorded share from miner {:?} for work {}",
            share.miner_address, share.work_id
        );

        true
    }

    /// Get miner account
    pub fn get_miner(&self, address: &[u8]) -> Option<MinerAccount> {
        self.miners.get(address).map(|m| m.clone())
    }

    /// Get pool statistics
    pub fn get_stats(&self) -> PoolStats {
        self.stats.read().clone()
    }

    /// Clean up old work packages
    pub fn cleanup_old_work(&self, max_age_secs: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.work_packages.retain(|_, work| now - work.created_at < max_age_secs);
    }

    /// Get active miner count
    pub fn active_miner_count(&self) -> usize {
        self.miners.len()
    }
}

impl Default for MiningPoolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mining_pool_creation() {
        let pool = MiningPoolManager::new();
        assert_eq!(pool.active_miner_count(), 0);
    }

    #[test]
    fn test_work_distribution() {
        let pool = MiningPoolManager::new();
        let work = pool.distribute_work(vec![1, 2, 3], 1000, 0);
        assert_eq!(work.work_id, 1);
        assert_eq!(work.target, 1000);
    }

    #[test]
    fn test_share_submission() {
        let pool = MiningPoolManager::new();
        let work = pool.distribute_work(vec![1, 2, 3], 1000, 0);

        let share = MinerShare {
            work_id: work.work_id,
            nonce: 12345,
            miner_address: vec![1, 2, 3],
            submitted_at: 0,
        };

        assert!(pool.submit_share(share));
        assert_eq!(pool.active_miner_count(), 1);
    }

    #[test]
    fn test_miner_account() {
        let mut account = MinerAccount::new(vec![1, 2, 3]);
        account.record_valid_share();
        account.record_valid_share();
        account.record_invalid_share();

        assert_eq!(account.total_shares, 3);
        assert_eq!(account.valid_shares, 2);
        assert_eq!(account.invalid_shares, 1);
        assert!((account.acceptance_rate() - 2.0 / 3.0).abs() < 0.001);
    }
}
