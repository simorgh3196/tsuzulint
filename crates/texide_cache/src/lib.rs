//! # texide_cache
//!
//! Caching system for Texide.
//!
//! This crate provides efficient caching to avoid re-linting unchanged files.
//!
//! ## Cache Strategy
//!
//! 1. **File-level cache**: Skip files whose content hash hasn't changed
//! 2. **Config-aware**: Invalidate when rule configuration changes
//! 3. **Rule-version tracking**: Invalidate when rule WASM changes
//!
//! ## Storage
//!
//! Cache is stored using `rkyv` for zero-copy deserialization,
//! providing fast cache reads without parsing overhead.

mod entry;
mod error;
mod manager;

pub use entry::CacheEntry;
pub use error::CacheError;
pub use manager::CacheManager;
