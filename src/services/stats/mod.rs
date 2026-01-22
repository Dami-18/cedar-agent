use async_trait::async_trait;

pub mod memory;

/// Statistics tracked by the Cedar agent
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Total number of authorization requests processed
    pub total_requests: u64,
    /// Number of requests resulting in "Allow" decision
    pub allows: u64,
    /// Number of requests resulting in "Deny" decision
    pub denies: u64,
}

/// Trait for tracking Cedar agent statistics
#[async_trait]
pub trait StatsStore: Send + Sync {
    /// Get current statistics
    async fn get_stats(&self) -> Stats;
    
    /// Increment the total authorization request counter
    async fn increment_auth_request(&self);
    
    /// Increment the allow decision counter
    async fn increment_allow(&self);
    
    /// Increment the deny decision counter
    async fn increment_deny(&self);
    
    /// Reset all statistics to zero
    async fn reset(&self);
}
