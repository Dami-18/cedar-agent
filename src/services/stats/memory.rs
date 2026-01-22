use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use super::{Stats, StatsStore};

/// In-memory implementation of StatsStore using atomic counters
/// for lock-free concurrent access
pub struct MemoryStatsStore {
    total_requests: AtomicU64,
    allows: AtomicU64,
    denies: AtomicU64,
}

impl MemoryStatsStore {
    /// Create a new MemoryStatsStore with all counters initialized to zero
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            allows: AtomicU64::new(0),
            denies: AtomicU64::new(0),
        }
    }
}

impl Default for MemoryStatsStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatsStore for MemoryStatsStore {
    async fn get_stats(&self) -> Stats {
        Stats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            allows: self.allows.load(Ordering::Relaxed),
            denies: self.denies.load(Ordering::Relaxed),
        }
    }

    async fn increment_auth_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    async fn increment_allow(&self) {
        self.allows.fetch_add(1, Ordering::Relaxed);
    }

    async fn increment_deny(&self) {
        self.denies.fetch_add(1, Ordering::Relaxed);
    }

    async fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.allows.store(0, Ordering::Relaxed);
        self.denies.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initial_stats_are_zero() {
        let store = MemoryStatsStore::new();
        let stats = store.get_stats().await;
        
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.allows, 0);
        assert_eq!(stats.denies, 0);
    }

    #[tokio::test]
    async fn test_increment_auth_request() {
        let store = MemoryStatsStore::new();
        
        store.increment_auth_request().await;
        store.increment_auth_request().await;
        
        let stats = store.get_stats().await;
        assert_eq!(stats.total_requests, 2);
    }

    #[tokio::test]
    async fn test_increment_allow() {
        let store = MemoryStatsStore::new();
        
        store.increment_allow().await;
        
        let stats = store.get_stats().await;
        assert_eq!(stats.allows, 1);
    }

    #[tokio::test]
    async fn test_increment_deny() {
        let store = MemoryStatsStore::new();
        
        store.increment_deny().await;
        store.increment_deny().await;
        store.increment_deny().await;
        
        let stats = store.get_stats().await;
        assert_eq!(stats.denies, 3);
    }

    #[tokio::test]
    async fn test_reset() {
        let store = MemoryStatsStore::new();
        
        store.increment_auth_request().await;
        store.increment_allow().await;
        store.increment_deny().await;
        
        store.reset().await;
        
        let stats = store.get_stats().await;
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.allows, 0);
        assert_eq!(stats.denies, 0);
    }

    #[tokio::test]
    async fn test_concurrent_increments() {
        let store = std::sync::Arc::new(MemoryStatsStore::new());
        
        let mut handles = vec![];
        
        // Spawn 100 tasks that each increment the counter
        for _ in 0..100 {
            let store_clone = store.clone();
            let handle = tokio::spawn(async move {
                store_clone.increment_auth_request().await;
            });
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        let stats = store.get_stats().await;
        assert_eq!(stats.total_requests, 100);
    }
}
