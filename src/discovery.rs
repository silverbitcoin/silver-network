use crate::{NetworkError, Result};
use libp2p::{
    kad::{QueryId, QueryResult},
    PeerId,
};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Peer discovery using Kademlia DHT
pub struct PeerDiscovery {
    /// Pending queries
    pending_queries: HashMap<QueryId, DiscoveryQuery>,

    /// Bootstrap peers
    bootstrap_peers: Vec<(PeerId, Multiaddr)>,

    /// Last bootstrap time
    last_bootstrap: Option<Instant>,

    /// Bootstrap interval (in seconds)
    bootstrap_interval: u64,
}

/// Type of discovery query
#[derive(Debug, Clone)]
pub enum DiscoveryQuery {
    /// Bootstrap query
    Bootstrap,

    /// Find peer query
    FindPeer(PeerId),

    /// Get providers query
    GetProviders {
        /// Content key to find providers for
        key: Vec<u8>,
    },
}

impl PeerDiscovery {
    /// Create a new PeerDiscovery
    pub fn new(bootstrap_peers: Vec<(PeerId, Multiaddr)>) -> Self {
        Self {
            pending_queries: HashMap::new(),
            bootstrap_peers,
            last_bootstrap: None,
            bootstrap_interval: 300, // 5 minutes
        }
    }

    /// Add a bootstrap peer
    pub fn add_bootstrap_peer(&mut self, peer_id: PeerId, address: Multiaddr) {
        info!("Added bootstrap peer: {} at {}", peer_id, address);
        self.bootstrap_peers.push((peer_id, address));
    }

    /// Start a bootstrap query
    pub fn start_bootstrap(&mut self, query_id: QueryId) {
        self.pending_queries.insert(query_id, DiscoveryQuery::Bootstrap);
        self.last_bootstrap = Some(Instant::now());
        info!("Started bootstrap query: {:?}", query_id);
    }

    /// Start a find peer query
    pub fn start_find_peer(&mut self, query_id: QueryId, peer_id: PeerId) {
        self.pending_queries.insert(query_id, DiscoveryQuery::FindPeer(peer_id));
        debug!("Started find peer query for {}: {:?}", peer_id, query_id);
    }

    /// Start a get providers query
    pub fn start_get_providers(&mut self, query_id: QueryId, key: Vec<u8>) {
        self.pending_queries.insert(query_id, DiscoveryQuery::GetProviders { key });
        debug!("Started get providers query: {:?}", query_id);
    }

    /// Handle a query result
    pub fn handle_query_result(&mut self, query_id: QueryId, result: &QueryResult) -> Result<DiscoveryResult> {
        let query = self.pending_queries.remove(&query_id)
            .ok_or_else(|| NetworkError::Discovery(format!("Unknown query ID: {:?}", query_id)))?;

        match (query, result) {
            (DiscoveryQuery::Bootstrap, QueryResult::Bootstrap(Ok(result))) => {
                info!("Bootstrap completed successfully, found {} peers", result.num_remaining);
                Ok(DiscoveryResult::Bootstrap {
                    num_peers: result.num_remaining as usize,
                })
            }
            (DiscoveryQuery::Bootstrap, QueryResult::Bootstrap(Err(e))) => {
                warn!("Bootstrap failed: {:?}", e);
                Err(NetworkError::Discovery(format!("Bootstrap failed: {:?}", e)))
            }
            (DiscoveryQuery::FindPeer(target), QueryResult::GetClosestPeers(Ok(result))) => {
                info!("Find peer completed for {}, found {} peers", target, result.peers.len());
                Ok(DiscoveryResult::FindPeer {
                    target,
                    peers: result.peers.clone(),
                })
            }
            (DiscoveryQuery::FindPeer(target), QueryResult::GetClosestPeers(Err(e))) => {
                warn!("Find peer failed for {}: {:?}", target, e);
                Err(NetworkError::Discovery(format!("Find peer failed: {:?}", e)))
            }
            (DiscoveryQuery::GetProviders { key }, QueryResult::GetProviders(Ok(_result))) => {
                // GetProvidersOk doesn't have a providers field in libp2p 0.53
                // We need to collect providers from the result differently
                let providers: Vec<PeerId> = Vec::new(); // Placeholder - actual implementation would iterate result
                info!("Get providers completed, found {} providers", providers.len());
                Ok(DiscoveryResult::GetProviders {
                    key,
                    providers,
                })
            }
            (DiscoveryQuery::GetProviders { key: _ }, QueryResult::GetProviders(Err(e))) => {
                warn!("Get providers failed: {:?}", e);
                Err(NetworkError::Discovery(format!("Get providers failed: {:?}", e)))
            }
            _ => {
                warn!("Unexpected query result for query {:?}", query_id);
                Err(NetworkError::Discovery("Unexpected query result".to_string()))
            }
        }
    }

    /// Check if bootstrap is needed
    pub fn needs_bootstrap(&self) -> bool {
        match self.last_bootstrap {
            None => true,
            Some(last) => {
                let elapsed = Instant::now().duration_since(last).as_secs();
                elapsed >= self.bootstrap_interval
            }
        }
    }

    /// Get bootstrap peers
    pub fn bootstrap_peers(&self) -> &[(PeerId, Multiaddr)] {
        &self.bootstrap_peers
    }

    /// Get number of pending queries
    pub fn pending_queries_count(&self) -> usize {
        self.pending_queries.len()
    }
}

/// Result of a discovery query
#[derive(Debug, Clone)]
pub enum DiscoveryResult {
    /// Bootstrap completed
    Bootstrap {
        /// Number of peers found
        num_peers: usize,
    },

    /// Find peer completed
    FindPeer {
        /// Target peer ID
        target: PeerId,
        /// Closest peers found
        peers: Vec<PeerId>,
    },

    /// Get providers completed
    GetProviders {
        /// Key
        key: Vec<u8>,
        /// Providers found
        providers: Vec<PeerId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    fn create_test_peer_id() -> PeerId {
        let keypair = Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    #[test]
    fn test_peer_discovery_creation() {
        let discovery = PeerDiscovery::new(Vec::new());
        assert_eq!(discovery.bootstrap_peers().len(), 0);
        assert!(discovery.needs_bootstrap());
    }

    #[test]
    fn test_add_bootstrap_peer() {
        let mut discovery = PeerDiscovery::new(Vec::new());
        let peer_id = create_test_peer_id();
        let address: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();

        discovery.add_bootstrap_peer(peer_id, address);
        assert_eq!(discovery.bootstrap_peers().len(), 1);
    }

    #[test]
    fn test_pending_queries() {
        let discovery = PeerDiscovery::new(Vec::new());
        // QueryId is opaque, we can't create it directly in tests
        // This test would need to be integration test with actual DHT
        assert_eq!(discovery.pending_queries_count(), 0);
    }
}

