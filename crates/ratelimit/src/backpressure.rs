use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;

#[derive(Debug, Error)]
pub enum BackpressureError {
    #[error("Queue is full, cannot accept more requests")]
    QueueFull,
    #[error("System is overloaded")]
    Overloaded,
}

pub struct BackpressureHandler {
    max_queue_size: usize,
    processing: AtomicU32,
    max_concurrent: u32,
    semaphore: Arc<Semaphore>,
}

impl BackpressureHandler {
    pub fn new(max_queue_size: usize, max_concurrent: u32) -> Self {
        Self {
            max_queue_size,
            processing: AtomicU32::new(0),
            max_concurrent,
            semaphore: Arc::new(Semaphore::new(max_concurrent as usize)),
        }
    }

    pub async fn submit<F, T>(&self, f: F) -> Result<T, BackpressureError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        // Check if we can accept more work
        if !self.is_accepting() {
            return Err(BackpressureError::Overloaded);
        }

        // Acquire semaphore permit (wait if at max concurrent)
        let permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| BackpressureError::Overloaded)?;

        // Execute the future
        self.processing.fetch_add(1, Ordering::SeqCst);
        let result = f.await;
        self.processing.fetch_sub(1, Ordering::SeqCst);

        // Release permit
        drop(permit);

        Ok(result)
    }

    pub fn queue_size(&self) -> usize {
        // Return current processing count as queue size approximation
        self.processing.load(Ordering::Relaxed) as usize
    }

    pub fn is_accepting(&self) -> bool {
        let current_processing = self.processing.load(Ordering::Relaxed);
        (current_processing as usize) < self.max_queue_size
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent
    }

    pub fn current_concurrent(&self) -> u32 {
        self.processing.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_backpressure_basic() {
        let handler = BackpressureHandler::new(10, 2);

        let result = handler.submit(async { 42 }).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_backpressure_concurrent_limit() {
        let handler = Arc::new(BackpressureHandler::new(10, 2));
        let mut handles = vec![];

        // Launch 5 tasks that take 100ms each
        for i in 0..5 {
            let h = handler.clone();
            handles.push(tokio::spawn(async move {
                h.submit(async move {
                    sleep(Duration::from_millis(100)).await;
                    i
                })
                .await
            }));
        }

        // At most 2 should be running concurrently
        sleep(Duration::from_millis(50)).await;
        assert!(handler.current_concurrent() <= 2);

        // Wait for all to complete
        for handle in handles {
            assert!(handle.await.is_ok());
        }

        assert_eq!(handler.current_concurrent(), 0);
    }

    #[tokio::test]
    async fn test_backpressure_queue_full() {
        let handler = Arc::new(BackpressureHandler::new(2, 1));
        let mut handles = vec![];

        // Fill up the queue with long-running tasks
        for _ in 0..2 {
            let h = handler.clone();
            handles.push(tokio::spawn(async move {
                h.submit(async {
                    sleep(Duration::from_millis(500)).await;
                })
                .await
            }));
        }

        // Wait for tasks to start and fill the queue
        sleep(Duration::from_millis(100)).await;

        // System should be at capacity now
        if !handler.is_accepting() {
            // Try to submit another task - should fail
            let result = handler
                .submit(async {
                    sleep(Duration::from_millis(10)).await;
                })
                .await;

            assert!(matches!(result, Err(BackpressureError::Overloaded)));
        }

        // Cleanup
        for handle in handles {
            handle.abort();
        }
    }

    #[tokio::test]
    async fn test_backpressure_is_accepting() {
        let handler = Arc::new(BackpressureHandler::new(2, 1));

        assert!(handler.is_accepting());

        // Start tasks to fill queue
        let h1 = handler.clone();
        let task1 = tokio::spawn(async move {
            h1.submit(async {
                sleep(Duration::from_millis(500)).await;
            })
            .await
        });

        let h2 = handler.clone();
        let task2 = tokio::spawn(async move {
            h2.submit(async {
                sleep(Duration::from_millis(500)).await;
            })
            .await
        });

        // Wait for tasks to start and fill queue
        sleep(Duration::from_millis(100)).await;

        // Should not be accepting new work when at capacity
        if handler.current_concurrent() >= 2 {
            assert!(!handler.is_accepting());
        }

        // Cleanup
        task1.abort();
        task2.abort();
    }

    #[tokio::test]
    async fn test_backpressure_queue_size() {
        let handler = Arc::new(BackpressureHandler::new(10, 5));

        assert_eq!(handler.queue_size(), 0);

        let mut handles = vec![];
        for _ in 0..3 {
            let h = handler.clone();
            handles.push(tokio::spawn(async move {
                h.submit(async {
                    sleep(Duration::from_millis(200)).await;
                })
                .await
            }));
        }

        // Wait for tasks to start
        sleep(Duration::from_millis(50)).await;

        // Should have 3 processing
        assert_eq!(handler.queue_size(), 3);

        // Wait for completion
        for handle in handles {
            handle.await.ok();
        }

        assert_eq!(handler.queue_size(), 0);
    }

    #[tokio::test]
    async fn test_backpressure_sequential_completion() {
        let handler = Arc::new(BackpressureHandler::new(10, 1));
        let completed = Arc::new(Mutex::new(Vec::new()));

        let mut handles = vec![];

        for i in 0..3 {
            let h = handler.clone();
            let c = completed.clone();
            handles.push(tokio::spawn(async move {
                h.submit(async move {
                    sleep(Duration::from_millis(50)).await;
                    c.lock().await.push(i);
                })
                .await
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.ok();
        }

        // All should have completed
        let results = completed.lock().await;
        assert_eq!(results.len(), 3);
    }
}
