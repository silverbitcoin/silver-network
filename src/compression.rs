//! OPTIMIZATION: Message compression for network layer (Task 35.3)
//!
//! This module provides:
//! - Snappy compression for large messages
//! - Batch message aggregation
//! - Optimized gossip protocol for fast propagation

use snap::raw::{Decoder, Encoder};
use std::io;
use tracing::{debug, trace};

/// Compression threshold in bytes
///
/// Messages smaller than this are not compressed to avoid overhead.
const COMPRESSION_THRESHOLD: usize = 1024; // 1KB

/// OPTIMIZATION: Message compressor using Snappy
///
/// Snappy provides fast compression with reasonable compression ratios,
/// making it ideal for network messages where latency is critical.
pub struct MessageCompressor {
    /// Snappy encoder (reusable)
    encoder: Encoder,

    /// Snappy decoder (reusable)
    decoder: Decoder,

    /// Statistics
    stats: CompressionStats,
}

/// Compression statistics
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// Total bytes compressed
    pub bytes_compressed: u64,

    /// Total bytes after compression
    pub bytes_after_compression: u64,

    /// Total bytes decompressed
    pub bytes_decompressed: u64,

    /// Number of messages compressed
    pub messages_compressed: u64,

    /// Number of messages decompressed
    pub messages_decompressed: u64,

    /// Number of messages skipped (too small)
    pub messages_skipped: u64,
}

impl CompressionStats {
    /// Get compression ratio (0.0 to 1.0, lower is better)
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_compressed > 0 {
            self.bytes_after_compression as f64 / self.bytes_compressed as f64
        } else {
            1.0
        }
    }

    /// Get space saved in bytes
    pub fn space_saved(&self) -> u64 {
        self.bytes_compressed
            .saturating_sub(self.bytes_after_compression)
    }

    /// Get space saved percentage
    pub fn space_saved_percent(&self) -> f64 {
        if self.bytes_compressed > 0 {
            (self.space_saved() as f64 / self.bytes_compressed as f64) * 100.0
        } else {
            0.0
        }
    }
}

impl MessageCompressor {
    /// Create a new message compressor
    pub fn new() -> Self {
        Self {
            encoder: Encoder::new(),
            decoder: Decoder::new(),
            stats: CompressionStats::default(),
        }
    }

    /// OPTIMIZATION: Compress a message using Snappy
    ///
    /// Only compresses messages larger than the threshold to avoid overhead.
    ///
    /// # Arguments
    /// * `data` - Message data to compress
    ///
    /// # Returns
    /// Compressed data with a header indicating if compression was used
    pub fn compress(&mut self, data: &[u8]) -> io::Result<Vec<u8>> {
        // Skip compression for small messages
        if data.len() < COMPRESSION_THRESHOLD {
            trace!(
                "Skipping compression for small message ({} bytes)",
                data.len()
            );
            self.stats.messages_skipped += 1;

            // Return with uncompressed flag (0x00)
            let mut result = Vec::with_capacity(data.len() + 1);
            result.push(0x00); // Uncompressed flag
            result.extend_from_slice(data);
            return Ok(result);
        }

        // Compress using Snappy
        let compressed = self
            .encoder
            .compress_vec(data)
            .map_err(io::Error::other)?;

        // Check if compression actually helped
        if compressed.len() >= data.len() {
            trace!(
                "Compression didn't help ({} -> {} bytes), using uncompressed",
                data.len(),
                compressed.len()
            );
            self.stats.messages_skipped += 1;

            let mut result = Vec::with_capacity(data.len() + 1);
            result.push(0x00); // Uncompressed flag
            result.extend_from_slice(data);
            return Ok(result);
        }

        // Update statistics
        self.stats.bytes_compressed += data.len() as u64;
        self.stats.bytes_after_compression += compressed.len() as u64;
        self.stats.messages_compressed += 1;

        debug!(
            "Compressed message: {} -> {} bytes ({:.1}% reduction)",
            data.len(),
            compressed.len(),
            (1.0 - compressed.len() as f64 / data.len() as f64) * 100.0
        );

        // Return with compressed flag (0x01)
        let mut result = Vec::with_capacity(compressed.len() + 1);
        result.push(0x01); // Compressed flag
        result.extend_from_slice(&compressed);
        Ok(result)
    }

    /// OPTIMIZATION: Decompress a message
    ///
    /// Checks the header to determine if decompression is needed.
    ///
    /// # Arguments
    /// * `data` - Compressed message data (with header)
    ///
    /// # Returns
    /// Decompressed data
    pub fn decompress(&mut self, data: &[u8]) -> io::Result<Vec<u8>> {
        if data.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Empty message"));
        }

        // Check compression flag
        let is_compressed = data[0] == 0x01;
        let payload = &data[1..];

        if !is_compressed {
            // Message was not compressed
            trace!("Message is uncompressed ({} bytes)", payload.len());
            return Ok(payload.to_vec());
        }

        // Decompress using Snappy
        let decompressed = self
            .decoder
            .decompress_vec(payload)
            .map_err(io::Error::other)?;

        // Update statistics
        self.stats.bytes_decompressed += decompressed.len() as u64;
        self.stats.messages_decompressed += 1;

        debug!(
            "Decompressed message: {} -> {} bytes",
            payload.len(),
            decompressed.len()
        );

        Ok(decompressed)
    }

    /// Get compression statistics
    pub fn stats(&self) -> &CompressionStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = CompressionStats::default();
    }
}

impl Default for MessageCompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// OPTIMIZATION: Batch message aggregator
///
/// Aggregates multiple small messages into a single larger message
/// to reduce network overhead and improve throughput.
pub struct MessageBatcher {
    /// Pending messages
    pending: Vec<Vec<u8>>,

    /// Current batch size in bytes
    current_size: usize,

    /// Maximum batch size in bytes
    max_batch_size: usize,

    /// Maximum number of messages per batch
    max_messages: usize,

    /// Statistics
    stats: BatchStats,
}

/// Batch statistics
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    /// Total batches created
    pub batches_created: u64,

    /// Total messages batched
    pub messages_batched: u64,

    /// Average messages per batch
    pub avg_messages_per_batch: f64,

    /// Average batch size in bytes
    pub avg_batch_size: f64,
}

impl MessageBatcher {
    /// Create a new message batcher
    ///
    /// # Arguments
    /// * `max_batch_size` - Maximum batch size in bytes (default: 64KB)
    /// * `max_messages` - Maximum messages per batch (default: 100)
    pub fn new(max_batch_size: usize, max_messages: usize) -> Self {
        Self {
            pending: Vec::new(),
            current_size: 0,
            max_batch_size,
            max_messages,
            stats: BatchStats::default(),
        }
    }

    /// Create a new message batcher with default settings
    pub fn with_defaults() -> Self {
        Self::new(64 * 1024, 100) // 64KB, 100 messages
    }

    /// OPTIMIZATION: Add a message to the batch
    ///
    /// Returns Some(batch) if the batch is ready to be sent.
    ///
    /// # Arguments
    /// * `message` - Message to add
    ///
    /// # Returns
    /// Optional batch if ready to send
    pub fn add_message(&mut self, message: Vec<u8>) -> Option<Vec<u8>> {
        let message_size = message.len();

        // Check if adding this message would exceed limits
        if !self.pending.is_empty()
            && (self.current_size + message_size > self.max_batch_size
                || self.pending.len() >= self.max_messages)
        {
            // Flush current batch before adding new message
            let batch = self.flush();
            self.pending.push(message);
            self.current_size = message_size;
            return Some(batch);
        }

        // Add message to pending
        self.pending.push(message);
        self.current_size += message_size;

        // Check if batch is ready
        if self.current_size >= self.max_batch_size || self.pending.len() >= self.max_messages {
            Some(self.flush())
        } else {
            None
        }
    }

    /// OPTIMIZATION: Flush pending messages into a batch
    ///
    /// Creates a batch from all pending messages.
    ///
    /// # Returns
    /// Serialized batch
    pub fn flush(&mut self) -> Vec<u8> {
        if self.pending.is_empty() {
            return Vec::new();
        }

        // Serialize batch
        // Format: [num_messages: u32] [msg1_len: u32] [msg1_data] [msg2_len: u32] [msg2_data] ...
        let mut batch = Vec::with_capacity(self.current_size + self.pending.len() * 4 + 4);

        // Write number of messages
        batch.extend_from_slice(&(self.pending.len() as u32).to_le_bytes());

        // Write each message
        for message in &self.pending {
            batch.extend_from_slice(&(message.len() as u32).to_le_bytes());
            batch.extend_from_slice(message);
        }

        // Update statistics
        self.stats.batches_created += 1;
        self.stats.messages_batched += self.pending.len() as u64;
        self.stats.avg_messages_per_batch =
            self.stats.messages_batched as f64 / self.stats.batches_created as f64;
        self.stats.avg_batch_size = (self.stats.avg_batch_size
            * (self.stats.batches_created - 1) as f64
            + batch.len() as f64)
            / self.stats.batches_created as f64;

        debug!(
            "Created batch: {} messages, {} bytes",
            self.pending.len(),
            batch.len()
        );

        // Clear pending
        self.pending.clear();
        self.current_size = 0;

        batch
    }

    /// OPTIMIZATION: Unbatch a batch into individual messages
    ///
    /// # Arguments
    /// * `batch` - Serialized batch
    ///
    /// # Returns
    /// Vector of individual messages
    pub fn unbatch(batch: &[u8]) -> io::Result<Vec<Vec<u8>>> {
        if batch.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Batch too small",
            ));
        }

        let mut cursor = 0;

        // Read number of messages
        let mut num_bytes = [0u8; 4];
        num_bytes.copy_from_slice(&batch[cursor..cursor + 4]);
        let num_messages = u32::from_le_bytes(num_bytes) as usize;
        cursor += 4;

        let mut messages = Vec::with_capacity(num_messages);

        // Read each message
        for _ in 0..num_messages {
            if cursor + 4 > batch.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Incomplete batch",
                ));
            }

            // Read message length
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&batch[cursor..cursor + 4]);
            let msg_len = u32::from_le_bytes(len_bytes) as usize;
            cursor += 4;

            if cursor + msg_len > batch.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Incomplete message",
                ));
            }

            // Read message data
            let message = batch[cursor..cursor + msg_len].to_vec();
            cursor += msg_len;

            messages.push(message);
        }

        debug!(
            "Unbatched {} messages from {} bytes",
            messages.len(),
            batch.len()
        );

        Ok(messages)
    }

    /// Get batch statistics
    pub fn stats(&self) -> &BatchStats {
        &self.stats
    }

    /// Check if there are pending messages
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Get number of pending messages
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}
