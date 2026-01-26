use rocket::serde::json::Json;
use rocket::{get, put, State};
use rocket_okapi::openapi;

use crate::authn::ApiKey;
use crate::errors::response::AgentError;
use crate::services::invalidation::InvalidationTarget;
use crate::services::invalidation::InvalidationTargetsStore;
use log::info;

#[openapi]
#[get("/invalidation/targets")]
pub async fn get_invalidation_targets(
    _auth: ApiKey,
    targets: &State<InvalidationTargetsStore>,
) -> Result<Json<Vec<InvalidationTarget>>, AgentError> {
    info!("Fetching invalidation targets");
    Ok(Json::from(targets.list().await))
}

#[openapi]
#[put("/invalidation/targets", format = "json", data = "<new_targets>")]
pub async fn put_invalidation_targets(
    _auth: ApiKey,
    targets: &State<InvalidationTargetsStore>,
    new_targets: Json<Vec<InvalidationTarget>>,
) -> Result<Json<Vec<InvalidationTarget>>, AgentError> {
    info!("Replacing invalidation targets");
    let list = new_targets.into_inner();

    // Basic validation: unique names and non-empty dsns.
    let mut names = std::collections::HashSet::new();
    for t in &list {
        if t.name.trim().is_empty() {
            return Err(AgentError::BadRequest {
                reason: "Invalidation target name must be non-empty".to_string(),
            });
        }
        if t.dsn.trim().is_empty() {
            return Err(AgentError::BadRequest {
                reason: format!("Invalidation target '{}' dsn must be non-empty", t.name),
            });
        }
        if !names.insert(t.name.clone()) {
            return Err(AgentError::Duplicate {
                object: "InvalidationTarget",
                id: t.name.clone(),
            });
        }
    }

    targets.replace_all(list.clone()).await;
    Ok(Json::from(list))
}
