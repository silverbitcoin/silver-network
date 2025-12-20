use crate::{NetworkError, Result};
use dashmap::DashMap;
use libp2p::PeerId;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Rate limiter for peer messages
pub struct RateLimiter {
    /// Rate limit per peer (messages per second)
    rate_limit: u32,

    /// Time window for rate limiting
    window: Duration,

    /// Message timestamps per peer
    peer_messages: Arc<DashMap<PeerId, VecDeque<Instant>>>,
}

impl RateLimiter {
    /// Create a new RateLimiter
    pub fn new(rate_limit: u32) -> Self {
        Self {
            rate_limit,
            window: Duration::from_secs(1),
            peer_messages: Arc::new(DashMap::new()),
        }
    }

    /// Create with custom window
    pub fn with_window(rate_limit: u32, window: Duration) -> Self {
        Self {
            rate_limit,
            window,
            peer_messages: Arc::new(DashMap::new()),
        }
    }

    /// Check if a message from a peer should be allowed
    pub fn check_rate_limit(&self, peer_id: &PeerId) -> Result<()> {
        let now = Instant::now();

        // Get or create message queue for peer
        let mut entry = self
            .peer_messages
            .entry(*peer_id)
            .or_default();

        // Remove old messages outside the window
        while let Some(&front) = entry.front() {
            if now.duration_since(front) > self.window {
                entry.pop_front();
            } else {
                break;
            }
        }

        // Check if rate limit exceeded
        if entry.len() >= self.rate_limit as usize {
            warn!(
                "Rate limit exceeded for peer {}: {} messages in {:?}",
                peer_id,
                entry.len(),
                self.window
            );
            return Err(NetworkError::RateLimitExceeded(peer_id.to_string()));
        }

        // Add current message timestamp
        entry.push_back(now);

        Ok(())
    }

    /// Get current message count for a peer
    pub fn get_message_count(&self, peer_id: &PeerId) -> usize {
        self.peer_messages
            .get(peer_id)
            .map(|entry| entry.len())
            .unwrap_or(0)
    }

    /// Clear rate limit data for a peer
    pub fn clear_peer(&self, peer_id: &PeerId) {
        self.peer_messages.remove(peer_id);
    }

    /// Clear all rate limit data
    pub fn clear_all(&self) {
        self.peer_messages.clear();
    }

    /// Get number of tracked peers
    pub fn tracked_peers_count(&self) -> usize {
        self.peer_messages.len()
    }
}

/// Peer reputation system
pub struct PeerReputation {
    /// Reputation scores per peer (0-100)
    scores: Arc<DashMap<PeerId, ReputationScore>>,

    /// Blocklist of banned peers
    blocklist: Arc<DashMap<PeerId, BlocklistEntry>>,

    /// Reputation decay rate (points per hour)
    decay_rate: u8,

    /// Ban threshold (reputation below this = banned)
    ban_threshold: u8,

    /// Ban duration
    ban_duration: Duration,
}

/// Reputation score information
#[derive(Debug, Clone)]
struct ReputationScore {
    /// Current score (0-100)
    score: u8,

    /// Last update timestamp
    last_update: Instant,

    /// Number of good behaviors
    good_count: u64,

    /// Number of bad behaviors
    bad_count: u64,
}

/// Blocklist entry
#[derive(Debug, Clone)]
struct BlocklistEntry {
    /// Peer ID (kept for blocklist management and logging)
    #[allow(dead_code)]
    peer_id: PeerId,

    /// Reason for ban
    reason: String,

    /// Ban timestamp
    banned_at: Instant,

    /// Ban duration
    duration: Duration,
}

impl PeerReputation {
    /// Create a new PeerReputation system
    pub fn new() -> Self {
        Self {
            scores: Arc::new(DashMap::new()),
            blocklist: Arc::new(DashMap::new()),
            decay_rate: 1, // 1 point per hour
            ban_threshold: 10,
            ban_duration: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Create with custom configuration
    pub fn with_config(decay_rate: u8, ban_threshold: u8, ban_duration: Duration) -> Self {
        Self {
            scores: Arc::new(DashMap::new()),
            blocklist: Arc::new(DashMap::new()),
            decay_rate,
            ban_threshold,
            ban_duration,
        }
    }

    /// Get reputation score for a peer
    pub fn get_score(&self, peer_id: &PeerId) -> u8 {
        self.scores
            .get(peer_id)
            .map(|entry| self.apply_decay(&entry))
            .unwrap_or(50) // Default neutral score
    }

    /// Apply time-based decay to reputation
    fn apply_decay(&self, score: &ReputationScore) -> u8 {
        let elapsed = Instant::now().duration_since(score.last_update);
        let hours = elapsed.as_secs() / 3600;
        let decay = (hours as u8).saturating_mul(self.decay_rate);

        // Decay towards neutral (50)
        if score.score > 50 {
            score.score.saturating_sub(decay).max(50)
        } else {
            score.score.saturating_add(decay).min(50)
        }
    }

    /// Increase reputation for good behavior
    pub fn increase_reputation(&self, peer_id: &PeerId, amount: u8) {
        let mut entry = self
            .scores
            .entry(*peer_id)
            .or_insert_with(|| ReputationScore {
                score: 50,
                last_update: Instant::now(),
                good_count: 0,
                bad_count: 0,
            });

        let current = self.apply_decay(&entry);
        entry.score = current.saturating_add(amount).min(100);
        entry.last_update = Instant::now();
        entry.good_count += 1;

        debug!(
            "Increased reputation for peer {} to {}",
            peer_id, entry.score
        );
    }

    /// Decrease reputation for bad behavior
    pub fn decrease_reputation(&self, peer_id: &PeerId, amount: u8, reason: &str) {
        let mut entry = self
            .scores
            .entry(*peer_id)
            .or_insert_with(|| ReputationScore {
                score: 50,
                last_update: Instant::now(),
                good_count: 0,
                bad_count: 0,
            });

        let current = self.apply_decay(&entry);
        entry.score = current.saturating_sub(amount);
        entry.last_update = Instant::now();
        entry.bad_count += 1;

        warn!(
            "Decreased reputation for peer {} to {} (reason: {})",
            peer_id, entry.score, reason
        );

        // Auto-ban if below threshold
        if entry.score < self.ban_threshold {
            drop(entry); // Release the lock before calling ban_peer
            self.ban_peer(peer_id, reason);
        }
    }

    /// Ban a peer
    pub fn ban_peer(&self, peer_id: &PeerId, reason: &str) {
        let entry = BlocklistEntry {
            peer_id: *peer_id,
            reason: reason.to_string(),
            banned_at: Instant::now(),
            duration: self.ban_duration,
        };

        self.blocklist.insert(*peer_id, entry);

        // Set reputation to 0 and update timestamp
        let mut score_entry = self
            .scores
            .entry(*peer_id)
            .or_insert_with(|| ReputationScore {
                score: 50,
                last_update: Instant::now(),
                good_count: 0,
                bad_count: 0,
            });
        score_entry.score = 0;
        score_entry.last_update = Instant::now();

        warn!(
            "Banned peer {} for {} (reason: {})",
            peer_id,
            humantime::format_duration(self.ban_duration),
            reason
        );
    }

    /// Unban a peer
    pub fn unban_peer(&self, peer_id: &PeerId) {
        if self.blocklist.remove(peer_id).is_some() {
            // Reset reputation to neutral
            let mut entry = self
                .scores
                .entry(*peer_id)
                .or_insert_with(|| ReputationScore {
                    score: 50,
                    last_update: Instant::now(),
                    good_count: 0,
                    bad_count: 0,
                });
            entry.score = 50;
            entry.last_update = Instant::now();
            debug!("Unbanned peer {}", peer_id);
        }
    }

    /// Check if a peer is banned
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        if let Some(entry) = self.blocklist.get(peer_id) {
            let elapsed = Instant::now().duration_since(entry.banned_at);
            if elapsed < entry.duration {
                return true;
            } else {
                // Ban expired, remove from blocklist
                drop(entry);
                self.unban_peer(peer_id);
                return false;
            }
        }
        false
    }

    /// Get ban reason for a peer
    pub fn get_ban_reason(&self, peer_id: &PeerId) -> Option<String> {
        self.blocklist
            .get(peer_id)
            .map(|entry| entry.reason.clone())
    }

    /// Remove expired bans
    pub fn cleanup_expired_bans(&self) -> usize {
        let now = Instant::now();
        let mut removed = 0;

        self.blocklist.retain(|_, entry| {
            let elapsed = now.duration_since(entry.banned_at);
            if elapsed >= entry.duration {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            debug!("Removed {} expired bans", removed);
        }

        removed
    }

    /// Get reputation statistics
    pub fn get_stats(&self) -> ReputationStats {
        let total_peers = self.scores.len();
        let banned_peers = self.blocklist.len();

        let mut good_peers = 0;
        let mut neutral_peers = 0;
        let mut bad_peers = 0;

        for entry in self.scores.iter() {
            let score = self.apply_decay(&entry);
            if score >= 70 {
                good_peers += 1;
            } else if score >= 30 {
                neutral_peers += 1;
            } else {
                bad_peers += 1;
            }
        }

        ReputationStats {
            total_peers,
            good_peers,
            neutral_peers,
            bad_peers,
            banned_peers,
        }
    }

    /// Clear all reputation data
    pub fn clear_all(&self) {
        self.scores.clear();
        self.blocklist.clear();
    }
}

impl Default for PeerReputation {
    fn default() -> Self {
        Self::new()
    }
}

/// Reputation statistics
#[derive(Debug, Clone)]
pub struct ReputationStats {
    /// Total number of peers
    pub total_peers: usize,

    /// Number of good peers (score >= 70)
    pub good_peers: usize,

    /// Number of neutral peers (30 <= score < 70)
    pub neutral_peers: usize,

    /// Number of bad peers (score < 30)
    pub bad_peers: usize,

    /// Number of banned peers
    pub banned_peers: usize,
}
