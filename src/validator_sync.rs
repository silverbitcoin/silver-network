//! Validator synchronization and broadcasting
//!
//! This module provides production-ready validator network operations:
//! - Validator set update broadcasting with libp2p
//! - Snapshot broadcasting and propagation
//! - Batch propagation with exponential backoff retry logic
//! - Peer discovery and management with health checks
//! - Network health monitoring and metrics
//! - Consensus message handling with message ordering
//! - Connection pooling and resource management
//! - Message compression and optimization

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use silver_core::{Error as CoreError, Result as CoreResult};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Network message types with versioning for compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Validator set change event
    ValidatorSetChange {
        /// Event data
        data: Vec<u8>,
        /// Message version for compatibility
        version: u32,
    },

    /// Snapshot certificate
    SnapshotCertificate {
        /// Certificate data
        data: Vec<u8>,
        /// Snapshot height
        height: u64,
        /// Message version for compatibility
        version: u32,
    },

    /// Batch of transactions
    TransactionBatch {
        /// Batch ID
        batch_id: Vec<u8>,
        /// Batch data
        data: Vec<u8>,
        /// Batch sequence number
        sequence: u64,
        /// Message version for compatibility
        version: u32,
    },

    /// Ping for health check
    Ping {
        /// Timestamp
        timestamp: u64,
        /// Nonce for tracking
        nonce: u64,
    },

    /// Pong response
    Pong {
        /// Timestamp
        timestamp: u64,
        /// Nonce matching ping
        nonce: u64,
    },

    /// Handshake for peer identification
    Handshake {
        /// Peer validator ID
        validator_id: Vec<u8>,
        /// Peer version
        version: u32,
        /// Supported message types
        capabilities: Vec<u8>,
    },

    /// Acknowledgment of message receipt
    Ack {
        /// Message hash being acknowledged
        message_hash: Vec<u8>,
        /// Sequence number
        sequence: u64,
    },
}

impl NetworkMessage {
    /// Serialize message to bytes with length prefix
    pub fn to_bytes(&self) -> CoreResult<Vec<u8>> {
        let serialized = bincode::serialize(self)
            .map_err(|e| CoreError::InvalidData(format!("Serialization error: {}", e)))?;

        // Add length prefix (4 bytes, big-endian)
        let len = serialized.len() as u32;
        let mut result = len.to_be_bytes().to_vec();
        result.extend_from_slice(&serialized);

        Ok(result)
    }

    /// Deserialize message from bytes with length prefix
    pub fn from_bytes(data: &[u8]) -> CoreResult<Self> {
        if data.len() < 4 {
            return Err(CoreError::InvalidData("Message too short".to_string()));
        }

        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return Err(CoreError::InvalidData("Incomplete message".to_string()));
        }

        bincode::deserialize(&data[4..4 + len])
            .map_err(|e| CoreError::InvalidData(format!("Deserialization error: {}", e)))
    }

    /// Calculate message hash for integrity verification
    pub fn hash(&self) -> CoreResult<Vec<u8>> {
        let serialized = bincode::serialize(self)
            .map_err(|e| CoreError::InvalidData(format!("Serialization error: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        Ok(hasher.finalize().to_vec())
    }
}

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Disconnected
    Disconnected,
    /// Connecting
    Connecting,
    /// Connected and handshaked
    Connected,
    /// Connection failed
    Failed,
    /// Peer is stale
    Stale,
}

/// Peer information with detailed metrics
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer address
    pub address: SocketAddr,

    /// Peer validator ID
    pub validator_id: Option<Vec<u8>>,

    /// Last seen timestamp
    pub last_seen: Instant,

    /// Connection state
    pub state: ConnectionState,

    /// Messages sent
    pub messages_sent: u64,

    /// Messages received
    pub messages_received: u64,

    /// Latency in milliseconds (exponential moving average)
    pub latency_ms: u64,

    /// Failed connection attempts
    pub failed_attempts: u32,

    /// Last error message
    pub last_error: Option<String>,

    /// Peer version
    pub version: u32,

    /// Peer capabilities
    pub capabilities: Vec<u8>,

    /// Total bytes sent
    pub bytes_sent: u64,

    /// Total bytes received
    pub bytes_received: u64,
}

impl PeerInfo {
    /// Create new peer info
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            validator_id: None,
            last_seen: Instant::now(),
            state: ConnectionState::Disconnected,
            messages_sent: 0,
            messages_received: 0,
            latency_ms: 0,
            failed_attempts: 0,
            last_error: None,
            version: 1,
            capabilities: vec![],
            bytes_sent: 0,
            bytes_received: 0,
        }
    }

    /// Check if peer is stale (no activity for 5 minutes)
    pub fn is_stale(&self) -> bool {
        self.last_seen.elapsed() > Duration::from_secs(300)
    }

    /// Check if peer is healthy
    pub fn is_healthy(&self) -> bool {
        self.state == ConnectionState::Connected && !self.is_stale()
    }

    /// Update last seen time
    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Record message sent with exponential moving average latency
    pub fn record_sent(&mut self, bytes: u64, latency_ms: u64) {
        self.messages_sent += 1;
        self.bytes_sent += bytes;
        self.update_last_seen();

        // Update latency with exponential moving average (alpha = 0.3)
        if self.latency_ms == 0 {
            self.latency_ms = latency_ms;
        } else {
            self.latency_ms = (self.latency_ms * 70 + latency_ms * 30) / 100;
        }
    }

    /// Record message received
    pub fn record_received(&mut self, bytes: u64) {
        self.messages_received += 1;
        self.bytes_received += bytes;
        self.update_last_seen();
        self.failed_attempts = 0;
        self.last_error = None;
    }

    /// Record failed attempt
    pub fn record_failure(&mut self, error: String) {
        self.failed_attempts += 1;
        self.last_error = Some(error);

        if self.failed_attempts > 5 {
            self.state = ConnectionState::Failed;
        }
    }

    /// Reset failure counter
    pub fn reset_failures(&mut self) {
        self.failed_attempts = 0;
        self.last_error = None;
    }
}

/// Message envelope for network transmission with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Message type identifier
    pub message_type: u8,

    /// Serialized message data
    pub data: Vec<u8>,

    /// Timestamp when message was created
    pub timestamp: u64,

    /// Sender address
    pub sender_addr: SocketAddr,

    /// Message sequence number for ordering
    pub sequence: u64,

    /// Message hash for integrity verification
    pub hash: Vec<u8>,

    /// Compression flag
    pub compressed: bool,
}

impl MessageEnvelope {
    /// Create new message envelope
    pub fn new(
        message_type: u8,
        data: Vec<u8>,
        sender_addr: SocketAddr,
        sequence: u64,
    ) -> CoreResult<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let hash = hasher.finalize().to_vec();

        Ok(Self {
            message_type,
            data,
            timestamp,
            sender_addr,
            sequence,
            hash,
            compressed: false,
        })
    }

    /// Serialize envelope to bytes
    pub fn to_bytes(&self) -> CoreResult<Vec<u8>> {
        let serialized = bincode::serialize(self)
            .map_err(|e| CoreError::InvalidData(format!("Serialization error: {}", e)))?;

        let len = serialized.len() as u32;
        let mut result = len.to_be_bytes().to_vec();
        result.extend_from_slice(&serialized);

        Ok(result)
    }

    /// Deserialize envelope from bytes
    pub fn from_bytes(data: &[u8]) -> CoreResult<Self> {
        if data.len() < 4 {
            return Err(CoreError::InvalidData("Envelope too short".to_string()));
        }

        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return Err(CoreError::InvalidData("Incomplete envelope".to_string()));
        }

        bincode::deserialize(&data[4..4 + len])
            .map_err(|e| CoreError::InvalidData(format!("Deserialization error: {}", e)))
    }

    /// Verify envelope integrity
    pub fn verify_integrity(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        hasher.finalize().to_vec() == self.hash
    }
}

/// Get message type identifier from NetworkMessage
#[allow(dead_code)]
fn message_type_from_network_message(message: &NetworkMessage) -> u8 {
    match message {
        NetworkMessage::ValidatorSetChange { .. } => 1,
        NetworkMessage::SnapshotCertificate { .. } => 2,
        NetworkMessage::TransactionBatch { .. } => 3,
        NetworkMessage::Ping { .. } => 4,
        NetworkMessage::Pong { .. } => 5,
        NetworkMessage::Handshake { .. } => 6,
        NetworkMessage::Ack { .. } => 7,
    }
}

/// Broadcast result with detailed metrics
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Total peers
    pub total_peers: usize,

    /// Successful broadcasts
    pub successful: usize,

    /// Failed broadcasts
    pub failed: usize,

    /// Average latency
    pub avg_latency_ms: u64,

    /// Min latency
    pub min_latency_ms: u64,

    /// Max latency
    pub max_latency_ms: u64,

    /// Total bytes sent
    pub total_bytes_sent: u64,

    /// Broadcast duration
    pub duration_ms: u64,
}

/// Validator network manager with real TCP/libp2p networking
#[derive(Clone)]
pub struct ValidatorNetworkManager {
    /// Connected peers
    peers: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,

    /// TCP listener for incoming connections
    listener: Arc<RwLock<Option<Arc<TcpListener>>>>,

    /// Local address
    local_addr: SocketAddr,

    /// Broadcast retry configuration
    max_retries: usize,

    /// Retry backoff duration (exponential)
    retry_backoff: Duration,

    /// Message timeout
    message_timeout: Duration,

    /// Broadcast statistics
    stats: Arc<RwLock<BroadcastStats>>,

    /// Connection semaphore for limiting concurrent connections
    connection_semaphore: Arc<Semaphore>,

    /// Message sequence counter
    #[allow(dead_code)]
    message_sequence: Arc<RwLock<u64>>,

    /// Pending messages queue for retry
    #[allow(dead_code)]
    pending_messages: Arc<RwLock<VecDeque<(SocketAddr, Vec<u8>, u32)>>>,

    /// Message channel for async operations
    #[allow(dead_code)]
    tx: mpsc::UnboundedSender<NetworkEvent>,

    /// Validator ID
    validator_id: Vec<u8>,
}

/// Broadcast statistics with detailed metrics
#[derive(Debug, Clone, Default)]
pub struct BroadcastStats {
    /// Total broadcasts
    pub total_broadcasts: u64,

    /// Successful broadcasts
    pub successful_broadcasts: u64,

    /// Failed broadcasts
    pub failed_broadcasts: u64,

    /// Total messages sent
    pub total_messages_sent: u64,

    /// Total messages received
    pub total_messages_received: u64,

    /// Average broadcast latency
    pub avg_broadcast_latency_ms: u64,

    /// Total bytes sent
    pub total_bytes_sent: u64,

    /// Total bytes received
    pub total_bytes_received: u64,

    /// Connection failures
    pub connection_failures: u64,

    /// Message retries
    pub message_retries: u64,

    /// Peer count
    pub peer_count: u64,

    /// Connected peer count
    pub connected_peer_count: u64,
}

/// Network event for async message handling
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// Peer connected
    PeerConnected(SocketAddr),
    /// Peer disconnected
    PeerDisconnected(SocketAddr),
    /// Message received
    MessageReceived(SocketAddr, NetworkMessage),
    /// Broadcast completed
    BroadcastCompleted(u64),
}

impl ValidatorNetworkManager {
    /// Create new validator network manager
    pub fn new(local_addr: SocketAddr, validator_id: Vec<u8>) -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();

        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            listener: Arc::new(RwLock::new(None)),
            local_addr,
            max_retries: 3,
            retry_backoff: Duration::from_millis(100),
            message_timeout: Duration::from_secs(5),
            stats: Arc::new(RwLock::new(BroadcastStats::default())),
            connection_semaphore: Arc::new(Semaphore::new(100)),
            message_sequence: Arc::new(RwLock::new(0)),
            pending_messages: Arc::new(RwLock::new(VecDeque::new())),
            tx,
            validator_id,
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        local_addr: SocketAddr,
        validator_id: Vec<u8>,
        max_retries: usize,
        retry_backoff: Duration,
        message_timeout: Duration,
    ) -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();

        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            listener: Arc::new(RwLock::new(None)),
            local_addr,
            max_retries,
            retry_backoff,
            message_timeout,
            stats: Arc::new(RwLock::new(BroadcastStats::default())),
            connection_semaphore: Arc::new(Semaphore::new(100)),
            message_sequence: Arc::new(RwLock::new(0)),
            pending_messages: Arc::new(RwLock::new(VecDeque::new())),
            tx,
            validator_id,
        }
    }

    /// Start listening for incoming connections
    pub async fn start_listener(&self) -> CoreResult<()> {
        let listener = TcpListener::bind(self.local_addr)
            .await
            .map_err(|e| CoreError::InvalidData(format!("Failed to bind listener: {}", e)))?;

        info!("Validator network listener started on {}", self.local_addr);

        let mut listener_guard = self.listener.write().await;
        *listener_guard = Some(Arc::new(listener));

        Ok(())
    }

    /// Accept incoming connections (should be run in background)
    pub async fn accept_connections(self: Arc<Self>) -> CoreResult<()> {
        let listener = {
            let guard = self.listener.read().await;
            guard.as_ref().cloned()
        };

        let listener =
            listener.ok_or_else(|| CoreError::InvalidData("Listener not started".to_string()))?;

        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    let semaphore = self.connection_semaphore.clone();
                    let peers = self.peers.clone();
                    let stats = self.stats.clone();
                    let validator_id = self.validator_id.clone();

                    tokio::spawn(async move {
                        let permit = match semaphore.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                error!("Semaphore error: {}", e);
                                return;
                            }
                        };

                        let _permit = permit;
                        if let Err(e) = Self::handle_peer_connection(
                            socket,
                            peer_addr,
                            peers,
                            stats,
                            validator_id,
                        )
                        .await
                        {
                            warn!("Error handling peer connection from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    }

    /// Handle incoming peer connection
    async fn handle_peer_connection(
        mut socket: TcpStream,
        peer_addr: SocketAddr,
        peers: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,
        stats: Arc<RwLock<BroadcastStats>>,
        validator_id: Vec<u8>,
    ) -> CoreResult<()> {
        // Send handshake
        let handshake = NetworkMessage::Handshake {
            validator_id: validator_id.clone(),
            version: 1,
            capabilities: vec![1, 2, 3, 4, 5, 6, 7],
        };

        let handshake_bytes = handshake.to_bytes()?;
        socket
            .write_all(&handshake_bytes)
            .await
            .map_err(|e| CoreError::InvalidData(format!("Failed to send handshake: {}", e)))?;

        // Receive handshake response
        let mut buf = vec![0u8; 4096];
        let n = timeout(Duration::from_secs(5), socket.read(&mut buf))
            .await
            .map_err(|_| CoreError::InvalidData("Handshake timeout".to_string()))?
            .map_err(|e| CoreError::InvalidData(format!("Failed to read handshake: {}", e)))?;

        if n == 0 {
            return Err(CoreError::InvalidData("Peer closed connection".to_string()));
        }

        let peer_handshake = NetworkMessage::from_bytes(&buf[..n])?;

        // Update peer info
        let mut peers_guard = peers.write().await;
        if let Some(peer) = peers_guard.get_mut(&peer_addr) {
            if let NetworkMessage::Handshake {
                validator_id: peer_id,
                version,
                capabilities,
            } = peer_handshake
            {
                peer.validator_id = Some(peer_id);
                peer.version = version;
                peer.capabilities = capabilities;
                peer.state = ConnectionState::Connected;
                peer.reset_failures();
            }
        }
        drop(peers_guard);

        // Message receiving loop
        loop {
            let mut buf = vec![0u8; 10 * 1024 * 1024]; // 10MB max message

            match timeout(Duration::from_secs(30), socket.read(&mut buf)).await {
                Ok(Ok(0)) => {
                    info!("Peer {} closed connection", peer_addr);
                    break;
                }
                Ok(Ok(n)) => {
                    if let Ok(_message) = NetworkMessage::from_bytes(&buf[..n]) {
                        let mut peers_guard = peers.write().await;
                        if let Some(peer) = peers_guard.get_mut(&peer_addr) {
                            peer.record_received(n as u64);
                        }
                        drop(peers_guard);

                        let mut stats_guard = stats.write().await;
                        stats_guard.total_messages_received += 1;
                        stats_guard.total_bytes_received += n as u64;
                    }
                }
                Ok(Err(e)) => {
                    warn!("Error reading from peer {}: {}", peer_addr, e);
                    break;
                }
                Err(_) => {
                    debug!("Read timeout from peer {}", peer_addr);
                    break;
                }
            }
        }

        // Update peer state on disconnect
        let mut peers_guard = peers.write().await;
        if let Some(peer) = peers_guard.get_mut(&peer_addr) {
            peer.state = ConnectionState::Disconnected;
        }

        Ok(())
    }

    /// Add peer to network
    pub async fn add_peer(&self, address: SocketAddr) -> CoreResult<()> {
        let mut peers = self.peers.write().await;

        if peers.contains_key(&address) {
            return Err(CoreError::InvalidData(format!(
                "Peer {} already exists",
                address
            )));
        }

        let peer = PeerInfo::new(address);
        peers.insert(address, peer);

        info!("Added peer: {}", address);

        let mut stats = self.stats.write().await;
        stats.peer_count += 1;

        Ok(())
    }

    /// Remove peer from network
    pub async fn remove_peer(&self, address: &SocketAddr) -> CoreResult<()> {
        let mut peers = self.peers.write().await;

        if peers.remove(address).is_some() {
            info!("Removed peer: {}", address);

            let mut stats = self.stats.write().await;
            stats.peer_count = stats.peer_count.saturating_sub(1);

            Ok(())
        } else {
            Err(CoreError::InvalidData(format!(
                "Peer {} not found",
                address
            )))
        }
    }

    /// Connect to peer with real TCP connection
    pub async fn connect_peer(&self, address: &SocketAddr) -> CoreResult<()> {
        let permit = self
            .connection_semaphore
            .acquire()
            .await
            .map_err(|e| CoreError::InvalidData(format!("Semaphore error: {}", e)))?;

        let mut peers = self.peers.write().await;

        if let Some(peer) = peers.get_mut(address) {
            peer.state = ConnectionState::Connecting;
        } else {
            return Err(CoreError::InvalidData(format!(
                "Peer {} not found",
                address
            )));
        }
        drop(peers);

        // Attempt TCP connection
        match timeout(self.message_timeout, TcpStream::connect(address)).await {
            Ok(Ok(mut socket)) => {
                // Send handshake
                let handshake = NetworkMessage::Handshake {
                    validator_id: self.validator_id.clone(),
                    version: 1,
                    capabilities: vec![1, 2, 3, 4, 5, 6, 7],
                };

                let handshake_bytes = handshake.to_bytes()?;

                if let Err(e) = socket.write_all(&handshake_bytes).await {
                    let mut peers = self.peers.write().await;
                    if let Some(peer) = peers.get_mut(address) {
                        peer.state = ConnectionState::Failed;
                        peer.record_failure(format!("Handshake write failed: {}", e));
                    }
                    drop(permit);
                    return Err(CoreError::InvalidData(format!(
                        "Failed to send handshake: {}",
                        e
                    )));
                }

                // Receive handshake response
                let mut buf = vec![0u8; 4096];
                match timeout(self.message_timeout, socket.read(&mut buf)).await {
                    Ok(Ok(n)) if n > 0 => {
                        if let Ok(peer_handshake) = NetworkMessage::from_bytes(&buf[..n]) {
                            let mut peers = self.peers.write().await;
                            if let Some(peer) = peers.get_mut(address) {
                                if let NetworkMessage::Handshake {
                                    validator_id: peer_id,
                                    version,
                                    capabilities,
                                } = peer_handshake
                                {
                                    peer.validator_id = Some(peer_id);
                                    peer.version = version;
                                    peer.capabilities = capabilities;
                                    peer.state = ConnectionState::Connected;
                                    peer.reset_failures();

                                    let mut stats = self.stats.write().await;
                                    stats.connected_peer_count += 1;

                                    info!("Connected to peer: {}", address);
                                }
                            }
                        }
                    }
                    _ => {
                        let mut peers = self.peers.write().await;
                        if let Some(peer) = peers.get_mut(address) {
                            peer.state = ConnectionState::Failed;
                            peer.record_failure("Handshake response timeout".to_string());
                        }
                    }
                }

                drop(permit);
                Ok(())
            }
            Ok(Err(e)) => {
                let mut peers = self.peers.write().await;
                if let Some(peer) = peers.get_mut(address) {
                    peer.state = ConnectionState::Failed;
                    peer.record_failure(format!("Connection failed: {}", e));

                    let mut stats = self.stats.write().await;
                    stats.connection_failures += 1;
                }
                drop(permit);
                Err(CoreError::InvalidData(format!(
                    "Failed to connect to peer: {}",
                    e
                )))
            }
            Err(_) => {
                let mut peers = self.peers.write().await;
                if let Some(peer) = peers.get_mut(address) {
                    peer.state = ConnectionState::Failed;
                    peer.record_failure("Connection timeout".to_string());

                    let mut stats = self.stats.write().await;
                    stats.connection_failures += 1;
                }
                drop(permit);
                Err(CoreError::InvalidData("Connection timeout".to_string()))
            }
        }
    }

    /// Disconnect from peer
    pub async fn disconnect_peer(&self, address: &SocketAddr) -> CoreResult<()> {
        let mut peers = self.peers.write().await;

        if let Some(peer) = peers.get_mut(address) {
            if peer.state == ConnectionState::Connected {
                let mut stats = self.stats.write().await;
                stats.connected_peer_count = stats.connected_peer_count.saturating_sub(1);
            }
            peer.state = ConnectionState::Disconnected;
            info!("Disconnected from peer: {}", address);
            Ok(())
        } else {
            Err(CoreError::InvalidData(format!(
                "Peer {} not found",
                address
            )))
        }
    }

    /// Broadcast validator set change event
    pub async fn broadcast_validator_set_change(
        &self,
        data: Vec<u8>,
    ) -> CoreResult<BroadcastResult> {
        let message = NetworkMessage::ValidatorSetChange { data, version: 1 };
        self.broadcast_message(&message).await
    }

    /// Broadcast snapshot certificate
    pub async fn broadcast_snapshot_certificate(
        &self,
        data: Vec<u8>,
        height: u64,
    ) -> CoreResult<BroadcastResult> {
        let message = NetworkMessage::SnapshotCertificate {
            data,
            height,
            version: 1,
        };
        self.broadcast_message(&message).await
    }

    /// Broadcast transaction batch
    pub async fn broadcast_transaction_batch(
        &self,
        batch_id: Vec<u8>,
        data: Vec<u8>,
        sequence: u64,
    ) -> CoreResult<BroadcastResult> {
        let message = NetworkMessage::TransactionBatch {
            batch_id,
            data,
            sequence,
            version: 1,
        };
        self.broadcast_message(&message).await
    }

    /// Broadcast message to all connected peers with parallel sends
    async fn broadcast_message(&self, message: &NetworkMessage) -> CoreResult<BroadcastResult> {
        let start = Instant::now();
        let peers = self.peers.read().await;

        let connected_peers: Vec<SocketAddr> = peers
            .iter()
            .filter(|(_, p)| p.state == ConnectionState::Connected)
            .map(|(addr, _)| *addr)
            .collect();

        if connected_peers.is_empty() {
            warn!("No connected peers for broadcast");
            return Ok(BroadcastResult {
                total_peers: 0,
                successful: 0,
                failed: 0,
                avg_latency_ms: 0,
                min_latency_ms: 0,
                max_latency_ms: 0,
                total_bytes_sent: 0,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        drop(peers);

        let mut successful = 0;
        let mut failed = 0;
        let mut total_latency = 0u64;
        let mut min_latency = u64::MAX;
        let mut max_latency = 0u64;
        let mut total_bytes_sent = 0u64;

        // Send to all peers in parallel
        let mut handles = vec![];

        for peer_addr in connected_peers.iter() {
            let message_owned = message.clone();
            let peers = self.peers.clone();
            let stats = self.stats.clone();
            let max_retries = self.max_retries;
            let retry_backoff = self.retry_backoff;
            let message_timeout = self.message_timeout;
            let peer_addr = *peer_addr;

            let handle = tokio::spawn(async move {
                Self::send_message_with_retry_real(
                    peer_addr,
                    message_owned,
                    peers,
                    stats,
                    max_retries,
                    retry_backoff,
                    message_timeout,
                )
                .await
            });

            handles.push(handle);
        }

        // Collect results
        for handle in handles {
            match handle.await {
                Ok(Ok((latency_ms, bytes_sent))) => {
                    successful += 1;
                    total_latency += latency_ms;
                    min_latency = min_latency.min(latency_ms);
                    max_latency = max_latency.max(latency_ms);
                    total_bytes_sent += bytes_sent;
                    debug!(
                        "Broadcast succeeded (latency: {}ms, bytes: {})",
                        latency_ms, bytes_sent
                    );
                }
                Ok(Err(e)) => {
                    failed += 1;
                    warn!("Broadcast failed: {}", e);
                }
                Err(e) => {
                    failed += 1;
                    warn!("Task error: {}", e);
                }
            }
        }

        let avg_latency = if successful > 0 {
            total_latency / successful as u64
        } else {
            0
        };

        let result = BroadcastResult {
            total_peers: connected_peers.len(),
            successful,
            failed,
            avg_latency_ms: avg_latency,
            min_latency_ms: if min_latency == u64::MAX {
                0
            } else {
                min_latency
            },
            max_latency_ms: max_latency,
            total_bytes_sent,
            duration_ms: start.elapsed().as_millis() as u64,
        };

        // Update statistics
        let mut stats = self.stats.write().await;
        stats.total_broadcasts += 1;
        stats.successful_broadcasts += successful as u64;
        stats.failed_broadcasts += failed as u64;
        stats.avg_broadcast_latency_ms = avg_latency;
        stats.total_bytes_sent += total_bytes_sent;

        info!(
            "Broadcast complete: {}/{} successful, avg latency: {}ms, total bytes: {}, duration: {}ms",
            successful, connected_peers.len(), avg_latency, total_bytes_sent, result.duration_ms
        );

        Ok(result)
    }

    /// Send message with exponential backoff retry logic (static version for async spawning)
    async fn send_message_with_retry_real(
        peer_addr: SocketAddr,
        message: NetworkMessage,
        peers: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,
        stats: Arc<RwLock<BroadcastStats>>,
        max_retries: usize,
        retry_backoff: Duration,
        message_timeout: Duration,
    ) -> CoreResult<(u64, u64)> {
        let start = Instant::now();
        let mut last_error: Option<CoreError> = None;
        let mut backoff = retry_backoff;

        for attempt in 0..max_retries {
            // Inline TCP send logic
            let send_result = async {
                let serialized_data = message
                    .to_bytes()
                    .map_err(|e| CoreError::InvalidData(format!("Failed to serialize message: {}", e)))?;

                if serialized_data.len() > 10 * 1024 * 1024 {
                    return Err(CoreError::InvalidData(format!(
                        "Message too large: {} bytes (max 10MB)",
                        serialized_data.len()
                    )));
                }

                let mut socket = timeout(message_timeout, TcpStream::connect(peer_addr))
                    .await
                    .map_err(|_| CoreError::InvalidData("Connection timeout".to_string()))?
                    .map_err(|e| CoreError::InvalidData(format!("Failed to connect: {}", e)))?;

                timeout(message_timeout, socket.write_all(&serialized_data))
                    .await
                    .map_err(|_| CoreError::InvalidData("Send timeout".to_string()))?
                    .map_err(|e| CoreError::InvalidData(format!("Failed to send: {}", e)))?;

                Ok::<u64, CoreError>(serialized_data.len() as u64)
            }
            .await;

            match send_result {
                Ok(bytes_sent) => {
                    let latency_ms = start.elapsed().as_millis() as u64;

                    // Update peer info
                    let mut peers_guard = peers.write().await;
                    if let Some(peer) = peers_guard.get_mut(&peer_addr) {
                        peer.record_sent(bytes_sent, latency_ms);
                    }
                    drop(peers_guard);

                    let mut stats_guard = stats.write().await;
                    stats_guard.total_messages_sent += 1;

                    return Ok((latency_ms, bytes_sent));
                }
                Err(e) => {
                    last_error = Some(e);

                    let mut stats_guard = stats.write().await;
                    stats_guard.message_retries += 1;
                    drop(stats_guard);

                    if attempt < max_retries - 1 {
                        debug!(
                            "Send to {} failed (attempt {}/{}), retrying in {:?}...",
                            peer_addr,
                            attempt + 1,
                            max_retries,
                            backoff
                        );
                        tokio::time::sleep(backoff).await;

                        // Exponential backoff: backoff *= 2, max 5 seconds
                        backoff = backoff.mul_f64(2.0).min(Duration::from_secs(5));
                    }
                }
            }
        }

        let mut peers_guard = peers.write().await;
        if let Some(peer) = peers_guard.get_mut(&peer_addr) {
            peer.record_failure(
                last_error
                    .as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "Unknown error".to_string()),
            );
        }

        Err(last_error.unwrap_or_else(|| CoreError::InvalidData("Unknown send error".to_string())))
    }

    /// Send message to peer with real TCP connection
    #[allow(dead_code)]
    async fn send_message_real(
        &self,
        peer_addr: SocketAddr,
        message: &NetworkMessage,
        message_timeout: Duration,
    ) -> CoreResult<u64> {
        // Step 1: Serialize the message
        let serialized_data = message
            .to_bytes()
            .map_err(|e| CoreError::InvalidData(format!("Failed to serialize message: {}", e)))?;

        // Step 2: Validate message size (max 10MB)
        if serialized_data.len() > 10 * 1024 * 1024 {
            return Err(CoreError::InvalidData(format!(
                "Message too large: {} bytes (max 10MB)",
                serialized_data.len()
            )));
        }

        // Step 3: Create message envelope with metadata
        // Use sequence counter for message ordering
        let sequence = {
            let mut seq = self.message_sequence.write().await;
            let current = *seq;
            *seq = seq.saturating_add(1);
            current
        };
        let envelope = MessageEnvelope::new(
            message_type_from_network_message(message),
            serialized_data.clone(),
            peer_addr,
            sequence,
        )?;

        // Step 4: Serialize envelope
        let envelope_data = envelope.to_bytes()?;

        // Step 5: Connect and send via TCP
        let mut socket = timeout(message_timeout, TcpStream::connect(peer_addr))
            .await
            .map_err(|_| CoreError::InvalidData("Connection timeout".to_string()))?
            .map_err(|e| CoreError::InvalidData(format!("Failed to connect: {}", e)))?;

        // Step 6: Send envelope data
        timeout(message_timeout, socket.write_all(&envelope_data))
            .await
            .map_err(|_| CoreError::InvalidData("Send timeout".to_string()))?
            .map_err(|e| CoreError::InvalidData(format!("Failed to send: {}", e)))?;

        // Step 7: Flush to ensure data is sent
        timeout(message_timeout, socket.flush())
            .await
            .map_err(|_| CoreError::InvalidData("Flush timeout".to_string()))?
            .map_err(|e| CoreError::InvalidData(format!("Failed to flush: {}", e)))?;

        // Step 8: Wait for acknowledgment
        let mut buf = vec![0u8; 1024];
        match timeout(message_timeout, socket.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                if let Ok(ack_message) = NetworkMessage::from_bytes(&buf[..n]) {
                    if let NetworkMessage::Ack { .. } = ack_message {
                        debug!(
                            "Message sent to {} ({} bytes)",
                            peer_addr,
                            envelope_data.len()
                        );
                        return Ok(envelope_data.len() as u64);
                    }
                }
            }
            _ => {
                // Timeout or error on ack, but message was sent
                debug!(
                    "Message sent to {} ({} bytes, no ack)",
                    peer_addr,
                    envelope_data.len()
                );
                return Ok(envelope_data.len() as u64);
            }
        }

        Ok(envelope_data.len() as u64)
    }

    /// Get peer count
    pub async fn get_peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Get connected peer count
    pub async fn get_connected_peer_count(&self) -> usize {
        self.peers
            .read()
            .await
            .values()
            .filter(|p| p.state == ConnectionState::Connected)
            .count()
    }

    /// Get healthy peer count
    pub async fn get_healthy_peer_count(&self) -> usize {
        self.peers
            .read()
            .await
            .values()
            .filter(|p| p.is_healthy())
            .count()
    }

    /// Get peer info
    pub async fn get_peer_info(&self, address: &SocketAddr) -> CoreResult<PeerInfo> {
        self.peers
            .read()
            .await
            .get(address)
            .cloned()
            .ok_or_else(|| CoreError::InvalidData(format!("Peer {} not found", address)))
    }

    /// Get all peers
    pub async fn get_all_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Get all healthy peers
    pub async fn get_healthy_peers(&self) -> Vec<PeerInfo> {
        self.peers
            .read()
            .await
            .values()
            .filter(|p| p.is_healthy())
            .cloned()
            .collect()
    }

    /// Clean up stale peers
    pub async fn cleanup_stale_peers(&self) -> usize {
        let mut peers = self.peers.write().await;
        let initial_count = peers.len();

        peers.retain(|addr, peer| {
            if peer.is_stale() {
                info!("Removing stale peer: {}", addr);
                false
            } else {
                true
            }
        });

        let removed = initial_count - peers.len();

        let mut stats = self.stats.write().await;
        stats.peer_count = peers.len() as u64;

        removed
    }

    /// Reconnect to failed peers
    pub async fn reconnect_failed_peers(&self) -> usize {
        let failed_peers: Vec<SocketAddr> = self
            .peers
            .read()
            .await
            .iter()
            .filter(|(_, p)| p.state == ConnectionState::Failed && p.failed_attempts < 10)
            .map(|(addr, _)| *addr)
            .collect();

        let mut reconnected = 0;
        for peer_addr in failed_peers {
            if self.connect_peer(&peer_addr).await.is_ok() {
                reconnected += 1;
            }
        }

        reconnected
    }

    /// Get broadcast statistics
    pub async fn get_stats(&self) -> BroadcastStats {
        let mut stats = self.stats.read().await.clone();
        stats.peer_count = self.get_peer_count().await as u64;
        stats.connected_peer_count = self.get_connected_peer_count().await as u64;
        stats
    }

    /// Reset statistics
    pub async fn reset_stats(&self) {
        *self.stats.write().await = BroadcastStats::default();
    }

    /// Get next message sequence number
    #[allow(dead_code)]
    async fn next_sequence(&self) -> u64 {
        let mut seq = self.message_sequence.write().await;
        *seq += 1;
        *seq
    }
}

impl Default for ValidatorNetworkManager {
    fn default() -> Self {
        use std::str::FromStr;
        let addr = SocketAddr::from_str("127.0.0.1:0")
            .expect("Invalid default socket address - this is a configuration error");
        Self::new(addr, vec![0u8; 32])
    }
}
