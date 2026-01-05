//! Security middleware for production deployment
//!
//! Provides rate limiting, API key authentication, and request validation.

use axum::{
    extract::{ConnectInfo, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per minute for anonymous users
    pub anonymous_rpm: u32,
    /// Requests per minute for authenticated users
    pub authenticated_rpm: u32,
    /// Requests per minute for solvers
    pub solver_rpm: u32,
    /// Burst size multiplier
    pub burst_multiplier: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            anonymous_rpm: 30,
            authenticated_rpm: 100,
            solver_rpm: 500,
            burst_multiplier: 2,
        }
    }
}

/// Rate limiter state
pub struct RateLimitState {
    config: RateLimitConfig,
    /// In-memory rate limiters by IP
    ip_limiters: RwLock<HashMap<String, Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>>>,
    /// Redis connection for distributed rate limiting
    redis: Option<redis::aio::ConnectionManager>,
}

impl RateLimitState {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            ip_limiters: RwLock::new(HashMap::new()),
            redis: None,
        }
    }

    pub async fn with_redis(mut self, redis_url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        self.redis = Some(redis::aio::ConnectionManager::new(client).await?);
        Ok(self)
    }

    async fn check_rate_limit(&self, key: &str, rpm: u32) -> Result<(), RateLimitError> {
        // Try Redis first for distributed rate limiting
        if let Some(redis) = &self.redis {
            return self.check_redis_rate_limit(redis.clone(), key, rpm).await;
        }

        // Fall back to in-memory rate limiting
        self.check_memory_rate_limit(key, rpm).await
    }

    async fn check_redis_rate_limit(
        &self,
        mut redis: redis::aio::ConnectionManager,
        key: &str,
        rpm: u32,
    ) -> Result<(), RateLimitError> {
        let redis_key = format!("ratelimit:{}", key);
        let window = 60; // 1 minute window

        // Use Redis INCR with TTL for sliding window
        let count: i64 = redis.incr(&redis_key, 1).await.unwrap_or(1);

        if count == 1 {
            let _: () = redis.expire(&redis_key, window).await.unwrap_or_default();
        }

        if count > rpm as i64 {
            let ttl: i64 = redis.ttl(&redis_key).await.unwrap_or(window);
            return Err(RateLimitError {
                limit: rpm,
                remaining: 0,
                reset_seconds: ttl as u32,
            });
        }

        Ok(())
    }

    async fn check_memory_rate_limit(&self, key: &str, rpm: u32) -> Result<(), RateLimitError> {
        let limiter = {
            let limiters = self.ip_limiters.read().await;
            limiters.get(key).cloned()
        };

        let limiter = match limiter {
            Some(l) => l,
            None => {
                let quota = Quota::per_minute(NonZeroU32::new(rpm).unwrap())
                    .allow_burst(NonZeroU32::new(rpm * self.config.burst_multiplier).unwrap());
                let new_limiter = Arc::new(RateLimiter::direct(quota));
                let mut limiters = self.ip_limiters.write().await;
                limiters.insert(key.to_string(), new_limiter.clone());
                new_limiter
            }
        };

        match limiter.check() {
            Ok(_) => Ok(()),
            Err(_) => Err(RateLimitError {
                limit: rpm,
                remaining: 0,
                reset_seconds: 60,
            }),
        }
    }
}

#[derive(Debug)]
pub struct RateLimitError {
    pub limit: u32,
    pub remaining: u32,
    pub reset_seconds: u32,
}

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "rate_limit_exceeded",
            "message": "Too many requests. Please slow down.",
            "limit": self.limit,
            "remaining": self.remaining,
            "reset_seconds": self.reset_seconds
        });

        (
            StatusCode::TOO_MANY_REQUESTS,
            [
                (header::RETRY_AFTER, self.reset_seconds.to_string()),
                ("X-RateLimit-Limit".to_string(), self.limit.to_string()),
                ("X-RateLimit-Remaining".to_string(), self.remaining.to_string()),
            ],
            Json(body),
        )
            .into_response()
    }
}

/// API key for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub solver_id: Option<String>,
    pub scopes: Vec<String>,
    pub rate_limit: u32,
}

/// API key authentication state
pub struct AuthState {
    /// In-memory API key cache
    keys: RwLock<HashMap<String, ApiKey>>,
    /// Redis for distributed key lookup
    redis: Option<redis::aio::ConnectionManager>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            redis: None,
        }
    }

    pub async fn with_redis(mut self, redis_url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        self.redis = Some(redis::aio::ConnectionManager::new(client).await?);
        Ok(self)
    }

    /// Register a demo API key (for testing)
    pub async fn register_demo_key(&self, key: &str, api_key: ApiKey) {
        let hash = hash_api_key(key);
        let mut keys = self.keys.write().await;
        keys.insert(hash, api_key);
    }

    /// Validate API key and return key info
    pub async fn validate_key(&self, key: &str) -> Option<ApiKey> {
        let hash = hash_api_key(key);

        // Check in-memory cache first
        {
            let keys = self.keys.read().await;
            if let Some(api_key) = keys.get(&hash) {
                return Some(api_key.clone());
            }
        }

        // Check Redis if available
        if let Some(redis) = &self.redis {
            if let Ok(data) = redis.clone().get::<_, Option<String>>(&format!("apikey:{}", hash)).await {
                if let Some(json) = data {
                    if let Ok(api_key) = serde_json::from_str::<ApiKey>(&json) {
                        // Cache locally
                        let mut keys = self.keys.write().await;
                        keys.insert(hash, api_key.clone());
                        return Some(api_key);
                    }
                }
            }
        }

        None
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Security context extracted from request
#[derive(Debug, Clone, Default)]
pub struct SecurityContext {
    pub client_ip: String,
    pub api_key: Option<ApiKey>,
    pub is_authenticated: bool,
    pub is_solver: bool,
}

/// Rate limiting middleware
pub async fn rate_limit_middleware<B>(
    State(rate_limiter): State<Arc<RateLimitState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, RateLimitError> {
    let client_ip = get_client_ip(&request, addr);

    // Get API key if present
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Determine rate limit based on authentication
    let (key, rpm) = if let Some(_key) = api_key {
        // Authenticated request - higher limit
        (format!("auth:{}", client_ip), rate_limiter.config.authenticated_rpm)
    } else {
        // Anonymous request
        (format!("anon:{}", client_ip), rate_limiter.config.anonymous_rpm)
    };

    // Check rate limit
    rate_limiter.check_rate_limit(&key, rpm).await?;

    Ok(next.run(request).await)
}

/// Authentication middleware
pub async fn auth_middleware<B>(
    State(auth_state): State<Arc<AuthState>>,
    mut request: Request<B>,
    next: Next<B>,
) -> Response {
    let mut context = SecurityContext::default();

    // Extract client IP
    if let Some(addr) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        context.client_ip = addr.0.ip().to_string();
    }

    // Check for API key
    if let Some(key_header) = request.headers().get("X-API-Key") {
        if let Ok(key) = key_header.to_str() {
            if let Some(api_key) = auth_state.validate_key(key).await {
                context.is_authenticated = true;
                context.is_solver = api_key.solver_id.is_some();
                context.api_key = Some(api_key);
            } else {
                warn!("Invalid API key attempted");
            }
        }
    }

    // Insert security context into request extensions
    request.extensions_mut().insert(context);

    next.run(request).await
}

/// Extract client IP from request, considering X-Forwarded-For header
fn get_client_ip<B>(request: &Request<B>, addr: SocketAddr) -> String {
    // Check X-Forwarded-For header (set by load balancer)
    if let Some(forwarded) = request.headers().get("X-Forwarded-For") {
        if let Ok(value) = forwarded.to_str() {
            // Take the first IP in the chain (original client)
            if let Some(ip) = value.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }

    // Fall back to direct connection IP
    addr.ip().to_string()
}

/// Request validation middleware
pub async fn validate_request_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // Validate content type for POST/PUT requests
    if matches!(request.method().as_str(), "POST" | "PUT" | "PATCH") {
        if let Some(content_type) = request.headers().get(header::CONTENT_TYPE) {
            let content_type = content_type.to_str().unwrap_or("");
            if !content_type.starts_with("application/json") {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    Json(serde_json::json!({
                        "error": "unsupported_media_type",
                        "message": "Content-Type must be application/json"
                    })),
                ));
            }
        }
    }

    // Validate request size (handled by body limit, but add explicit check)
    if let Some(content_length) = request.headers().get(header::CONTENT_LENGTH) {
        if let Ok(length) = content_length.to_str().unwrap_or("0").parse::<usize>() {
            const MAX_BODY_SIZE: usize = 1024 * 1024; // 1MB
            if length > MAX_BODY_SIZE {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(serde_json::json!({
                        "error": "payload_too_large",
                        "message": format!("Request body exceeds maximum size of {} bytes", MAX_BODY_SIZE)
                    })),
                ));
            }
        }
    }

    Ok(next.run(request).await)
}

/// CORS configuration for production
pub fn create_cors_layer(allowed_origins: Vec<String>) -> tower_http::cors::CorsLayer {
    use tower_http::cors::{AllowOrigin, CorsLayer};

    let origins: Vec<_> = allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(3600))
}

/// Security headers middleware
pub async fn security_headers_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();

    // Security headers
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    headers.insert(
        header::HeaderName::from_static("x-xss-protection"),
        "1; mode=block".parse().unwrap(),
    );
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=31536000; includeSubDomains".parse().unwrap(),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        "default-src 'self'".parse().unwrap(),
    );

    response
}

/// Hex encoding utility
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key() {
        let hash1 = hash_api_key("test-key-1");
        let hash2 = hash_api_key("test-key-1");
        let hash3 = hash_api_key("test-key-2");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA256 hex = 64 chars
    }
}
