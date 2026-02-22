//! Integration testing module
//!
//! End-to-end tests for the HLS streaming server:
//! - Stream creation and indexing
//! - Playlist generation and validation
//! - Segment generation
//! - Audio track switching
//! - Subtitle synchronization
//! - Performance benchmarks

pub mod dts_debug;
pub mod e2e;
pub mod fixtures;
pub mod validation;
pub mod validator_debug;
