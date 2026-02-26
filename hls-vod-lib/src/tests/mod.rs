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
pub mod fixtures;
pub mod test_context_reuse;
pub mod test_send;
pub mod validation;
pub mod validator_debug;
pub mod e2e;
