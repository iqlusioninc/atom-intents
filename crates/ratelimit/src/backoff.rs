use std::time::Duration;

pub struct ExponentialBackoff {
    initial: Duration,
    max: Duration,
    multiplier: f64,
    current_attempt: u32,
}

impl ExponentialBackoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            multiplier: 2.0,
            current_attempt: 0,
        }
    }

    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = if self.current_attempt == 0 {
            self.initial
        } else {
            let multiplier = self.multiplier.powi(self.current_attempt as i32);
            let delay_ms = self.initial.as_millis() as f64 * multiplier;
            let delay_ms = delay_ms.min(self.max.as_millis() as f64);
            Duration::from_millis(delay_ms as u64)
        };

        self.current_attempt += 1;
        delay
    }

    pub fn reset(&mut self) {
        self.current_attempt = 0;
    }

    pub fn current_attempt(&self) -> u32 {
        self.current_attempt
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(100), Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_initial() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10));

        let delay = backoff.next_delay();
        assert_eq!(delay, Duration::from_millis(100));
        assert_eq!(backoff.current_attempt(), 1);
    }

    #[test]
    fn test_exponential_backoff_progression() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10));

        let delay1 = backoff.next_delay();
        assert_eq!(delay1, Duration::from_millis(100));

        let delay2 = backoff.next_delay();
        assert_eq!(delay2, Duration::from_millis(200));

        let delay3 = backoff.next_delay();
        assert_eq!(delay3, Duration::from_millis(400));

        let delay4 = backoff.next_delay();
        assert_eq!(delay4, Duration::from_millis(800));
    }

    #[test]
    fn test_exponential_backoff_max_cap() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(1));

        // Keep calling until we hit the max
        for _ in 0..20 {
            let delay = backoff.next_delay();
            assert!(delay <= Duration::from_secs(1));
        }

        // Should be capped at max
        let delay = backoff.next_delay();
        assert_eq!(delay, Duration::from_secs(1));
    }

    #[test]
    fn test_exponential_backoff_reset() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10));

        backoff.next_delay();
        backoff.next_delay();
        backoff.next_delay();
        assert_eq!(backoff.current_attempt(), 3);

        backoff.reset();
        assert_eq!(backoff.current_attempt(), 0);

        let delay = backoff.next_delay();
        assert_eq!(delay, Duration::from_millis(100));
    }

    #[test]
    fn test_exponential_backoff_custom_multiplier() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10))
                .with_multiplier(3.0);

        let delay1 = backoff.next_delay();
        assert_eq!(delay1, Duration::from_millis(100));

        let delay2 = backoff.next_delay();
        assert_eq!(delay2, Duration::from_millis(300));

        let delay3 = backoff.next_delay();
        assert_eq!(delay3, Duration::from_millis(900));
    }

    #[test]
    fn test_exponential_backoff_default() {
        let mut backoff = ExponentialBackoff::default();

        let delay1 = backoff.next_delay();
        assert_eq!(delay1, Duration::from_millis(100));

        let delay2 = backoff.next_delay();
        assert_eq!(delay2, Duration::from_millis(200));

        // Keep going until we hit max
        for _ in 0..20 {
            backoff.next_delay();
        }

        let delay = backoff.next_delay();
        assert_eq!(delay, Duration::from_secs(30));
    }

    #[test]
    fn test_exponential_backoff_sequence() {
        let mut backoff =
            ExponentialBackoff::new(Duration::from_millis(50), Duration::from_secs(5));

        let expected = vec![
            Duration::from_millis(50),   // 2^0 * 50 = 50
            Duration::from_millis(100),  // 2^1 * 50 = 100
            Duration::from_millis(200),  // 2^2 * 50 = 200
            Duration::from_millis(400),  // 2^3 * 50 = 400
            Duration::from_millis(800),  // 2^4 * 50 = 800
            Duration::from_millis(1600), // 2^5 * 50 = 1600
            Duration::from_millis(3200), // 2^6 * 50 = 3200
        ];

        for expected_delay in expected {
            let delay = backoff.next_delay();
            assert_eq!(delay, expected_delay);
        }
    }
}
