//! HTTP server module
//!
//! This module handles HTTP request routing and handling:
//! - Axum router with all HLS endpoints
//! - Request handlers for playlists and segments
//! - Stream management (create, list, delete)
//! - LRU segment cache with memory limits
//! - HTTP headers (Content-Type, Cache-Control)
//! - CORS middleware

pub mod cache;
pub mod dynamic;
pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod streams;

pub use routes::create_router;
