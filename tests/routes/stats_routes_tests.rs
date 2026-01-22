use crate::routes::utils::*;
use cedar_agent::services::stats::memory::MemoryStatsStore;
use cedar_agent::services::stats::StatsStore;

/// Test that initial stats are zero
#[tokio::test]
async fn test_get_stats_initial_zero() {
    let stats_store = MemoryStatsStore::new();
    let stats = stats_store.get_stats().await;

    assert_eq!(stats.total_requests, 0);
    assert_eq!(stats.allows, 0);
    assert_eq!(stats.denies, 0);
}

/// Test incrementing total requests
#[tokio::test]
async fn test_increment_total_requests() {
    let stats_store = MemoryStatsStore::new();

    stats_store.increment_auth_request().await;
    stats_store.increment_auth_request().await;
    stats_store.increment_auth_request().await;

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.total_requests, 3);
}

/// Test incrementing allows
#[tokio::test]
async fn test_increment_allows() {
    let stats_store = MemoryStatsStore::new();

    stats_store.increment_allow().await;
    stats_store.increment_allow().await;

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.allows, 2);
}

/// Test incrementing denies
#[tokio::test]
async fn test_increment_denies() {
    let stats_store = MemoryStatsStore::new();

    stats_store.increment_deny().await;

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.denies, 1);
}

/// Test mixed allow/deny breakdown
#[tokio::test]
async fn test_stats_allow_deny_breakdown() {
    let stats_store = MemoryStatsStore::new();

    // Simulate 5 requests: 3 allows, 2 denies
    for _ in 0..5 {
        stats_store.increment_auth_request().await;
    }

    stats_store.increment_allow().await;
    stats_store.increment_allow().await;
    stats_store.increment_allow().await;

    stats_store.increment_deny().await;
    stats_store.increment_deny().await;

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.total_requests, 5);
    assert_eq!(stats.allows, 3);
    assert_eq!(stats.denies, 2);
}

/// Test stats reset clears all counters
#[tokio::test]
async fn test_stats_reset() {
    let stats_store = MemoryStatsStore::new();

    // Add some stats
    stats_store.increment_auth_request().await;
    stats_store.increment_auth_request().await;
    stats_store.increment_allow().await;
    stats_store.increment_deny().await;

    // Verify they're not zero
    let stats_before = stats_store.get_stats().await;
    assert!(stats_before.total_requests > 0);
    assert!(stats_before.allows > 0);
    assert!(stats_before.denies > 0);

    // Reset
    stats_store.reset().await;

    // Verify everything is zero
    let stats_after = stats_store.get_stats().await;
    assert_eq!(stats_after.total_requests, 0);
    assert_eq!(stats_after.allows, 0);
    assert_eq!(stats_after.denies, 0);
}

/// Test stats continue incrementing after reset
#[tokio::test]
async fn test_stats_after_reset() {
    let stats_store = MemoryStatsStore::new();

    // Add stats
    stats_store.increment_auth_request().await;
    stats_store.increment_allow().await;

    // Reset
    stats_store.reset().await;

    // Add more stats
    stats_store.increment_auth_request().await;
    stats_store.increment_deny().await;

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.total_requests, 1);
    assert_eq!(stats.allows, 0);
    assert_eq!(stats.denies, 1);
}

/// Test concurrent increments are thread-safe
#[tokio::test]
async fn test_concurrent_stats_updates() {
    let stats_store = std::sync::Arc::new(MemoryStatsStore::new());
    let mut handles = vec![];

    // Spawn 50 tasks that increment requests
    for _ in 0..50 {
        let store = stats_store.clone();
        handles.push(tokio::spawn(async move {
            store.increment_auth_request().await;
        }));
    }

    // Spawn 30 tasks that increment allows
    for _ in 0..30 {
        let store = stats_store.clone();
        handles.push(tokio::spawn(async move {
            store.increment_allow().await;
        }));
    }

    // Spawn 20 tasks that increment denies
    for _ in 0..20 {
        let store = stats_store.clone();
        handles.push(tokio::spawn(async move {
            store.increment_deny().await;
        }));
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.total_requests, 50);
    assert_eq!(stats.allows, 30);
    assert_eq!(stats.denies, 20);
}

/// Test that stats accurately reflect authorization decisions
#[tokio::test]
async fn test_stats_reflect_decisions() {
    let stats_store = MemoryStatsStore::new();

    // Simulate processing 10 authorization requests
    for i in 0..10 {
        stats_store.increment_auth_request().await;

        // First 7 are allowed, last 3 are denied
        if i < 7 {
            stats_store.increment_allow().await;
        } else {
            stats_store.increment_deny().await;
        }
    }

    let stats = stats_store.get_stats().await;
    assert_eq!(stats.total_requests, 10);
    assert_eq!(stats.allows, 7);
    assert_eq!(stats.denies, 3);

    // Verify allow + deny = total (assuming all requests get a decision)
    assert_eq!(stats.allows + stats.denies, stats.total_requests);
}

/// Test multiple resets
#[tokio::test]
async fn test_multiple_resets() {
    let stats_store = MemoryStatsStore::new();

    for _ in 0..3 {
        stats_store.increment_auth_request().await;
        stats_store.reset().await;

        let stats = stats_store.get_stats().await;
        assert_eq!(stats.total_requests, 0);
    }
}
