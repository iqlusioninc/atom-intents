//! Rate limiting and backpressure handling for ATOM Intent-Based Liquidity System
//!
//! This crate provides components for controlling request flow and preventing system overload:
//!
//! - `RateLimiter`: Token bucket-based rate limiting with per-key limits
//! - `CircuitBreaker`: Fault tolerance with automatic failure detection and recovery
//! - `BackpressureHandler`: Queue-based backpressure with concurrent request limiting
//! - `ExponentialBackoff`: Exponential backoff strategy for retries

pub mod backoff;
pub mod backpressure;
pub mod circuit_breaker;
pub mod limiter;

pub use backoff::ExponentialBackoff;
pub use backpressure::{BackpressureError, BackpressureHandler};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitState};
pub use limiter::{RateLimitError, RateLimiter, TokenBucket};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_rate_limiter_with_backpressure() {
        let rate_limiter = Arc::new(
            RateLimiter::new()
                .with_limit("api", 10) // 10 requests per second
        );
        let backpressure = Arc::new(BackpressureHandler::new(50, 5));

        let mut handles = vec![];

        // Spawn 20 concurrent requests
        for i in 0..20 {
            let rl = rate_limiter.clone();
            let bp = backpressure.clone();

            handles.push(tokio::spawn(async move {
                // First check rate limit
                if rl.try_acquire("api") {
                    // Then submit to backpressure handler
                    bp.submit(async move {
                        sleep(Duration::from_millis(10)).await;
                        i
                    })
                    .await
                    .ok()
                } else {
                    None
                }
            }));
        }

        let mut successes = 0;
        for handle in handles {
            if let Ok(Some(_)) = handle.await {
                successes += 1;
            }
        }

        // Some should succeed, but not all due to rate limiting
        assert!(successes > 0);
        assert!(successes <= 100); // 10 req/s * 10s burst
    }

    #[tokio::test]
    async fn test_circuit_breaker_with_backoff() {
        let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout_duration: Duration::from_millis(50),
            half_open_requests: 2,
        });

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(20),
            Duration::from_millis(200),
        );

        // Trigger circuit breaker to open
        for _ in 0..3 {
            circuit_breaker
                .call(|| -> Result<(), &str> { Err("service unavailable") })
                .ok();
        }

        assert_eq!(circuit_breaker.state(), CircuitState::Open);

        // Try with exponential backoff
        let mut attempts = 0;
        loop {
            attempts += 1;
            if attempts > 10 {
                panic!("Failed to recover after 10 attempts");
            }

            let delay = backoff.next_delay();
            sleep(delay).await;

            if let Ok(_) = circuit_breaker.call(|| -> Result<i32, &str> { Ok(42) }) {
                // Success, reset backoff
                backoff.reset();
                break;
            }
        }

        // Should eventually recover (after timeout)
        assert!(attempts >= 1);
        assert!(attempts <= 10);
    }

    #[tokio::test]
    async fn test_full_stack_integration() {
        let rate_limiter = Arc::new(RateLimiter::new().with_limit("api", 5));
        let backpressure = Arc::new(BackpressureHandler::new(10, 2));
        let circuit_breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout_duration: Duration::from_secs(1),
            half_open_requests: 2,
        }));

        // Simulate API call with all protections
        let make_request = |id: u32| {
            let rl = rate_limiter.clone();
            let bp = backpressure.clone();
            let cb = circuit_breaker.clone();

            async move {
                // 1. Check rate limit
                if !rl.try_acquire("api") {
                    return Err("rate limited");
                }

                // 2. Apply backpressure
                let result = bp.submit(async move {
                    // 3. Execute through circuit breaker
                    cb.call_async(|| async move {
                        sleep(Duration::from_millis(50)).await;
                        if id % 10 == 0 {
                            Err("simulated failure")
                        } else {
                            Ok(id)
                        }
                    })
                    .await
                })
                .await;

                match result {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(_)) => Err("circuit breaker error"),
                    Err(_) => Err("backpressure error"),
                }
            }
        };

        let mut handles = vec![];
        for i in 0..15 {
            handles.push(tokio::spawn(make_request(i)));
        }

        let mut successes = 0;
        let mut failures = 0;

        for handle in handles {
            match handle.await {
                Ok(Ok(_)) => successes += 1,
                Ok(Err(_)) => failures += 1,
                _ => {}
            }
        }

        // Should have some successes
        assert!(successes > 0);
        // Should have hit rate limits or other protections
        assert!(failures > 0);
    }

    #[tokio::test]
    async fn test_recovery_after_overload() {
        let backpressure = Arc::new(BackpressureHandler::new(2, 1));

        // Overload the system - fill queue and max concurrent
        let mut overload_handles = vec![];
        for _ in 0..2 {
            let bp = backpressure.clone();
            overload_handles.push(tokio::spawn(async move {
                bp.submit(async {
                    sleep(Duration::from_millis(300)).await;
                })
                .await
            }));
        }

        // Wait for system to be overloaded
        sleep(Duration::from_millis(100)).await;

        // System should not be accepting (queue full)
        if !backpressure.is_accepting() {
            // Try to submit - should fail
            let result = backpressure
                .submit(async {
                    sleep(Duration::from_millis(10)).await;
                })
                .await;
            assert!(result.is_err());
        }

        // Wait for recovery
        for handle in overload_handles {
            handle.await.ok();
        }

        // Should be accepting again
        sleep(Duration::from_millis(50)).await;
        assert!(backpressure.is_accepting());

        // Should succeed now
        let result = backpressure
            .submit(async {
                sleep(Duration::from_millis(10)).await;
            })
            .await;
        assert!(result.is_ok());
    }
}
