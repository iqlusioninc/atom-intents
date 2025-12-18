//! Hot-reload configuration watcher

use crate::{AppConfig, ConfigError, ConfigLoader, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Configuration watcher that monitors a config file for changes
///
/// Provides hot-reload capability by watching the config file and automatically
/// reloading when changes are detected.
pub struct ConfigWatcher {
    /// Current configuration
    config: Arc<RwLock<AppConfig>>,
    /// Path to the config file being watched
    path: PathBuf,
}

impl ConfigWatcher {
    /// Create a new config watcher
    ///
    /// Loads the initial configuration from the specified path
    pub fn new(path: PathBuf) -> Result<Self> {
        let config = ConfigLoader::from_file(&path)?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            path,
        })
    }

    /// Get a clone of the current configuration
    ///
    /// This acquires a read lock on the config
    pub fn get_config(&self) -> AppConfig {
        self.config.read().expect("Config lock poisoned").clone()
    }

    /// Start watching the config file for changes
    ///
    /// Returns a join handle for the watcher task. The task will run until dropped.
    pub fn start_watching(&self) -> Result<JoinHandle<()>> {
        let config = Arc::clone(&self.config);
        let path = self.path.clone();

        // Create a channel for file system events
        let (tx, mut rx) = mpsc::channel(100);

        // Set up the file watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| match res {
                Ok(event) => {
                    if let Err(e) = tx.blocking_send(event) {
                        error!("Failed to send file event: {}", e);
                    }
                }
                Err(e) => error!("File watch error: {}", e),
            },
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| ConfigError::WatchError(e.to_string()))?;

        // Watch the config file
        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .map_err(|e| ConfigError::WatchError(e.to_string()))?;

        info!("Started watching config file: {:?}", path);

        // Spawn the watcher task
        let handle = tokio::spawn(async move {
            // Keep the watcher alive by moving it into the task
            let _watcher = watcher;

            while let Some(event) = rx.recv().await {
                // Only reload on modify events
                if matches!(event.kind, EventKind::Modify(_)) {
                    debug!("Config file modified, reloading...");

                    match ConfigLoader::from_file(&path) {
                        Ok(new_config) => match config.write() {
                            Ok(mut guard) => {
                                *guard = new_config;
                                info!("Config reloaded successfully");
                            }
                            Err(e) => {
                                error!("Failed to acquire write lock for config reload: {}", e);
                            }
                        },
                        Err(e) => {
                            warn!("Failed to reload config: {}. Keeping old config.", e);
                        }
                    }
                }
            }

            debug!("Config watcher task stopped");
        });

        Ok(handle)
    }

    /// Create a watcher and start watching immediately
    ///
    /// This is a convenience method that combines `new` and `start_watching`
    pub fn watch(path: PathBuf) -> Result<(Self, JoinHandle<()>)> {
        let watcher = Self::new(path)?;
        let handle = watcher.start_watching()?;
        Ok((watcher, handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_config_watcher_basic() {
        let toml = r#"
[network]
environment = "testnet"
log_level = "info"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
provider = "slinky"
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
        "#;

        let mut file = tempfile::Builder::new().suffix(".toml").tempfile().unwrap();
        file.write_all(toml.as_bytes()).unwrap();
        file.flush().unwrap();

        let watcher = ConfigWatcher::new(file.path().to_path_buf()).unwrap();
        let config = watcher.get_config();

        assert_eq!(config.network.log_level, "info");
    }

    #[tokio::test]
    async fn test_config_watcher_reload() {
        let initial_toml = r#"
[network]
environment = "testnet"
log_level = "info"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
provider = "slinky"
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
        "#;

        // Create a persistent temp file
        let mut file = tempfile::Builder::new().suffix(".toml").tempfile().unwrap();
        file.write_all(initial_toml.as_bytes()).unwrap();
        file.flush().unwrap();

        let path = file.path().to_path_buf();
        let (watcher, _handle) = ConfigWatcher::watch(path.clone()).unwrap();

        // Verify initial config
        let config = watcher.get_config();
        assert_eq!(config.network.log_level, "info");

        // Give the watcher time to start
        sleep(Duration::from_millis(100)).await;

        // Update the config file
        let updated_toml = r#"
[network]
environment = "testnet"
log_level = "debug"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
provider = "slinky"
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
        "#;

        std::fs::write(&path, updated_toml).unwrap();

        // Wait for the file watcher to detect the change and reload
        sleep(Duration::from_secs(3)).await;

        // Verify the config was reloaded
        let config = watcher.get_config();
        assert_eq!(config.network.log_level, "debug");
    }

    #[tokio::test]
    async fn test_config_watcher_invalid_update() {
        let initial_toml = r#"
[network]
environment = "testnet"
log_level = "info"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
provider = "slinky"
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
        "#;

        let mut file = tempfile::Builder::new().suffix(".toml").tempfile().unwrap();
        file.write_all(initial_toml.as_bytes()).unwrap();
        file.flush().unwrap();

        let path = file.path().to_path_buf();
        let (watcher, _handle) = ConfigWatcher::watch(path.clone()).unwrap();

        // Verify initial config
        let config = watcher.get_config();
        assert_eq!(config.network.log_level, "info");

        // Give the watcher time to start
        sleep(Duration::from_millis(100)).await;

        // Write invalid TOML
        std::fs::write(&path, "invalid toml {{[[]").unwrap();

        // Wait for the file watcher to attempt reload
        sleep(Duration::from_secs(3)).await;

        // Verify the old config is still intact
        let config = watcher.get_config();
        assert_eq!(config.network.log_level, "info");
    }
}
