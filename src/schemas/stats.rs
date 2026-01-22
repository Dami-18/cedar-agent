use rocket::serde::{Deserialize, Serialize};
use rocket_okapi::okapi::schemars;
use rocket_okapi::okapi::schemars::JsonSchema;

/// Statistics response for Cedar agent
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "rocket::serde")]
pub struct StatsResponse {
    /// Total number of authorization requests processed
    pub total_requests: u64,
    /// Number of requests resulting in "Allow" decision
    pub allows: u64,
    /// Number of requests resulting in "Deny" decision  
    pub denies: u64,
}

impl From<crate::services::stats::Stats> for StatsResponse {
    fn from(stats: crate::services::stats::Stats) -> Self {
        Self {
            total_requests: stats.total_requests,
            allows: stats.allows,
            denies: stats.denies,
        }
    }
}
