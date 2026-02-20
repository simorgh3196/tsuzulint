//! PluginHost pooling for parallel processing.
//!
//! This module provides a thread-safe pool for reusing `PluginHost` instances,
//! reducing WASM loading overhead in parallel file processing.

use std::collections::VecDeque;

use parking_lot::Mutex;
use tsuzulint_plugin::PluginHost;

/// Thread-safe PluginHost pool.
///
/// The pool lazily creates new `PluginHost` instances on demand and returns
/// them to the pool when dropped. This reduces the overhead of WASM runtime
/// initialization in parallel processing scenarios.
///
/// # Example
///
/// ```ignore
/// use std::sync::Arc;
/// use tsuzulint_core::pool::PluginHostPool;
///
/// let pool = Arc::new(PluginHostPool::new());
///
/// // In parallel threads:
/// let mut host = pool.acquire();
/// host.load_rule("rule.wasm")?;
/// let diagnostics = host.run_rule("rule-name", &ast, &source, None)?;
/// // host is returned to pool when dropped
/// ```
pub struct PluginHostPool {
    available: Mutex<VecDeque<PluginHost>>,
}

impl PluginHostPool {
    /// Creates a new empty pool.
    pub fn new() -> Self {
        Self {
            available: Mutex::new(VecDeque::new()),
        }
    }

    /// Acquires a `PluginHost` from the pool.
    ///
    /// If no hosts are available, a new one is created.
    /// The host is returned to the pool when the `PooledHost` is dropped.
    pub fn acquire(&self) -> PooledHost<'_> {
        let host = {
            let existing = self.available.lock().pop_front();
            existing.unwrap_or_default()
        };
        PooledHost {
            host: Some(host),
            pool: &self.available,
        }
    }

    /// Returns the number of available hosts in the pool.
    pub fn available_count(&self) -> usize {
        self.available.lock().len()
    }

    /// Clears all hosts from the pool.
    pub fn clear(&self) {
        self.available.lock().clear();
    }
}

impl Default for PluginHostPool {
    fn default() -> Self {
        Self::new()
    }
}

/// A RAII guard that returns the `PluginHost` to the pool on drop.
pub struct PooledHost<'a> {
    host: Option<PluginHost>,
    pool: &'a Mutex<VecDeque<PluginHost>>,
}

impl std::ops::Deref for PooledHost<'_> {
    type Target = PluginHost;

    fn deref(&self) -> &Self::Target {
        self.host.as_ref().expect("host was already taken")
    }
}

impl std::ops::DerefMut for PooledHost<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.host.as_mut().expect("host was already taken")
    }
}

impl Drop for PooledHost<'_> {
    fn drop(&mut self) {
        if let Some(mut host) = self.host.take() {
            host.unload_all();
            self.pool.lock().push_back(host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_new_is_empty() {
        let pool = PluginHostPool::new();
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_acquire_creates_new_host_when_empty() {
        let pool = PluginHostPool::new();
        let host = pool.acquire();
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    fn test_host_returned_to_pool_on_drop() {
        let pool = PluginHostPool::new();
        assert_eq!(pool.available_count(), 0);

        {
            let _host = pool.acquire();
        }

        assert_eq!(pool.available_count(), 1);
    }

    #[test]
    fn test_reuse_host_from_pool() {
        let pool = PluginHostPool::new();

        {
            let _host = pool.acquire();
        }

        assert_eq!(pool.available_count(), 1);

        let _host = pool.acquire();
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_clear_pool() {
        let pool = PluginHostPool::new();

        {
            let _host1 = pool.acquire();
            let _host2 = pool.acquire();
        }

        assert_eq!(pool.available_count(), 2);
        pool.clear();
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_default_creates_empty_pool() {
        let pool = PluginHostPool::default();
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_multiple_hosts_can_be_acquired() {
        let pool = PluginHostPool::new();

        {
            let _h1 = pool.acquire();
            let _h2 = pool.acquire();
            let _h3 = pool.acquire();
        }

        assert_eq!(pool.available_count(), 3);
    }

    #[test]
    fn test_host_is_reset_before_returning_to_pool() {
        let pool = PluginHostPool::new();

        {
            let host = pool.acquire();
            assert!(host.loaded_rules().next().is_none());
        }

        let host = pool.acquire();
        assert!(
            host.loaded_rules().next().is_none(),
            "Host should be clean after being returned to pool"
        );
    }

    #[test]
    fn test_pool_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let pool = Arc::new(PluginHostPool::new());

        let pool1 = Arc::clone(&pool);
        let pool2 = Arc::clone(&pool);

        let h1 = thread::spawn(move || {
            let host = pool1.acquire();
            drop(host);
            pool1.available_count()
        });

        let h2 = thread::spawn(move || {
            let host = pool2.acquire();
            drop(host);
            pool2.available_count()
        });

        let r1 = h1.join().unwrap();
        let r2 = h2.join().unwrap();

        assert!(r1 + r2 >= 1, "At least one host should be returned");
    }
}
