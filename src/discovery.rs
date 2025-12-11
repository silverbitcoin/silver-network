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
        self.pending_queries
            .insert(query_id, DiscoveryQuery::Bootstrap);
        self.last_bootstrap = Some(Instant::now());
        info!("Started bootstrap query: {:?}", query_id);
    }

    /// Start a find peer query
    pub fn start_find_peer(&mut self, query_id: QueryId, peer_id: PeerId) {
        self.pending_queries
            .insert(query_id, DiscoveryQuery::FindPeer(peer_id));
        debug!("Started find peer query for {}: {:?}", peer_id, query_id);
    }

    /// Start a get providers query
    pub fn start_get_providers(&mut self, query_id: QueryId, key: Vec<u8>) {
        self.pending_queries
            .insert(query_id, DiscoveryQuery::GetProviders { key });
        debug!("Started get providers query: {:?}", query_id);
    }

    /// Handle a query result
    pub fn handle_query_result(
        &mut self,
        query_id: QueryId,
        result: &QueryResult,
    ) -> Result<DiscoveryResult> {
        let query = self
            .pending_queries
            .remove(&query_id)
            .ok_or_else(|| NetworkError::Discovery(format!("Unknown query ID: {:?}", query_id)))?;

        match (query, result) {
            (DiscoveryQuery::Bootstrap, QueryResult::Bootstrap(Ok(result))) => {
                info!(
                    "Bootstrap completed successfully, found {} peers",
                    result.num_remaining
                );
                Ok(DiscoveryResult::Bootstrap {
                    num_peers: result.num_remaining as usize,
                })
            }
            (DiscoveryQuery::Bootstrap, QueryResult::Bootstrap(Err(e))) => {
                warn!("Bootstrap failed: {:?}", e);
                Err(NetworkError::Discovery(format!(
                    "Bootstrap failed: {:?}",
                    e
                )))
            }
            (DiscoveryQuery::FindPeer(target), QueryResult::GetClosestPeers(Ok(result))) => {
                info!(
                    "Find peer completed for {}, found {} peers",
                    target,
                    result.peers.len()
                );
                Ok(DiscoveryResult::FindPeer {
                    target,
                    peers: result.peers.clone(),
                })
            }
            (DiscoveryQuery::FindPeer(target), QueryResult::GetClosestPeers(Err(e))) => {
                warn!("Find peer failed for {}: {:?}", target, e);
                Err(NetworkError::Discovery(format!(
                    "Find peer failed: {:?}",
                    e
                )))
            }
            (DiscoveryQuery::GetProviders { key }, QueryResult::GetProviders(Ok(result))) => {
                // PRODUCTION IMPLEMENTATION: Extract providers from libp2p 0.45 GetProvidersOk
                // GetProvidersOk is an enum with two variants:
                // 1. FoundProviders: Contains a HashSet of provider peer IDs
                // 2. FinishedWithNoAdditionalRecord: Contains closest peers when no providers found
                
                let mut providers: Vec<PeerId> = Vec::new();
                
                match result {
                    libp2p::kad::GetProvidersOk::FoundProviders { providers: provider_set, .. } => {
                        // Extract all provider peer IDs from the HashSet
                        // These are peers that are actively providing the content
                        for peer_id in provider_set {
                            providers.push(*peer_id);
                        }
                        
                        info!(
                            "Get providers found {} providers for key {:?}",
                            providers.len(),
                            String::from_utf8_lossy(&key)
                        );
                    }
                    libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers } => {
                        // When no providers are found, return the closest peers in the DHT
                        // These peers are closest to the key in the DHT keyspace
                        // and may have information about providers or can cache the content
                        providers = closest_peers.clone();
                        
                        info!(
                            "Get providers found no providers, returning {} closest peers for key {:?}",
                            providers.len(),
                            String::from_utf8_lossy(&key)
                        );
                    }
                }
                
                // Sort providers by peer ID for deterministic ordering
                // This ensures consistent ordering across different nodes
                providers.sort_by(|a, b| {
                    a.to_bytes().cmp(&b.to_bytes())
                });
                
                // Remove duplicates while maintaining sorted order
                // This ensures we don't return the same provider multiple times
                providers.dedup();
                
                Ok(DiscoveryResult::GetProviders { key, providers })
            }
            (DiscoveryQuery::GetProviders { key: _ }, QueryResult::GetProviders(Err(e))) => {
                warn!("Get providers failed: {:?}", e);
                Err(NetworkError::Discovery(format!(
                    "Get providers failed: {:?}",
                    e
                )))
            }
            _ => {
                warn!("Unexpected query result for query {:?}", query_id);
                Err(NetworkError::Discovery(
                    "Unexpected query result".to_string(),
                ))
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
