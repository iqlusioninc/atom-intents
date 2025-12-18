//! Configuration management for ATOM Intent-Based Liquidity System
//!
//! This crate provides centralized configuration management with support for:
//! - Multiple config formats (TOML, YAML, JSON)
//! - Environment variable overrides
//! - Config validation
//! - Hot-reload support
//! - Default configs for mainnet, testnet, and local environments

mod config;
mod loader;
mod validation;
mod watcher;

pub use config::*;
pub use loader::*;
pub use validation::*;
pub use watcher::*;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to load config: {0}")]
    LoadError(String),

    #[error("Failed to parse config: {0}")]
    ParseError(String),

    #[error("Config validation failed: {0}")]
    ValidationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Config library error: {0}")]
    ConfigLibError(#[from] ::config::ConfigError),

    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),

    #[error("YAML parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Watch error: {0}")]
    WatchError(String),
}

pub type Result<T> = std::result::Result<T, ConfigError>;
