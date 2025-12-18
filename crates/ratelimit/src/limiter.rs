use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded for key: {0}")]
    LimitExceeded(String),
    #[error("Invalid rate limit configuration")]
    InvalidConfig,
}

pub struct TokenBucket {
    capacity: u32,
    tokens: AtomicU32,
    refill_rate: u32, // tokens per second
    last_refill: AtomicU64,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_rate: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            capacity,
            tokens: AtomicU32::new(capacity),
            refill_rate,
            last_refill: AtomicU64::new(now),
        }
    }

    fn refill(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let last = self.last_refill.load(Ordering::Relaxed);
        let elapsed = now.saturating_sub(last);

        if elapsed == 0 {
            return;
        }

        // Calculate tokens to add
        let tokens_to_add = (elapsed as u32).saturating_mul(self.refill_rate);
        if tokens_to_add == 0 {
            return;
        }

        // Try to update last_refill timestamp
        if self
            .last_refill
            .compare_exchange(last, now, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            // Successfully updated timestamp, now add tokens
            self.tokens.fetch_update(Ordering::SeqCst, Ordering::Relaxed, |current| {
                Some(std::cmp::min(current.saturating_add(tokens_to_add), self.capacity))
            }).ok();
        }
    }

    pub fn try_acquire(&self) -> bool {
        self.refill();

        self.tokens
            .fetch_update(Ordering::SeqCst, Ordering::Relaxed, |current| {
                if current > 0 {
                    Some(current - 1)
                } else {
                    None
                }
            })
            .is_ok()
    }

    pub fn remaining(&self) -> u32 {
        self.refill();
        self.tokens.load(Ordering::Relaxed)
    }
}

pub struct RateLimiter {
    limits: HashMap<String, TokenBucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
        }
    }

    pub fn with_limit(mut self, key: &str, requests_per_second: u32) -> Self {
        let capacity = requests_per_second * 10; // 10 second burst capacity
        self.limits.insert(
            key.to_string(),
            TokenBucket::new(capacity, requests_per_second),
        );
        self
    }

    pub async fn acquire(&self, key: &str) -> Result<(), RateLimitError> {
        if self.try_acquire(key) {
            Ok(())
        } else {
            Err(RateLimitError::LimitExceeded(key.to_string()))
        }
    }

    pub fn try_acquire(&self, key: &str) -> bool {
        if let Some(bucket) = self.limits.get(key) {
            bucket.try_acquire()
        } else {
            // No limit configured, allow
            true
        }
    }

    pub fn remaining(&self, key: &str) -> u32 {
        if let Some(bucket) = self.limits.get(key) {
            bucket.remaining()
        } else {
            u32::MAX
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_token_bucket_basic() {
        let bucket = TokenBucket::new(10, 5);

        // Should have full capacity initially
        assert_eq!(bucket.remaining(), 10);

        // Consume some tokens
        assert!(bucket.try_acquire());
        assert_eq!(bucket.remaining(), 9);

        // Consume all tokens
        for _ in 0..9 {
            assert!(bucket.try_acquire());
        }
        assert_eq!(bucket.remaining(), 0);

        // Should fail when empty
        assert!(!bucket.try_acquire());
    }

    #[tokio::test]
    async fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(10, 5);

        // Consume all tokens
        for _ in 0..10 {
            assert!(bucket.try_acquire());
        }
        assert_eq!(bucket.remaining(), 0);

        // Wait for refill (5 tokens per second)
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Should have refilled
        assert!(bucket.remaining() >= 4); // Allow some timing variance
        assert!(bucket.try_acquire());
    }

    #[test]
    fn test_rate_limiter_with_limit() {
        let limiter = RateLimiter::new()
            .with_limit("test", 10);

        // Should allow requests up to capacity
        for _ in 0..100 {
            assert!(limiter.try_acquire("test"));
        }

        // Should block when limit exceeded
        assert!(!limiter.try_acquire("test"));
    }

    #[test]
    fn test_rate_limiter_multiple_keys() {
        let limiter = RateLimiter::new()
            .with_limit("key1", 5)
            .with_limit("key2", 10);

        // Consume key1's capacity
        for _ in 0..50 {
            assert!(limiter.try_acquire("key1"));
        }
        assert!(!limiter.try_acquire("key1"));

        // key2 should still work
        assert!(limiter.try_acquire("key2"));
    }

    #[test]
    fn test_rate_limiter_no_limit() {
        let limiter = RateLimiter::new();

        // Should allow unlimited requests for unconfigured keys
        for _ in 0..1000 {
            assert!(limiter.try_acquire("unlimited"));
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_async_acquire() {
        let limiter = RateLimiter::new()
            .with_limit("async_test", 10);

        // Should succeed when tokens available
        assert!(limiter.acquire("async_test").await.is_ok());

        // Consume remaining tokens
        for _ in 0..99 {
            assert!(limiter.try_acquire("async_test"));
        }

        // Should fail when exhausted
        assert!(limiter.acquire("async_test").await.is_err());
    }

    #[test]
    fn test_rate_limiter_remaining() {
        let limiter = RateLimiter::new()
            .with_limit("test", 10);

        assert_eq!(limiter.remaining("test"), 100); // 10 req/s * 10s burst

        limiter.try_acquire("test");
        assert_eq!(limiter.remaining("test"), 99);

        // Unconfigured key should return max
        assert_eq!(limiter.remaining("unknown"), u32::MAX);
    }
}
