//! Error recovery strategies and mechanisms
//!
//! Provides comprehensive error recovery including:
//! - Exponential backoff for connection failures
//! - Throttling for rate-limited peers
//! - Automatic reconnection scheduling
//! - Error logging with context
//! - Recovery state tracking

use crate::error::{NetworkError, RecoveryStrategy, Result};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Exponential backoff calculator
///
/// Implements exponential backoff with jitter for connection retries.
/// Starts at initial_delay and increases exponentially up to max_delay.
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// Initial delay in milliseconds
    initial_delay_ms: u64,

    /// Maximum delay in milliseconds
    max_delay_ms: u64,

    /// Current attempt count
    attempt: u32,

    /// Last backoff time
    last_backoff: Option<Instant>,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff calculator
    pub fn new(initial_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            initial_delay_ms,
            max_delay_ms,
            attempt: 0,
            last_backoff: None,
        }
    }

    /// Calculate next backoff duration
    pub fn next_backoff(&mut self) -> Duration {
        // Calculate exponential backoff: initial_delay * 2^attempt
        let delay_ms = self
            .initial_delay_ms
            .saturating_mul(2_u64.saturating_pow(self.attempt))
            .min(self.max_delay_ms);

        // Add jitter (±10%)
        let jitter = (delay_ms as f64 * 0.1) as u64;
        let jittered_delay = delay_ms.saturating_sub(jitter / 2).saturating_add(
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64)
                % jitter,
        );

        self.attempt = self.attempt.saturating_add(1);
        self.last_backoff = Some(Instant::now());

        Duration::from_millis(jittered_delay)
    }

    /// Reset backoff on successful connection
    pub fn reset(&mut self) {
        self.attempt = 0;
        self.last_backoff = None;
        debug!("Backoff reset to initial state");
    }

    /// Get current attempt count
    pub fn attempt_count(&self) -> u32 {
        self.attempt
    }

    /// Check if ready to retry
    pub fn is_ready_to_retry(&self) -> bool {
        match self.last_backoff {
            None => true,
            Some(last) => {
                // Calculate expected delay without mutating
                let delay_ms = self
                    .initial_delay_ms
                    .saturating_mul(2_u64.saturating_pow(self.attempt))
                    .min(self.max_delay_ms);
                let expected_delay = Duration::from_millis(delay_ms);
                Instant::now().duration_since(last) >= expected_delay
            }
        }
    }
}

/// Rate limiter for peer throttling
///
/// Tracks message rate per peer and enforces limits.
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum messages per second
    max_messages_per_sec: u32,

    /// Current message count in current second
    message_count: Arc<AtomicU32>,

    /// Last reset time
    last_reset: Arc<parking_lot::Mutex<Instant>>,

    /// Throttle duration when limit exceeded
    throttle_duration: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_messages_per_sec: u32, throttle_duration: Duration) -> Self {
        Self {
            max_messages_per_sec,
            message_count: Arc::new(AtomicU32::new(0)),
            last_reset: Arc::new(parking_lot::Mutex::new(Instant::now())),
            throttle_duration,
        }
    }

    /// Check if message is allowed
    pub fn allow_message(&self) -> bool {
        let mut last_reset = self.last_reset.lock();
        let now = Instant::now();

        // Reset counter if a second has passed
        if now.duration_since(*last_reset) >= Duration::from_secs(1) {
            self.message_count.store(0, Ordering::Relaxed);
            *last_reset = now;
        }

        let current_count = self.message_count.fetch_add(1, Ordering::Relaxed);
        current_count < self.max_messages_per_sec
    }

    /// Get throttle duration
    pub fn throttle_duration(&self) -> Duration {
        self.throttle_duration
    }

    /// Reset rate limiter
    pub fn reset(&self) {
        self.message_count.store(0, Ordering::Relaxed);
        *self.last_reset.lock() = Instant::now();
    }
}

/// Error recovery manager
///
/// Coordinates error recovery strategies and tracks recovery state.
pub struct ErrorRecoveryManager {
    /// Backoff state for each peer
    backoff_states: Arc<parking_lot::RwLock<std::collections::HashMap<String, ExponentialBackoff>>>,

    /// Rate limiters for each peer
    rate_limiters: Arc<parking_lot::RwLock<std::collections::HashMap<String, RateLimiter>>>,

    /// Error count tracking
    error_counts: Arc<parking_lot::RwLock<std::collections::HashMap<String, u32>>>,

    /// Maximum errors before giving up
    max_errors_before_ban: u32,
}

impl ErrorRecoveryManager {
    /// Create a new error recovery manager
    pub fn new(max_errors_before_ban: u32) -> Self {
        Self {
            backoff_states: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            rate_limiters: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            error_counts: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            max_errors_before_ban,
        }
    }

    /// Handle error with recovery strategy
    pub fn handle_error(
        &self,
        peer_id: &str,
        error: &NetworkError,
    ) -> Result<RecoveryAction> {
        let strategy = error.recovery_strategy();

        match strategy {
            RecoveryStrategy::ExponentialBackoff => {
                self.handle_exponential_backoff(peer_id, error)
            }
            RecoveryStrategy::Throttle => self.handle_throttle(peer_id, error),
            RecoveryStrategy::Reconnect => self.handle_reconnect(peer_id, error),
            RecoveryStrategy::Retry => self.handle_retry(peer_id, error),
            RecoveryStrategy::WaitAndRetry => self.handle_wait_and_retry(peer_id, error),
            RecoveryStrategy::Alert => self.handle_alert(peer_id, error),
            RecoveryStrategy::FailStartup => self.handle_fail_startup(peer_id, error),
            RecoveryStrategy::Log => self.handle_log(peer_id, error),
        }
    }

    /// Handle exponential backoff recovery
    fn handle_exponential_backoff(
        &self,
        peer_id: &str,
        error: &NetworkError,
    ) -> Result<RecoveryAction> {
        let mut backoff_states = self.backoff_states.write();
        let backoff = backoff_states
            .entry(peer_id.to_string())
            .or_insert_with(|| ExponentialBackoff::new(1000, 300000)); // 1s to 5min

        let delay = backoff.next_backoff();
        warn!(
            peer_id = peer_id,
            delay_ms = delay.as_millis(),
            attempt = backoff.attempt_count(),
            error = %error,
            "Scheduling reconnection with exponential backoff"
        );

        Ok(RecoveryAction::ScheduleReconnect(delay))
    }

    /// Handle throttle recovery
    fn handle_throttle(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        let mut rate_limiters = self.rate_limiters.write();
        let limiter = rate_limiters
            .entry(peer_id.to_string())
            .or_insert_with(|| RateLimiter::new(100, Duration::from_secs(60)));

        let throttle_duration = limiter.throttle_duration();
        warn!(
            peer_id = peer_id,
            throttle_duration_secs = throttle_duration.as_secs(),
            error = %error,
            "Throttling peer due to rate limit"
        );

        Ok(RecoveryAction::Throttle(throttle_duration))
    }

    /// Handle reconnect recovery
    fn handle_reconnect(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        let mut error_counts = self.error_counts.write();
        let count = error_counts.entry(peer_id.to_string()).or_insert(0);
        *count = count.saturating_add(1);

        if *count > self.max_errors_before_ban {
            error!(
                peer_id = peer_id,
                error_count = count,
                max_errors = self.max_errors_before_ban,
                error = %error,
                "Peer exceeded maximum error threshold - banning"
            );
            return Ok(RecoveryAction::Ban);
        }

        info!(
            peer_id = peer_id,
            error_count = count,
            error = %error,
            "Scheduling reconnection attempt"
        );

        Ok(RecoveryAction::Reconnect)
    }

    /// Handle retry recovery
    fn handle_retry(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        debug!(
            peer_id = peer_id,
            error = %error,
            "Retrying operation"
        );
        Ok(RecoveryAction::Retry)
    }

    /// Handle wait and retry recovery
    fn handle_wait_and_retry(
        &self,
        peer_id: &str,
        error: &NetworkError,
    ) -> Result<RecoveryAction> {
        warn!(
            peer_id = peer_id,
            error = %error,
            "Network partition detected - waiting before retry"
        );
        Ok(RecoveryAction::WaitAndRetry(Duration::from_secs(5)))
    }

    /// Handle alert recovery
    fn handle_alert(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        error!(
            peer_id = peer_id,
            error = %error,
            "CRITICAL ERROR - operator intervention required"
        );
        Ok(RecoveryAction::Alert)
    }

    /// Handle fail startup recovery
    fn handle_fail_startup(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        error!(
            peer_id = peer_id,
            error = %error,
            "Configuration error - failing startup"
        );
        Ok(RecoveryAction::FailStartup)
    }

    /// Handle log recovery
    fn handle_log(&self, peer_id: &str, error: &NetworkError) -> Result<RecoveryAction> {
        debug!(
            peer_id = peer_id,
            error = %error,
            "Logging error"
        );
        Ok(RecoveryAction::Log)
    }

    /// Reset error count for peer
    pub fn reset_error_count(&self, peer_id: &str) {
        let mut error_counts = self.error_counts.write();
        error_counts.remove(peer_id);

        let mut backoff_states = self.backoff_states.write();
        if let Some(backoff) = backoff_states.get_mut(peer_id) {
            backoff.reset();
        }

        info!(peer_id = peer_id, "Error count and backoff reset");
    }

    /// Get error count for peer
    pub fn get_error_count(&self, peer_id: &str) -> u32 {
        self.error_counts
            .read()
            .get(peer_id)
            .copied()
            .unwrap_or(0)
    }

    /// Check if peer should be banned
    pub fn should_ban_peer(&self, peer_id: &str) -> bool {
        self.get_error_count(peer_id) > self.max_errors_before_ban
    }
}

/// Recovery action to take
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Schedule reconnection with delay
    ScheduleReconnect(Duration),

    /// Throttle peer for duration
    Throttle(Duration),

    /// Attempt immediate reconnection
    Reconnect,

    /// Retry operation
    Retry,

    /// Wait and retry
    WaitAndRetry(Duration),

    /// Alert operator
    Alert,

    /// Fail startup
    FailStartup,

    /// Ban peer
    Ban,

    /// Just log
    Log,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff() {
        let mut backoff = ExponentialBackoff::new(1000, 300000);

        let delay1 = backoff.next_backoff();
        assert!(delay1.as_millis() >= 900 && delay1.as_millis() <= 1100);

        let delay2 = backoff.next_backoff();
        assert!(delay2.as_millis() >= 1800 && delay2.as_millis() <= 2200);

        backoff.reset();
        assert_eq!(backoff.attempt_count(), 0);
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(10, Duration::from_secs(60));

        for _ in 0..10 {
            assert!(limiter.allow_message());
        }

        assert!(!limiter.allow_message());
    }

    #[test]
    fn test_error_recovery_manager() {
        let manager = ErrorRecoveryManager::new(5);

        let error = NetworkError::ConnectionTimeout("test".to_string());
        let action = manager.handle_error("peer1", &error);

        assert!(action.is_ok());
        assert_eq!(manager.get_error_count("peer1"), 0);
    }
}
