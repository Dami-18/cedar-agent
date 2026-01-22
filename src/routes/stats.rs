use log::debug;
use rocket::serde::json::Json;
use rocket::{get, post, State};
use rocket::response::status;
use rocket_okapi::openapi;

use crate::authn::ApiKey;
use crate::schemas::stats::StatsResponse;
use crate::services::stats::StatsStore;

/// Get current statistics
#[openapi]
#[get("/stats")]
pub async fn get_stats(
    _auth: ApiKey,
    stats_store: &State<Box<dyn StatsStore>>,
) -> Json<StatsResponse> {
    let stats = stats_store.get_stats().await;
    debug!("Stats requested: {:?}", stats);
    Json(StatsResponse::from(stats))
}

/// Reset all statistics to zero
#[openapi]
#[post("/stats/reset")]
pub async fn reset_stats(
    _auth: ApiKey,
    stats_store: &State<Box<dyn StatsStore>>,
) -> status::NoContent {
    stats_store.reset().await;
    debug!("Stats reset");
    status::NoContent
}
