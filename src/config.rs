use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Local listen address (e.g., "/ip4/0.0.0.0/tcp/9000")
    pub listen_address: Multiaddr,

    /// External address advertised to peers
    pub external_address: Option<Multiaddr>,

    /// Bootstrap peers for initial connection
    pub bootstrap_peers: Vec<Multiaddr>,

    /// Maximum number of connected peers
    pub max_peers: usize,

    /// Minimum number of connected peers
    pub min_peers: usize,

    /// Connection timeout
    pub connection_timeout: Duration,

    /// Enable QUIC transport
    pub enable_quic: bool,

    /// Enable TCP transport
    pub enable_tcp: bool,

    /// Enable mDNS for local peer discovery
    pub enable_mdns: bool,

    /// DHT mode (server or client)
    pub dht_server_mode: bool,

    /// Maximum message size in bytes
    pub max_message_size: usize,

    /// Rate limit: maximum messages per second per peer
    pub rate_limit_per_peer: u32,

    /// Gossipsub heartbeat interval
    pub gossip_heartbeat_interval: Duration,

    /// Gossipsub message history length
    pub gossip_history_length: usize,

    /// Gossipsub message history time
    pub gossip_history_time: Duration,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let listen_address = "/ip4/0.0.0.0/tcp/9000".parse()
            .expect("Invalid default listen address - this is a configuration error");
        
        Self {
            listen_address,
            external_address: None,
            bootstrap_peers: Vec::new(),
            max_peers: 50,
            min_peers: 8,
            connection_timeout: Duration::from_secs(30),
            enable_quic: true,
            enable_tcp: true,
            enable_mdns: true,
            dht_server_mode: false,
            max_message_size: 10 * 1024 * 1024, // 10 MB
            rate_limit_per_peer: 10_000,        // 10,000 messages per second
            gossip_heartbeat_interval: Duration::from_millis(700),
            gossip_history_length: 5,
            gossip_history_time: Duration::from_secs(120),
        }
    }
}

impl NetworkConfig {
    /// Create a new network configuration
    pub fn new(listen_address: Multiaddr) -> Self {
        Self {
            listen_address,
            ..Default::default()
        }
    }

    /// Set external address
    pub fn with_external_address(mut self, address: Multiaddr) -> Self {
        self.external_address = Some(address);
        self
    }

    /// Add bootstrap peer
    pub fn with_bootstrap_peer(mut self, peer: Multiaddr) -> Self {
        self.bootstrap_peers.push(peer);
        self
    }

    /// Set maximum peers
    pub fn with_max_peers(mut self, max_peers: usize) -> Self {
        self.max_peers = max_peers;
        self
    }

    /// Enable DHT server mode
    pub fn with_dht_server_mode(mut self, enabled: bool) -> Self {
        self.dht_server_mode = enabled;
        self
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_peers < self.min_peers {
            return Err("max_peers must be >= min_peers".to_string());
        }

        if self.max_peers == 0 {
            return Err("max_peers must be > 0".to_string());
        }

        if !self.enable_tcp && !self.enable_quic {
            return Err("At least one transport (TCP or QUIC) must be enabled".to_string());
        }

        if self.max_message_size == 0 {
            return Err("max_message_size must be > 0".to_string());
        }

        if self.rate_limit_per_peer == 0 {
            return Err("rate_limit_per_peer must be > 0".to_string());
        }

        Ok(())
    }
}
