//! Generic in-memory TTL cache for HTTP API responses.
//!
//! Inspired by `theseus/cache.rs`.  Avoids redundant network round-trips when
//! the same project/version is queried repeatedly within a short time window
//! (e.g. while resolving a dependency tree).
//!
//! Each cached value has a configurable time-to-live after which it is
//! considered *stale*.  Callers can choose one of two behaviours:
//!
//! - **Return-stale**: serve the stale value immediately, but trigger a
//!   background re-fetch so the *next* call hits fresh data.
//! - **Return-fresh-only**: wait for a fresh value (blocking).
//!
//! In the blocking/sync world we keep it simple: a `Mutex<HashMap>` behind a
//! `LazyLock`.  The cache is process-global and lives for the entire session.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default time-to-live for cached entries.
pub const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60); // 5 minutes

/// Maximum number of entries before we start evicting oldest-first.
pub const MAX_ENTRIES: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    inserted_at: Instant,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn is_fresh(&self) -> bool {
        self.inserted_at.elapsed() < self.ttl
    }
}

#[derive(Debug, Default)]
struct CacheInner {
    entries: HashMap<String, Box<dyn std::any::Any + Send>>,
}

static CACHE: LazyLock<Mutex<CacheInner>> = LazyLock::new(|| {
    Mutex::new(CacheInner {
        entries: HashMap::new(),
    })
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Retrieves a value from the cache by key, if present and fresh.
///
/// `T` must match the type originally stored under `key`.
pub fn get<T: Clone + 'static>(key: &str) -> Option<T> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let entry = cache.entries.get(key)?;
    let entry = entry.downcast_ref::<CacheEntry<T>>()?;
    if entry.is_fresh() {
        Some(entry.value.clone())
    } else {
        None
    }
}

/// Retrieves a value from the cache by key, returning it even if stale.
///
/// Returns `None` if the key is not in the cache at all.
pub fn get_stale<T: Clone + 'static>(key: &str) -> Option<T> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let entry = cache.entries.get_mut(key)?;
    let entry = entry.downcast_mut::<CacheEntry<T>>()?;
    entry.inserted_at = Instant::now();
    Some(entry.value.clone())
}

/// Returns `true` if a fresh (non-stale) value exists for `key`.
pub fn is_fresh(key: &str) -> bool {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entries
        .get(key)
        .and_then(|e| e.downcast_ref::<CacheEntry<()>>())
        .map(|e| e.is_fresh())
        .unwrap_or(false)
}

/// Inserts a value into the cache with the default TTL.
pub fn put<T: Clone + Send + 'static>(key: impl Into<String>, value: T) {
    put_with_ttl(key, value, DEFAULT_TTL);
}

/// Inserts a value into the cache with a custom TTL.
pub fn put_with_ttl<T: Clone + Send + 'static>(key: impl Into<String>, value: T, ttl: Duration) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());

    // Evict stale entries first, then if still over capacity evict oldest.
    if cache.entries.len() >= MAX_ENTRIES {
        evict_stale(&mut cache);
    }
    if cache.entries.len() >= MAX_ENTRIES {
        evict_oldest(&mut cache);
    }

    let entry = Box::new(CacheEntry {
        value,
        inserted_at: Instant::now(),
        ttl,
    });
    cache.entries.insert(key.into(), entry);
}

/// Returns the cached value for `key` if fresh; otherwise calls `f()`,
/// stores the result, and returns it.  This is the primary "stale-while-
/// revalidate" pattern: the first caller pays the `f()` cost, subsequent
/// callers within the TTL window get the cached value for free.
pub fn get_or_insert<T, F>(key: &str, f: F) -> T
where
    T: Clone + Send + 'static,
    F: FnOnce() -> T,
{
    if let Some(v) = get::<T>(key) {
        return v;
    }
    let value = f();
    put(key, value.clone());
    value
}

/// Same as [`get_or_insert`] but with a custom TTL.
pub fn get_or_insert_with_ttl<T, F>(key: &str, ttl: Duration, f: F) -> T
where
    T: Clone + Send + 'static,
    F: FnOnce() -> T,
{
    if let Some(v) = get::<T>(key) {
        return v;
    }
    let value = f();
    put_with_ttl(key, value.clone(), ttl);
    value
}

/// Returns the cached value for `key` if present (even if stale); otherwise
/// calls `f()`, caches, and returns.  Use this when the network call is
/// expensive and you'd rather serve a slightly stale value immediately.
pub fn get_or_insert_stale<T, F>(key: &str, f: F) -> T
where
    T: Clone + Send + 'static,
    F: FnOnce() -> T,
{
    if let Some(v) = get_stale::<T>(key) {
        return v;
    }
    let value = f();
    put(key, value.clone());
    value
}

/// Removes a single entry from the cache.
pub fn invalidate(key: &str) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entries.remove(key);
}

/// Removes all entries whose keys start with `prefix`.
pub fn invalidate_prefix(prefix: &str) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entries.retain(|k, _| !k.starts_with(prefix));
}

/// Clears the entire cache.
pub fn clear() {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entries.clear();
}

/// Returns the number of entries currently in the cache.
pub fn len() -> usize {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entries.len()
}

/// Returns `true` if the cache is empty.
pub fn is_empty() -> bool {
    len() == 0
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn evict_stale(cache: &mut CacheInner) {
    // Collect keys of stale entries to avoid borrow issues.
    let stale_keys: Vec<String> = cache
        .entries
        .iter()
        .filter_map(|(k, v)| {
            let entry = v.downcast_ref::<CacheEntry<()>>()?;
            if entry.is_fresh() {
                None
            } else {
                Some(k.clone())
            }
        })
        .collect();
    for key in stale_keys {
        cache.entries.remove(&key);
    }
}

fn evict_oldest(cache: &mut CacheInner) {
    // Find the entry with the oldest `inserted_at`.
    if let Some(oldest_key) = cache
        .entries
        .iter()
        .min_by_key(|(_, v)| {
            v.downcast_ref::<CacheEntry<()>>()
                .map(|e| e.inserted_at)
                .unwrap_or(Instant::now())
        })
        .map(|(k, _)| k.clone())
    {
        cache.entries.remove(&oldest_key);
    }
}

// ---------------------------------------------------------------------------
// Typed helper for common Modrinth/CurseForge shapes
// ---------------------------------------------------------------------------

/// Builds a cache key for a Modrinth project lookup.
pub fn project_key(provider: &str, project_id: &str) -> String {
    format!("{provider}:project:{project_id}")
}

/// Builds a cache key for a Modrinth version lookup.
pub fn version_key(provider: &str, version_id: &str) -> String {
    format!("{provider}:version:{version_id}")
}

/// Builds a cache key for a Modrinth hash lookup.
pub fn hash_key(provider: &str, sha1: &str) -> String {
    format!("{provider}:hash:{sha1}")
}

/// Builds a cache key for a version list of a project.
pub fn project_versions_key(provider: &str, project_id: &str) -> String {
    format!("{provider}:project_versions:{project_id}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Process-global cache is shared; serialize these tests so clear/evict
    /// in one test cannot wipe entries another test just inserted.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    fn with_isolated_cache<R>(f: impl FnOnce() -> R) -> R {
        let _lock = TEST_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        let out = f();
        clear();
        out
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        with_isolated_cache(|| {
            assert!(get::<String>("nonexistent").is_none());
        });
    }

    #[test]
    fn put_and_get_roundtrip() {
        with_isolated_cache(|| {
            put("test_key", "hello".to_string());
            assert_eq!(get::<String>("test_key"), Some("hello".to_string()));
        });
    }

    #[test]
    fn get_or_insert_calls_fn_on_miss() {
        with_isolated_cache(|| {
            let val = get_or_insert("computed", || 42);
            assert_eq!(val, 42);
            // Second call should return cached.
            let val2 = get_or_insert("computed", || 99);
            assert_eq!(val2, 42);
        });
    }

    #[test]
    fn invalidate_removes_entry() {
        with_isolated_cache(|| {
            put("to_remove", 1);
            invalidate("to_remove");
            assert!(get::<i32>("to_remove").is_none());
        });
    }

    #[test]
    fn invalidate_prefix_removes_matching() {
        with_isolated_cache(|| {
            put("test_prefix_a:a", 1_i32);
            put("test_prefix_a:b", 2_i32);
            put("test_prefix_b:c", 3_i32);
            invalidate_prefix("test_prefix_a:");
            assert!(get::<i32>("test_prefix_a:a").is_none());
            assert!(get::<i32>("test_prefix_a:b").is_none());
            assert_eq!(get::<i32>("test_prefix_b:c"), Some(3_i32));
        });
    }

    #[test]
    fn get_stale_returns_value_even_after_ttl() {
        with_isolated_cache(|| {
            // Insert with a 0-second TTL so it's immediately stale.
            put_with_ttl("stale_key", "value".to_string(), Duration::from_secs(0));
            // `get` should return None (stale).
            assert!(get::<String>("stale_key").is_none());
            // `get_stale` should still return the value.
            assert_eq!(get_stale::<String>("stale_key"), Some("value".to_string()));
        });
    }

    #[test]
    fn cache_evicts_oldest_when_full() {
        with_isolated_cache(|| {
            for i in 0..MAX_ENTRIES + 10 {
                put(format!("key_{i}"), i);
            }
            // Should not exceed MAX_ENTRIES by much (oldest evicted).
            assert!(len() <= MAX_ENTRIES);
        });
    }

    #[test]
    fn project_key_format() {
        assert_eq!(project_key("modrinth", "abc123"), "modrinth:project:abc123");
    }
}
