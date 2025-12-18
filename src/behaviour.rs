use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify,
    kad::{self, store::MemoryStore},
    mdns, ping,
    swarm::NetworkBehaviour,
    PeerId,
};
use std::time::Duration;

/// Combined network behaviour for SilverBitcoin
#[derive(NetworkBehaviour)]
pub struct SilverBehaviour {
    /// Kademlia DHT for peer discovery
    pub kad: kad::Behaviour<MemoryStore>,

    /// Gossipsub for message propagation
    pub gossipsub: gossipsub::Behaviour,

    /// Identify protocol for peer information exchange
    pub identify: identify::Behaviour,

    /// Ping protocol for connection keep-alive
    pub ping: ping::Behaviour,

    /// mDNS for local peer discovery
    pub mdns: mdns::tokio::Behaviour,
}

impl SilverBehaviour {
    /// Create a new SilverBehaviour
    pub fn new(
        local_peer_id: PeerId,
        local_keypair: libp2p::identity::Keypair,
        enable_mdns: bool,
        dht_server_mode: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Configure Kademlia DHT
        let mut kad_config = kad::Config::default();
        kad_config.set_query_timeout(Duration::from_secs(60));

        let store = MemoryStore::new(local_peer_id);
        let mut kad = kad::Behaviour::with_config(local_peer_id, store, kad_config);

        // Set DHT mode
        if dht_server_mode {
            kad.set_mode(Some(kad::Mode::Server));
        } else {
            kad.set_mode(Some(kad::Mode::Client));
        }

        // Configure Gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_millis(700))
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(|message| {
                // Use message content hash as ID
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                message.data.hash(&mut hasher);
                gossipsub::MessageId::from(hasher.finish().to_string())
            })
            .max_transmit_size(10 * 1024 * 1024) // 10 MB max message size
            .build()
            .map_err(|e| format!("Failed to build gossipsub config: {}", e))?;

        let gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| format!("Failed to create gossipsub behaviour: {}", e))?;

        // Configure Identify protocol
        let identify_config =
            identify::Config::new("/silverbitcoin/1.0.0".to_string(), local_keypair.public())
                .with_agent_version(format!("silverbitcoin/{}", env!("CARGO_PKG_VERSION")));

        let identify = identify::Behaviour::new(identify_config);

        // Configure Ping protocol
        let ping_config = ping::Config::new()
            .with_interval(Duration::from_secs(30))
            .with_timeout(Duration::from_secs(10));

        let ping = ping::Behaviour::new(ping_config);

        // Configure mDNS
        let mdns = if enable_mdns {
            mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?
        } else {
            // Create a disabled mDNS behaviour
            mdns::tokio::Behaviour::new(
                mdns::Config {
                    ttl: Duration::from_secs(60),
                    query_interval: Duration::from_secs(3600), // Very long interval = effectively disabled
                    enable_ipv6: false,
                },
                local_peer_id,
            )?
        };

        Ok(Self {
            kad,
            gossipsub,
            identify,
            ping,
            mdns,
        })
    }

    /// Subscribe to a gossipsub topic
    pub fn subscribe(&mut self, topic: &str) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = IdentTopic::new(topic);
        self.gossipsub.subscribe(&topic)
    }

    /// Unsubscribe from a gossipsub topic
    pub fn unsubscribe(&mut self, topic: &str) -> Result<bool, gossipsub::PublishError> {
        let topic = IdentTopic::new(topic);
        self.gossipsub.unsubscribe(&topic)
    }

    /// Publish a message to a gossipsub topic
    pub fn publish(
        &mut self,
        topic: &str,
        data: Vec<u8>,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        let topic = IdentTopic::new(topic);
        self.gossipsub.publish(topic, data)
    }

    /// Add a peer to the DHT routing table
    pub fn add_address(&mut self, peer_id: &PeerId, address: multiaddr::Multiaddr) {
        self.kad.add_address(peer_id, address);
    }

    /// Bootstrap the DHT
    pub fn bootstrap(&mut self) -> Result<kad::QueryId, kad::NoKnownPeers> {
        self.kad.bootstrap()
    }

    /// Get the number of connected peers
    pub fn connected_peers(&self) -> usize {
        self.gossipsub.all_peers().count()
    }
}
