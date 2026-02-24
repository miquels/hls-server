//! Rate limiting and connection limiting middleware

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per second limit
    pub requests_per_second: u32,
    /// Burst size
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 100,
            burst_size: 50,
        }
    }
}

/// Token bucket rate limiter
#[derive(Debug)]
pub struct TokenBucket {
    /// Maximum tokens (burst size)
    max_tokens: u32,
    /// Current tokens
    tokens: u32,
    /// Tokens added per second
    refill_rate: f64,
    /// Last refill time
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(max_tokens: u32, refill_rate: u32) -> Self {
        Self {
            max_tokens,
            tokens: max_tokens,
            refill_rate: refill_rate as f64,
            last_refill: Instant::now(),
        }
    }

    /// Try to consume a token
    pub fn try_consume(&mut self) -> bool {
        self.refill();

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        let tokens_to_add = elapsed * self.refill_rate;
        self.tokens = (self.tokens as f64 + tokens_to_add).min(self.max_tokens as f64) as u32;

        self.last_refill = now;
    }
}

/// Rate limiter state
#[derive(Debug)]
pub struct RateLimiter {
    /// Per-IP rate limiters
    limiters: RwLock<HashMap<SocketAddr, TokenBucket>>,
    /// Configuration
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            limiters: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Check if request is allowed
    pub fn is_allowed(&self, ip: SocketAddr) -> bool {
        let mut limiters = self.limiters.write();

        let limiter = limiters.entry(ip).or_insert_with(|| {
            TokenBucket::new(self.config.burst_size, self.config.requests_per_second)
        });

        limiter.try_consume()
    }

    /// Clean up old entries
    pub fn cleanup(&self, max_age: Duration) {
        let mut limiters = self.limiters.write();
        let now = Instant::now();

        limiters.retain(|_, limiter| now.duration_since(limiter.last_refill) < max_age);
    }
}

/// Connection limiter state
#[derive(Debug)]
pub struct ConnectionLimiter {
    /// Current connections per IP
    connections: RwLock<HashMap<SocketAddr, u32>>,
    /// Maximum connections per IP
    max_connections_per_ip: u32,
    /// Maximum total connections
    max_total_connections: u32,
}

impl ConnectionLimiter {
    pub fn new(max_per_ip: u32, max_total: u32) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            max_connections_per_ip: max_per_ip,
            max_total_connections: max_total,
        }
    }

    /// Try to acquire a connection slot
    pub fn try_acquire(&self, ip: SocketAddr) -> bool {
        let mut connections = self.connections.write();

        // Check total limit
        let total: u32 = connections.values().sum();
        if total >= self.max_total_connections {
            return false;
        }

        // Check per-IP limit
        let ip_connections = connections.entry(ip).or_insert(0);
        if *ip_connections >= self.max_connections_per_ip {
            return false;
        }

        *ip_connections += 1;
        true
    }

    /// Release a connection slot
    pub fn release(&self, ip: SocketAddr) {
        let mut connections = self.connections.write();

        if let Some(count) = connections.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                connections.remove(&ip);
            }
        }
    }

    /// Get current connection count
    pub fn connection_count(&self) -> u32 {
        let connections = self.connections.read();
        connections.values().sum()
    }
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, &'static str)> {
    // Get client IP
    let ip = request
        .extensions()
        .get::<SocketAddr>()
        .copied()
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));

    if !limiter.is_allowed(ip) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded"));
    }

    Ok(next.run(request).await)
}

/// Connection limiting middleware
pub async fn connection_limit_middleware(
    State(limiter): State<Arc<ConnectionLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, &'static str)> {
    // Get client IP
    let ip = request
        .extensions()
        .get::<SocketAddr>()
        .copied()
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));

    if !limiter.try_acquire(ip) {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Too many connections"));
    }

    let response = next.run(request).await;

    // Release connection after response
    limiter.release(ip);

    Ok(response)
}

/// Create rate limiter from config
pub fn create_rate_limiter(config: &crate::config::ServerConfig) -> Arc<RateLimiter> {
    // Default rate limits
    let rate_limit = config.rate_limit_rps.unwrap_or(100);

    Arc::new(RateLimiter::new(RateLimitConfig {
        requests_per_second: rate_limit,
        burst_size: rate_limit / 2,
    }))
}

/// Create connection limiter from config
pub fn create_connection_limiter(config: &crate::config::ServerConfig) -> Arc<ConnectionLimiter> {
    let max_streams = config.max_concurrent_streams.unwrap_or(100) as u32;

    Arc::new(ConnectionLimiter::new(
        max_streams / 10, // Max 10% per IP
        max_streams,      // Max total
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 5);

        // Should allow burst
        for _ in 0..10 {
            assert!(bucket.try_consume());
        }

        // Should be empty now
        assert!(!bucket.try_consume());
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(RateLimitConfig {
            requests_per_second: 10,
            burst_size: 5,
        });

        let ip = SocketAddr::from(([127, 0, 0, 1], 8080));

        // Should allow burst
        for _ in 0..5 {
            assert!(limiter.is_allowed(ip));
        }

        // Should be rate limited
        assert!(!limiter.is_allowed(ip));
    }

    #[test]
    fn test_connection_limiter() {
        let limiter = ConnectionLimiter::new(5, 10);
        let ip = SocketAddr::from(([127, 0, 0, 1], 8080));

        // Should allow up to max per IP
        for _ in 0..5 {
            assert!(limiter.try_acquire(ip));
        }

        // Should reject additional
        assert!(!limiter.try_acquire(ip));

        // Release one
        limiter.release(ip);

        // Should allow again
        assert!(limiter.try_acquire(ip));
    }

    #[test]
    fn test_connection_limiter_total() {
        let limiter = ConnectionLimiter::new(5, 3);

        let ip1 = SocketAddr::from(([127, 0, 0, 1], 8080));
        let ip2 = SocketAddr::from(([127, 0, 0, 2], 8080));
        let ip3 = SocketAddr::from(([127, 0, 0, 3], 8080));

        assert!(limiter.try_acquire(ip1));
        assert!(limiter.try_acquire(ip2));
        assert!(limiter.try_acquire(ip3));

        // Total limit reached
        assert!(!limiter.try_acquire(SocketAddr::from(([127, 0, 0, 4], 8080))));
    }
}
