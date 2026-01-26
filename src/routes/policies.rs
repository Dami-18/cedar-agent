use std::borrow::Borrow;

use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{delete, get, post, put, State};
use rocket_okapi::openapi;

use crate::authn::ApiKey;
use crate::errors::response::AgentError;
use crate::schemas::policies as schemas;
use crate::services::invalidation::InvalidationService;
use crate::services::invalidation::InvalidationTargetsStore;
use crate::services::policies::errors::PolicyStoreError;
use crate::services::policies::PolicyStore;
use crate::services::schema::SchemaStore;
use log::{info, warn};

#[openapi]
#[get("/policies")]
pub async fn get_policies(
    _auth: ApiKey,
    policy_store: &State<Box<dyn PolicyStore>>,
) -> Result<Json<Vec<schemas::Policy>>, AgentError> {
    info!("Fetching all policies");
    Ok(Json::from(policy_store.get_policies().await))
}

#[openapi]
#[get("/policies/<id>")]
pub async fn get_policy(
    _auth: ApiKey,
    id: String,
    policy_store: &State<Box<dyn PolicyStore>>,
) -> Result<Json<schemas::Policy>, AgentError> {
    info!("Fetching policy with id='{}'", id);
    match policy_store.get_policy(id.borrow()).await {
        Ok(policy) => Ok(Json::from(policy)),
        Err(_) => Err(AgentError::NotFound {
            id,
            object: "policy",
        }),
    }
}

#[openapi]
#[post("/policies", format = "json", data = "<policy>")]
pub async fn create_policy(
    _auth: ApiKey,
    policy: Json<schemas::Policy>,
    policy_store: &State<Box<dyn PolicyStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Policy>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_policies = policy_store.get_policies().await;

    let policy = policy.into_inner();
    let schema = schema_store.get_cedar_schema().await;
    info!("Creating policy with id='{}'", policy.id);

    let created = match policy_store.create_policy(policy.borrow(), schema).await {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(policy_store_error) = e.downcast_ref::<PolicyStoreError>() {
                match policy_store_error {
                    PolicyStoreError::PolicyInvalid(_, reason) => Err(AgentError::BadRequest {
                        reason: reason.clone(),
                    }),
                    PolicyStoreError::PolicyParseError(parse_errors) => {
                        Err(AgentError::BadRequest {
                            reason: format!("Policy parsing failed: {}", parse_errors),
                        })
                    }
                    _ => Err(AgentError::BadRequest {
                        reason: format!("Policy error: {}", policy_store_error),
                    }),
                }
            } else {
                warn!("Duplicate policy detected while creating");
                Err(AgentError::Duplicate {
                    id: policy.id,
                    object: "policy",
                })
            }
        }
    }?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = policy_store
            .update_policies(prev_policies, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (policy change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(created))
}

#[openapi]
#[put("/policies", format = "json", data = "<policy>")]
pub async fn update_policies(
    _auth: ApiKey,
    policy: Json<Vec<schemas::Policy>>,
    policy_store: &State<Box<dyn PolicyStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<Vec<schemas::Policy>>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_policies = policy_store.get_policies().await;

    let schema = schema_store.get_cedar_schema().await;
    info!("Updating policies in bulk");

    let updated = match policy_store.update_policies(policy.into_inner(), schema).await {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(policy_store_error) = e.downcast_ref::<PolicyStoreError>() {
                match policy_store_error {
                    PolicyStoreError::PolicyInvalid(_, reason) => {
                        return Err(AgentError::BadRequest {
                            reason: reason.clone(),
                        });
                    }
                    PolicyStoreError::PolicyParseError(parse_errors) => {
                        return Err(AgentError::BadRequest {
                            reason: format!("Policy parsing failed: {}", parse_errors),
                        });
                    }
                    _ => {}
                }
            }
            if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                if io_err.kind() == std::io::ErrorKind::AlreadyExists {
                    warn!("Duplicate policy id found in bulk update payload");
                    // Try to extract the id from the error message: "Policy with id <id> already exists"
                    let msg = io_err.to_string();
                    let dup_id = msg
                        .strip_prefix("Policy with id ")
                        .and_then(|s| s.strip_suffix(" already exists"))
                        .unwrap_or("")
                        .to_string();
                    return Err(AgentError::Duplicate {
                        object: "policy",
                        id: dup_id,
                    });
                }
            }
            Err(AgentError::BadRequest {
                reason: e.to_string(),
            })
        }
    }?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = policy_store
            .update_policies(prev_policies, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (policy change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(updated))
}

#[openapi]
#[put("/policies/<id>", format = "json", data = "<policy>")]
pub async fn update_policy(
    _auth: ApiKey,
    id: String,
    policy: Json<schemas::PolicyUpdate>,
    policy_store: &State<Box<dyn PolicyStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Policy>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_policies = policy_store.get_policies().await;

    let schema = schema_store.get_cedar_schema().await;
    info!("Updating policy with id='{}'", id);

    let updated = match policy_store.update_policy(id, policy.into_inner(), schema).await {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(policy_store_error) = e.downcast_ref::<PolicyStoreError>() {
                match policy_store_error {
                    PolicyStoreError::PolicyInvalid(_, reason) => {
                        return Err(AgentError::BadRequest {
                            reason: reason.clone(),
                        });
                    }
                    PolicyStoreError::PolicyParseError(parse_errors) => {
                        return Err(AgentError::BadRequest {
                            reason: format!("Policy parsing failed: {}", parse_errors),
                        });
                    }
                    _ => {}
                }
            }
            Err(AgentError::BadRequest {
                reason: e.to_string(),
            })
        }
    }?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = policy_store
            .update_policies(prev_policies, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (policy change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(updated))
}

#[openapi]
#[delete("/policies/<id>")]
pub async fn delete_policy(
    _auth: ApiKey,
    id: String,
    policy_store: &State<Box<dyn PolicyStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<status::NoContent, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_policies = policy_store.get_policies().await;

    info!("Deleting policy with id='{}'", id);
    match policy_store.delete_policy(id.borrow()).await {
        Ok(_p) => Ok(status::NoContent),
        Err(_err) => Err(AgentError::NotFound {
            id,
            object: "Policy",
        }),
    }?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = policy_store
            .update_policies(prev_policies, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (policy change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(status::NoContent)
}
