//! PluginHost pooling for parallel processing.
//!
//! This module provides a thread-safe pool for reusing `PluginHost` instances,
//! reducing WASM loading overhead in parallel file processing.
//!
//! # Design
//!
//! The pool supports two modes:
//! - **Without initializer**: Hosts are created with default settings. Callers must
//!   load rules themselves. The pool reuses `PluginHost` instances but not loaded rules.
//! - **With initializer**: Hosts are pre-loaded with rules via a factory function.
//!   When hosts are returned to the pool, they retain their loaded rules for reuse.
//!
//! The second mode provides the best performance when processing multiple files
//! with the same rule set, as WASM modules remain loaded between uses.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use tsuzulint_plugin::PluginHost;

type HostInitializer = dyn Fn(&mut PluginHost) + Send + Sync;

/// Thread-safe PluginHost pool.
///
/// The pool lazily creates new `PluginHost` instances on demand and returns
/// them to the pool when dropped. This reduces the overhead of WASM runtime
/// initialization in parallel processing scenarios.
///
/// # Example (with initializer for rule reuse)
///
/// ```ignore
/// use std::sync::Arc;
/// use tsuzulint_core::pool::PluginHostPool;
///
/// let pool = Arc::new(PluginHostPool::with_initializer(|host| {
///     host.load_rule("rule.wasm").unwrap();
/// }));
///
/// // In parallel threads:
/// let host = pool.acquire();
/// // Rules are already loaded!
/// let diagnostics = host.run_rule("rule-name", &ast, &source, None)?;
/// // host is returned to pool with rules intact
/// ```
pub struct PluginHostPool {
    available: Mutex<VecDeque<PluginHost>>,
    initializer: Option<Arc<HostInitializer>>,
}

impl PluginHostPool {
    /// Creates a new empty pool without an initializer.
    ///
    /// Hosts created by this pool will have no rules pre-loaded. Callers must
    /// load rules themselves after acquiring a host.
    pub fn new() -> Self {
        Self {
            available: Mutex::new(VecDeque::new()),
            initializer: None,
        }
    }

    /// Creates a pool with a custom initializer function.
    ///
    /// The initializer is called when creating new `PluginHost` instances.
    /// This allows pre-loading WASM rules so they can be reused across
    /// multiple acquire/release cycles.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let pool = PluginHostPool::with_initializer(|host| {
    ///     host.load_rule("rules/pronoun.wasm").unwrap();
    /// });
    /// ```
    pub fn with_initializer<F>(initializer: F) -> Self
    where
        F: Fn(&mut PluginHost) + Send + Sync + 'static,
    {
        Self {
            available: Mutex::new(VecDeque::new()),
            initializer: Some(Arc::new(initializer)),
        }
    }

    /// Acquires a `PluginHost` from the pool.
    ///
    /// If no hosts are available, a new one is created (and initialized if
    /// an initializer was provided). The host is returned to the pool when
    /// the `PooledHost` is dropped, retaining any loaded rules.
    pub fn acquire(&self) -> PooledHost<'_> {
        let host = {
            let existing = self.available.lock().pop_back();
            existing.unwrap_or_else(|| {
                let mut host = PluginHost::default();
                if let Some(ref init) = self.initializer {
                    init(&mut host);
                }
                host
            })
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
        // Invariant: `host` is only taken in `Drop`, so it is always `Some` while
        // this value is accessible.
        self.host
            .as_ref()
            .unwrap_or_else(|| unreachable!("host was already taken"))
    }
}

impl std::ops::DerefMut for PooledHost<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Invariant: `host` is only taken in `Drop`, so it is always `Some` while
        // this value is accessible.
        self.host
            .as_mut()
            .unwrap_or_else(|| unreachable!("host was already taken"))
    }
}

impl Drop for PooledHost<'_> {
    fn drop(&mut self) {
        if let Some(host) = self.host.take() {
            // Note: We intentionally do NOT call `unload_all()` here.
            // Hosts retain their loaded rules so they can be reused by the
            // next caller, which is the key performance benefit of pooling.
            //
            // However, if the thread is panicking, the host may be in an
            // inconsistent state (e.g., mid-WASM execution). Discard it
            // instead of returning it to the pool.
            if !std::thread::panicking() {
                self.pool.lock().push_back(host);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    fn test_fresh_host_has_no_loaded_rules() {
        let pool = PluginHostPool::new();

        {
            let host = pool.acquire();
            assert!(host.loaded_rules().next().is_none());
        }

        let host = pool.acquire();
        assert!(
            host.loaded_rules().next().is_none(),
            "Host from pool without initializer should have no rules"
        );
    }

    #[test]
    fn test_with_initializer_calls_initializer_on_new_host() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let pool = PluginHostPool::with_initializer(move |_host| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(call_count.load(Ordering::SeqCst), 0);

        let _host1 = pool.acquire();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let _host2 = pool.acquire();
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_with_initializer_reuses_host_without_calling_initializer() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let pool = PluginHostPool::with_initializer(move |_host| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        {
            let _host = pool.acquire();
        }

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        assert_eq!(pool.available_count(), 1);

        let _host = pool.acquire();
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "Initializer should not be called when reusing pooled host"
        );
    }

    #[test]
    fn test_pool_thread_safety() {
        use std::thread;

        let pool = Arc::new(PluginHostPool::new());

        let pool1 = Arc::clone(&pool);
        let pool2 = Arc::clone(&pool);

        let h1 = thread::spawn(move || {
            let host = pool1.acquire();
            drop(host);
        });

        let h2 = thread::spawn(move || {
            let host = pool2.acquire();
            drop(host);
        });

        h1.join().unwrap();
        h2.join().unwrap();

        // Note: Due to thread scheduling, one thread might re-acquire the host
        // returned by the other, so we only verify that at least one host is
        // available and no data races occur.
        assert!(
            pool.available_count() >= 1,
            "At least one host should be available after both threads finish"
        );
    }

    #[test]
    fn test_lifo_semantics_most_recently_returned_is_reused() {
        let pool = PluginHostPool::new();

        // Acquire two hosts
        let host1 = pool.acquire();
        let host2 = pool.acquire();

        // Return them in order: host1 first, then host2
        drop(host1);
        drop(host2);

        // With LIFO, the next acquire should return host2 (most recently returned)
        // which is more likely to have warm CPU/WASM caches
        assert_eq!(pool.available_count(), 2);

        let _host = pool.acquire();
        assert_eq!(pool.available_count(), 1);
    }

    #[test]
    fn test_panic_discards_host() {
        use std::panic;

        let pool = PluginHostPool::new();

        // Acquire a host and return it normally
        {
            let _host = pool.acquire();
        }
        assert_eq!(pool.available_count(), 1);

        // Acquire a host and panic while holding it
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _host = pool.acquire();
            panic!("intentional panic for testing");
        }));

        assert!(result.is_err());

        // The host should NOT be returned to the pool due to panic
        assert_eq!(
            pool.available_count(),
            0,
            "Host should be discarded when thread is panicking"
        );
    }
}
