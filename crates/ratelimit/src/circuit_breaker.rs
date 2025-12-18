use std::future::Future;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open")]
    Open,
    #[error("Operation failed: {0}")]
    Operation(E),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, reject requests
    HalfOpen, // Testing if recovered
}

impl From<u8> for CircuitState {
    fn from(value: u8) -> Self {
        match value {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

impl From<CircuitState> for u8 {
    fn from(state: CircuitState) -> Self {
        match state {
            CircuitState::Closed => 0,
            CircuitState::Open => 1,
            CircuitState::HalfOpen => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_duration: Duration,
    pub half_open_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            timeout_duration: Duration::from_secs(60),
            half_open_requests: 3,
        }
    }
}

pub struct CircuitBreaker {
    state: AtomicU8,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure: AtomicU64,
    half_open_attempts: AtomicU32,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: AtomicU8::new(CircuitState::Closed.into()),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure: AtomicU64::new(0),
            half_open_attempts: AtomicU32::new(0),
            config,
        }
    }

    pub fn state(&self) -> CircuitState {
        let state_value = self.state.load(Ordering::Relaxed);
        CircuitState::from(state_value)
    }

    fn should_attempt_reset(&self) -> bool {
        let last_failure = self.last_failure.load(Ordering::Relaxed);
        if last_failure == 0 {
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let elapsed = now.saturating_sub(last_failure);
        elapsed >= self.config.timeout_duration.as_secs()
    }

    fn try_transition_to_half_open(&self) -> bool {
        if self.should_attempt_reset() {
            self.state
                .compare_exchange(
                    CircuitState::Open.into(),
                    CircuitState::HalfOpen.into(),
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                )
                .is_ok()
        } else {
            false
        }
    }

    pub fn record_success(&self) {
        let current_state = self.state();

        match current_state {
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    // Transition to closed
                    self.state.store(CircuitState::Closed.into(), Ordering::SeqCst);
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.success_count.store(0, Ordering::Relaxed);
                    self.half_open_attempts.store(0, Ordering::Relaxed);
                    tracing::info!("Circuit breaker transitioned to CLOSED");
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {}
        }
    }

    pub fn record_failure(&self) {
        let current_state = self.state();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.last_failure.store(now, Ordering::Relaxed);

        match current_state {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold {
                    // Transition to open
                    self.state.store(CircuitState::Open.into(), Ordering::SeqCst);
                    self.success_count.store(0, Ordering::Relaxed);
                    tracing::warn!("Circuit breaker transitioned to OPEN after {} failures", failures);
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open goes back to open
                self.state.store(CircuitState::Open.into(), Ordering::SeqCst);
                self.success_count.store(0, Ordering::Relaxed);
                self.half_open_attempts.store(0, Ordering::Relaxed);
                tracing::warn!("Circuit breaker transitioned back to OPEN from HALF_OPEN");
            }
            CircuitState::Open => {}
        }
    }

    pub fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => {
                match f() {
                    Ok(result) => {
                        self.record_success();
                        Ok(result)
                    }
                    Err(e) => {
                        self.record_failure();
                        Err(CircuitBreakerError::Operation(e))
                    }
                }
            }
            CircuitState::Open => {
                if self.try_transition_to_half_open() {
                    // Successfully transitioned to half-open, try the operation
                    match f() {
                        Ok(result) => {
                            self.record_success();
                            Ok(result)
                        }
                        Err(e) => {
                            self.record_failure();
                            Err(CircuitBreakerError::Operation(e))
                        }
                    }
                } else {
                    Err(CircuitBreakerError::Open)
                }
            }
            CircuitState::HalfOpen => {
                let attempts = self.half_open_attempts.fetch_add(1, Ordering::SeqCst);
                if attempts < self.config.half_open_requests {
                    match f() {
                        Ok(result) => {
                            self.record_success();
                            Ok(result)
                        }
                        Err(e) => {
                            self.record_failure();
                            Err(CircuitBreakerError::Operation(e))
                        }
                    }
                } else {
                    // Too many half-open attempts
                    Err(CircuitBreakerError::Open)
                }
            }
        }
    }

    pub async fn call_async<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => {
                match f().await {
                    Ok(result) => {
                        self.record_success();
                        Ok(result)
                    }
                    Err(e) => {
                        self.record_failure();
                        Err(CircuitBreakerError::Operation(e))
                    }
                }
            }
            CircuitState::Open => {
                if self.try_transition_to_half_open() {
                    // Successfully transitioned to half-open, try the operation
                    match f().await {
                        Ok(result) => {
                            self.record_success();
                            Ok(result)
                        }
                        Err(e) => {
                            self.record_failure();
                            Err(CircuitBreakerError::Operation(e))
                        }
                    }
                } else {
                    Err(CircuitBreakerError::Open)
                }
            }
            CircuitState::HalfOpen => {
                let attempts = self.half_open_attempts.fetch_add(1, Ordering::SeqCst);
                if attempts < self.config.half_open_requests {
                    match f().await {
                        Ok(result) => {
                            self.record_success();
                            Ok(result)
                        }
                        Err(e) => {
                            self.record_failure();
                            Err(CircuitBreakerError::Operation(e))
                        }
                    }
                } else {
                    // Too many half-open attempts
                    Err(CircuitBreakerError::Open)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Simulate failures
        for i in 0..3 {
            let result = cb.call(|| -> Result<(), &str> { Err("error") });
            assert!(result.is_err());

            if i < 2 {
                assert_eq!(cb.state(), CircuitState::Closed);
            }
        }

        // Should be open after threshold failures
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_rejects_when_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Trigger open state
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();

        assert_eq!(cb.state(), CircuitState::Open);

        // Should reject immediately
        let result = cb.call(|| -> Result<(), &str> { Ok(()) });
        assert!(matches!(result, Err(CircuitBreakerError::Open)));
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_duration: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // First call after timeout should transition to half-open and succeed
        let result = cb.call(|| -> Result<i32, &str> { Ok(42) });
        assert!(result.is_ok());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Second success should close the circuit
        let result = cb.call(|| -> Result<i32, &str> { Ok(42) });
        assert!(result.is_ok());
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout_duration: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Failure in half-open should return to open
        let result = cb.call(|| -> Result<(), &str> { Err("error") });
        assert!(result.is_err());
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_async() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Test async success
        let result = cb.call_async(|| async { Ok::<i32, &str>(42) }).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        // Test async failure
        let result = cb.call_async(|| async { Err::<i32, &str>("error") }).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_circuit_breaker_success_resets_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Two failures
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        assert_eq!(cb.state(), CircuitState::Closed);

        // Success should reset
        cb.call(|| -> Result<i32, &str> { Ok(42) }).ok();

        // Should need 3 more failures to open
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.call(|| -> Result<(), &str> { Err("error") }).ok();
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
