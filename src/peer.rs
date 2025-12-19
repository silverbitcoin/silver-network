use dashmap::DashMap;
use multiaddr::Multiaddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub use libp2p::PeerId;

/// Information about a connected peer.
///
/// Tracks peer metadata, connection status, and statistics.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer ID
    #[allow(dead_code)]
    pub peer_id: PeerId,

    /// Peer addresses
    pub addresses: Vec<Multiaddr>,

    /// Connection status
    pub status: PeerStatus,

    /// Last seen timestamp
    pub last_seen: Instant,

    /// Number of messages sent to this peer
    pub messages_sent: u64,

    /// Number of messages received from this peer
    pub messages_received: u64,

    /// Reputation score (0-100)
    pub reputation: u8,

    /// Whether this is a mining pool peer
    pub is_mining_pool: bool,

    /// Protocol version
    pub protocol_version: Option<String>,

    /// Agent version
    pub agent_version: Option<String>,
}

/// Peer connection status.
///
/// Represents the current state of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    /// Connecting to peer
    Connecting,

    /// Connected and active
    Connected,

    /// Disconnected
    Disconnected,

    /// Banned due to misbehavior
    Banned,
}

impl PeerInfo {
    /// Create a new PeerInfo
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            addresses: Vec::new(),
            status: PeerStatus::Connecting,
            last_seen: Instant::now(),
            messages_sent: 0,
            messages_received: 0,
            reputation: 50, // Start with neutral reputation
            is_mining_pool: false,
            protocol_version: None,
            agent_version: None,
        }
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Increment messages sent counter
    pub fn increment_sent(&mut self) {
        self.messages_sent = self.messages_sent.saturating_add(1);
    }

    /// Increment messages received counter
    pub fn increment_received(&mut self) {
        self.messages_received = self.messages_received.saturating_add(1);
    }

    /// Increase reputation (max 100)
    pub fn increase_reputation(&mut self, amount: u8) {
        self.reputation = self.reputation.saturating_add(amount).min(100);
    }

    /// Decrease reputation (min 0)
    pub fn decrease_reputation(&mut self, amount: u8) {
        self.reputation = self.reputation.saturating_sub(amount);
    }

    /// Check if peer is healthy
    pub fn is_healthy(&self) -> bool {
        self.status == PeerStatus::Connected && self.reputation >= 30
    }

    /// Check if peer should be banned
    pub fn should_ban(&self) -> bool {
        self.reputation < 10
    }
}

/// Manages peer connections and information
pub struct PeerManager {
    /// Connected peers
    peers: Arc<DashMap<PeerId, PeerInfo>>,

    /// Maximum number of peers
    max_peers: usize,

    /// Minimum number of peers
    min_peers: usize,

    /// Peer timeout duration
    timeout: Duration,
}

impl PeerManager {
    /// Create a new PeerManager
    pub fn new(max_peers: usize, min_peers: usize, timeout: Duration) -> Self {
        Self {
            peers: Arc::new(DashMap::new()),
            max_peers,
            min_peers,
            timeout,
        }
    }

    /// Add a new peer
    pub fn add_peer(&self, peer_id: PeerId) -> bool {
        if self.peers.len() >= self.max_peers {
            warn!(
                "Maximum peer limit reached ({}), rejecting peer {}",
                self.max_peers, peer_id
            );
            return false;
        }

        if self.peers.contains_key(&peer_id) {
            debug!("Peer {} already exists", peer_id);
            return false;
        }

        let peer_info = PeerInfo::new(peer_id);
        self.peers.insert(peer_id, peer_info);
        info!("Added peer {}, total peers: {}", peer_id, self.peers.len());
        true
    }

    /// Remove a peer
    pub fn remove_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        let removed = self.peers.remove(peer_id);
        if removed.is_some() {
            info!(
                "Removed peer {}, total peers: {}",
                peer_id,
                self.peers.len()
            );
        }
        removed.map(|(_, info)| info)
    }

    /// Get peer information
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.get(peer_id).map(|entry| entry.clone())
    }

    /// Update peer status
    pub fn update_peer_status(&self, peer_id: &PeerId, status: PeerStatus) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.status = status;
            peer.update_last_seen();
            debug!("Updated peer {} status to {:?}", peer_id, status);
        }
    }

    /// Update peer addresses
    pub fn update_peer_addresses(&self, peer_id: &PeerId, addresses: Vec<Multiaddr>) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.addresses = addresses;
            peer.update_last_seen();
        }
    }

    /// Update peer protocol information
    pub fn update_peer_protocol(
        &self,
        peer_id: &PeerId,
        protocol_version: String,
        agent_version: String,
    ) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.protocol_version = Some(protocol_version);
            peer.agent_version = Some(agent_version);
            peer.update_last_seen();
        }
    }

    /// Mark peer as mining pool
    pub fn mark_as_mining_pool(&self, peer_id: &PeerId) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.is_mining_pool = true;
        }
    }

    /// Record message sent to peer
    pub fn record_message_sent(&self, peer_id: &PeerId) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.increment_sent();
            peer.update_last_seen();
        }
    }

    /// Record message received from peer
    pub fn record_message_received(&self, peer_id: &PeerId) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.increment_received();
            peer.update_last_seen();
        }
    }

    /// Increase peer reputation
    pub fn increase_reputation(&self, peer_id: &PeerId, amount: u8) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.increase_reputation(amount);
            debug!(
                "Increased reputation for peer {} to {}",
                peer_id, peer.reputation
            );
        }
    }

    /// Decrease peer reputation
    pub fn decrease_reputation(&self, peer_id: &PeerId, amount: u8) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.decrease_reputation(amount);
            warn!(
                "Decreased reputation for peer {} to {}",
                peer_id, peer.reputation
            );

            if peer.should_ban() {
                peer.status = PeerStatus::Banned;
                warn!("Banned peer {} due to low reputation", peer_id);
            }
        }
    }

    /// Ban a peer
    pub fn ban_peer(&self, peer_id: &PeerId) {
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.status = PeerStatus::Banned;
            peer.reputation = 0;
            warn!("Banned peer {}", peer_id);
        }
    }

    /// Get all connected peers
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|entry| entry.status == PeerStatus::Connected)
            .map(|entry| entry.peer_id)
            .collect()
    }

    /// Get all mining pool peers
    pub fn mining_pool_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|entry| entry.is_mining_pool && entry.status == PeerStatus::Connected)
            .map(|entry| entry.peer_id)
            .collect()
    }

    /// Get number of connected peers
    pub fn connected_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|entry| entry.status == PeerStatus::Connected)
            .count()
    }

    /// Check if we need more peers
    pub fn needs_more_peers(&self) -> bool {
        self.connected_count() < self.min_peers
    }

    /// Check if we have too many peers
    pub fn has_too_many_peers(&self) -> bool {
        self.connected_count() > self.max_peers
    }

    /// Remove timed out peers
    pub fn remove_timed_out_peers(&self) -> Vec<PeerId> {
        let now = Instant::now();
        let mut timed_out = Vec::new();

        self.peers.retain(|peer_id, peer_info| {
            if peer_info.status == PeerStatus::Connected
                && now.duration_since(peer_info.last_seen) > self.timeout
            {
                timed_out.push(*peer_id);
                warn!("Peer {} timed out", peer_id);
                false
            } else {
                true
            }
        });

        timed_out
    }

    /// Get peer statistics
    pub fn get_stats(&self) -> PeerStats {
        let total = self.peers.len();
        let connected = self.connected_count();
        let mining_pools = self.mining_pool_peers().len();
        let banned = self
            .peers
            .iter()
            .filter(|entry| entry.status == PeerStatus::Banned)
            .count();

        PeerStats {
            total,
            connected,
            mining_pools,
            banned,
        }
    }
}

/// Peer statistics
#[derive(Debug, Clone)]
pub struct PeerStats {
    /// Total number of peers
    pub total: usize,

    /// Number of connected peers
    pub connected: usize,

    /// Number of mining pool peers
    pub mining_pools: usize,

    /// Number of banned peers
    pub banned: usize,
}
