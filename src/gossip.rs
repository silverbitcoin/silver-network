use crate::{NetworkError, NetworkMessage, Result};
use libp2p::gossipsub::{IdentTopic, MessageId, TopicHash};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Gossip protocol for message propagation
pub struct GossipProtocol {
    /// Active topics
    topics: HashMap<String, TopicInfo>,

    /// Message cache for deduplication
    message_cache: HashMap<MessageId, CachedMessage>,

    /// Maximum cache size
    max_cache_size: usize,

    /// Cache TTL
    cache_ttl: Duration,

    /// Message propagation statistics
    stats: GossipStats,
}

/// Information about a subscribed topic.
///
/// Tracks metadata and statistics for a gossip topic.
#[derive(Debug, Clone)]
pub struct TopicInfo {
    /// Topic name
    #[allow(dead_code)]
    name: String,

    /// Topic hash
    #[allow(dead_code)]
    hash: TopicHash,

    /// Number of messages sent on this topic
    messages_sent: u64,

    /// Number of messages received on this topic
    messages_received: u64,

    /// Last activity timestamp
    last_activity: Instant,
}

/// Cached message for deduplication.
///
/// Stores message metadata for duplicate detection.
#[derive(Debug, Clone)]
struct CachedMessage {
    /// Message ID
    #[allow(dead_code)]
    id: MessageId,

    /// Message data (kept for potential future use in message validation)
    #[allow(dead_code)]
    data: Vec<u8>,

    /// Timestamp when cached
    cached_at: Instant,

    /// Number of times seen
    #[allow(dead_code)]
    seen_count: u32,
}

/// Gossip protocol statistics
#[derive(Debug, Clone, Default)]
pub struct GossipStats {
    /// Total messages sent
    pub total_sent: u64,

    /// Total messages received
    pub total_received: u64,

    /// Total messages deduplicated
    pub total_deduplicated: u64,

    /// Total bytes sent
    pub total_bytes_sent: u64,

    /// Total bytes received
    pub total_bytes_received: u64,
}

impl GossipProtocol {
    /// Create a new GossipProtocol
    pub fn new() -> Self {
        Self {
            topics: HashMap::new(),
            message_cache: HashMap::new(),
            max_cache_size: 10_000,
            cache_ttl: Duration::from_secs(120),
            stats: GossipStats::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(max_cache_size: usize, cache_ttl: Duration) -> Self {
        Self {
            topics: HashMap::new(),
            message_cache: HashMap::new(),
            max_cache_size,
            cache_ttl,
            stats: GossipStats::default(),
        }
    }

    /// Subscribe to a topic
    pub fn subscribe(&mut self, topic: &str) -> Result<()> {
        if self.topics.contains_key(topic) {
            debug!("Already subscribed to topic: {}", topic);
            return Ok(());
        }

        let ident_topic = IdentTopic::new(topic);
        let topic_info = TopicInfo {
            name: topic.to_string(),
            hash: ident_topic.hash(),
            messages_sent: 0,
            messages_received: 0,
            last_activity: Instant::now(),
        };

        self.topics.insert(topic.to_string(), topic_info);
        info!("Subscribed to topic: {}", topic);
        Ok(())
    }

    /// Unsubscribe from a topic
    pub fn unsubscribe(&mut self, topic: &str) -> Result<()> {
        if self.topics.remove(topic).is_some() {
            info!("Unsubscribed from topic: {}", topic);
            Ok(())
        } else {
            Err(NetworkError::InvalidMessage(format!("Not subscribed to topic: {}", topic)))
        }
    }

    /// Prepare a message for publishing
    pub fn prepare_message(&mut self, topic: &str, message: &NetworkMessage) -> Result<Vec<u8>> {
        // Check if subscribed to topic
        let topic_info = self.topics.get_mut(topic)
            .ok_or_else(|| NetworkError::InvalidMessage(format!("Not subscribed to topic: {}", topic)))?;

        // Serialize message
        let data = message.to_bytes()
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;

        // Check message size
        if data.len() > 10 * 1024 * 1024 {
            return Err(NetworkError::MessageTooLarge(data.len(), 10 * 1024 * 1024));
        }

        // Update statistics
        topic_info.messages_sent += 1;
        topic_info.last_activity = Instant::now();
        self.stats.total_sent += 1;
        self.stats.total_bytes_sent += data.len() as u64;

        debug!("Prepared message for topic {}: {} bytes", topic, data.len());
        Ok(data)
    }

    /// Handle received message
    pub fn handle_received_message(
        &mut self,
        message_id: MessageId,
        topic: &str,
        data: Vec<u8>,
    ) -> Result<Option<NetworkMessage>> {
        // Check for duplicate
        if let Some(cached) = self.message_cache.get_mut(&message_id) {
            cached.seen_count += 1;
            self.stats.total_deduplicated += 1;
            debug!("Deduplicated message {} (seen {} times)", message_id, cached.seen_count);
            return Ok(None);
        }

        // Update topic statistics
        if let Some(topic_info) = self.topics.get_mut(topic) {
            topic_info.messages_received += 1;
            topic_info.last_activity = Instant::now();
        }

        // Update global statistics
        self.stats.total_received += 1;
        self.stats.total_bytes_received += data.len() as u64;

        // Cache message
        self.cache_message(message_id.clone(), data.clone());

        // Deserialize message
        let message = NetworkMessage::from_bytes(&data)
            .map_err(|e| NetworkError::Deserialization(format!("Failed to deserialize message: {}", e)))?;

        debug!("Received message {} on topic {}: {:?}", message_id, topic, message.message_type());
        Ok(Some(message))
    }

    /// Cache a message for deduplication
    fn cache_message(&mut self, message_id: MessageId, data: Vec<u8>) {
        // Evict old messages if cache is full
        if self.message_cache.len() >= self.max_cache_size {
            self.evict_old_messages();
        }

        let cached = CachedMessage {
            id: message_id.clone(),
            data,
            cached_at: Instant::now(),
            seen_count: 1,
        };

        self.message_cache.insert(message_id, cached);
    }

    /// Evict old messages from cache
    fn evict_old_messages(&mut self) {
        let now = Instant::now();
        let ttl = self.cache_ttl;

        self.message_cache.retain(|_, cached| {
            now.duration_since(cached.cached_at) < ttl
        });

        // If still too large, remove oldest 10%
        if self.message_cache.len() >= self.max_cache_size {
            let to_remove = self.max_cache_size / 10;
            let mut entries: Vec<_> = self.message_cache.iter()
                .map(|(id, cached)| (id.clone(), cached.cached_at))
                .collect();
            entries.sort_by_key(|(_, time)| *time);

            for (id, _) in entries.iter().take(to_remove) {
                self.message_cache.remove(id);
            }

            debug!("Evicted {} old messages from cache", to_remove);
        }
    }

    /// Get topic information
    pub fn get_topic_info(&self, topic: &str) -> Option<&TopicInfo> {
        self.topics.get(topic)
    }

    /// Get all subscribed topics
    pub fn subscribed_topics(&self) -> Vec<String> {
        self.topics.keys().cloned().collect()
    }

    /// Get statistics
    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Clear message cache
    pub fn clear_cache(&mut self) {
        self.message_cache.clear();
        debug!("Cleared message cache");
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.message_cache.len()
    }
    
    /// Broadcast a message to all peers on a topic
    pub async fn broadcast(&self, message_type: crate::MessageType, data: Vec<u8>) -> Result<()> {
        // Determine topic based on message type
        let topic = match message_type {
            crate::MessageType::Transaction => topics::TRANSACTIONS,
            crate::MessageType::Batch => topics::BATCHES,
            crate::MessageType::Certificate => topics::CERTIFICATES,
            crate::MessageType::Snapshot => topics::SNAPSHOTS,
            _ => return Err(NetworkError::InvalidMessage(format!("Unsupported message type for broadcast: {:?}", message_type))),
        };
        
        // Check if subscribed to topic
        if !self.topics.contains_key(topic) {
            return Err(NetworkError::InvalidMessage(format!("Not subscribed to topic: {}", topic)));
        }
        
        // In a real implementation, this would publish to the libp2p gossipsub network
        // For now, we'll just log it
        debug!("Broadcasting {} bytes to topic {}", data.len(), topic);
        Ok(())
    }
}

impl Default for GossipProtocol {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard gossip topics for SilverBitcoin
pub mod topics {
    /// Transaction propagation topic
    pub const TRANSACTIONS: &str = "silverbitcoin/transactions/1.0.0";

    /// Batch propagation topic
    pub const BATCHES: &str = "silverbitcoin/batches/1.0.0";

    /// Certificate propagation topic
    pub const CERTIFICATES: &str = "silverbitcoin/certificates/1.0.0";

    /// Snapshot propagation topic
    pub const SNAPSHOTS: &str = "silverbitcoin/snapshots/1.0.0";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkMessage;

    #[test]
    fn test_gossip_protocol_creation() {
        let protocol = GossipProtocol::new();
        assert_eq!(protocol.subscribed_topics().len(), 0);
        assert_eq!(protocol.cache_size(), 0);
    }

    #[test]
    fn test_topic_subscription() {
        let mut protocol = GossipProtocol::new();
        
        assert!(protocol.subscribe("test-topic").is_ok());
        assert_eq!(protocol.subscribed_topics().len(), 1);
        
        assert!(protocol.unsubscribe("test-topic").is_ok());
        assert_eq!(protocol.subscribed_topics().len(), 0);
    }

    #[test]
    fn test_message_preparation() {
        let mut protocol = GossipProtocol::new();
        protocol.subscribe("test-topic").unwrap();

        let message = NetworkMessage::Ping { timestamp: 12345 };
        let data = protocol.prepare_message("test-topic", &message);
        
        assert!(data.is_ok());
        assert!(data.unwrap().len() > 0);
    }

    #[test]
    fn test_message_deduplication() {
        let mut protocol = GossipProtocol::new();
        protocol.subscribe("test-topic").unwrap();

        let message_id = MessageId::from("test-id");
        // Create a valid serialized NetworkMessage
        let message = NetworkMessage::Ping { timestamp: 12345 };
        let data = message.to_bytes().unwrap();

        // First message should be processed
        let result1 = protocol.handle_received_message(message_id.clone(), "test-topic", data.clone());
        assert!(result1.is_ok());
        assert!(result1.unwrap().is_some());

        // Second message with same ID should be deduplicated
        let result2 = protocol.handle_received_message(message_id, "test-topic", data);
        assert!(result2.is_ok());
        assert!(result2.unwrap().is_none());
        
        assert_eq!(protocol.stats().total_deduplicated, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let mut protocol = GossipProtocol::with_config(10, Duration::from_secs(1));
        protocol.subscribe("test-topic").unwrap();

        // Fill cache beyond capacity
        for i in 0..15 {
            let message_id = MessageId::from(format!("msg-{}", i));
            let data = vec![i as u8];
            let _ = protocol.handle_received_message(message_id, "test-topic", data);
        }

        // Cache should be limited
        assert!(protocol.cache_size() <= 10);
    }
}

