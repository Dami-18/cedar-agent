use rocket::response::status;

use cedar_policy::EntityUid;
use rocket::serde::json::{Json, Value, *};
use rocket::{delete, get, patch, put, State};
use rocket_okapi::openapi;
use serde_json::Map;
use std::collections::HashSet;
use std::str::FromStr;

use crate::authn::ApiKey;
use crate::errors::response::AgentError;
use crate::schemas::data as schemas;
use crate::services::invalidation::InvalidationService;
use crate::services::invalidation::InvalidationTargetsStore;
use crate::services::data::DataStore;
use crate::services::schema::SchemaStore;
use crate::write_origin::WriteOrigin;
use log::{info, warn};

#[openapi]
#[get("/data")]
pub async fn get_entities(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
) -> Result<Json<schemas::Entities>, AgentError> {
    info!("Fetching all entities");
    Ok(Json::from(data_store.get_entities().await))
}

#[openapi]
#[put("/data", format = "json", data = "<entities>")]
pub async fn update_entities(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entities: Json<schemas::Entities>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entities>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let schema = schema_store.get_cedar_schema().await;
    info!("Updating entities in bulk");

    // Check for duplicate entity UIDs in the incoming payload
    let incoming = entities.into_inner();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for e in &incoming {
        if let Some(uid) = e.get().get("uid") {
            if let (Some(id), Some(typ)) = (uid.get("id"), uid.get("type")) {
                if let (Some(id_str), Some(typ_str)) = (id.as_str(), typ.as_str()) {
                    let key = (typ_str.to_string(), id_str.to_string());
                    if !seen.insert(key.clone()) {
                        warn!("Duplicate entity detected in payload: {}::{}", key.0, key.1);
                        return Err(AgentError::Duplicate {
                            object: "Entity",
                            id: format!("{}::{}", key.0, key.1),
                        });
                    }
                }
            }
        }
    }

    let updated = match data_store.update_entities(incoming, schema).await {
        Ok(entities) => entities,
        Err(err) => {
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            })
        }
    };

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
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
#[delete("/data")]
pub async fn delete_entities(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<status::NoContent, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    info!("Deleting all entities");
    data_store.delete_entities().await;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
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

#[openapi]
#[put("/data/entity", format = "json", data = "<entity>")]
pub async fn add_new_entity(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entity: Json<schemas::NewEntity>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entities>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let schema = schema_store.get_cedar_schema().await;
    let full_type = if entity.namespace.is_empty() {
        entity.entity_type.clone()
    } else {
        format!("{}::{}", entity.namespace, entity.entity_type)
    };
    info!(
        "Adding new entity: type='{}', id='{}'",
        full_type, entity.entity_id
    );

    // create new entity from a string representation of the above format
    let new_entity = schemas::Entity::from(json!({
        "uid": {
            "id": entity.entity_id,
            "type": full_type,
        },
        "attrs": {},
        "parents": []
    }));
    let new_entity = vec![new_entity];
    let existing_entities = data_store.get_entities().await;

    // check if the entity already exists
    if existing_entities.clone().into_iter().any(|e| {
        if let Some(uid) = e.get().get("uid") {
            return uid.get("id").unwrap() == &Value::String(entity.entity_id.clone())
                && uid.get("type").unwrap() == &Value::String(full_type.clone());
        }
        false
    }) {
        return Err(AgentError::Duplicate {
            object: "Entity",
            id: entity.entity_id.clone(),
        });
    }

    // add new entity to existing entities atomically
    let updated = match data_store.add_entities(new_entity.into_iter().collect(), schema).await {
        Ok(entities) => entities,
        Err(err) => {
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            })
        }
    };

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
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
#[put("/data/attribute", format = "json", data = "<entity_attribute>")]
pub async fn update_entity_attribute(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entity_attribute: Json<schemas::EntityAttributeWithValue>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entity>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let full_type = if entity_attribute.namespace.is_empty() {
        entity_attribute.entity_type.clone()
    } else {
        format!(
            "{}::{}",
            entity_attribute.namespace, entity_attribute.entity_type
        )
    };
    info!(
        "Updating attribute '{}' on entity type='{}' id='{}'",
        entity_attribute.attribute_name, full_type, entity_attribute.entity_id
    );
    let entity = data_store
        .inner()
        .get_entities()
        .await
        .into_iter()
        .find(|e| {
            let uid = e.get().get("uid");
            let id_match = uid
                .and_then(|u| u.get("id"))
                .map(|v| v == &Value::String(entity_attribute.entity_id.clone()))
                .unwrap_or(false);
            let type_match = uid
                .and_then(|u| u.get("type"))
                .map(|v| v == &Value::String(full_type.clone()))
                .unwrap_or(false);
            id_match && type_match
        });
    if entity.is_none() {
        return Err(AgentError::NotFound {
            object: "Entity",
            id: format!("{}::{}", full_type, entity_attribute.entity_id),
        });
    }

    let mut entity = entity.unwrap().clone();

    // Ensure the entity has an "attrs" object
    if entity.get().get("attrs").is_none() {
        entity
            .get_mut()
            .as_object_mut()
            .and_then(|obj| obj.insert("attrs".to_string(), Value::Object(Map::new())));
    }

    // Update the attribute value
    if let Some(attrs) = entity
        .get_mut()
        .get_mut("attrs")
        .and_then(|attr| attr.as_object_mut())
    {
        attrs.insert(
            entity_attribute.attribute_name.clone(),
            Value::String(entity_attribute.attribute_value.clone()),
        );
    }

    // Get all entities and update the specific one
    let mut entities = data_store.inner().get_entities().await;

    // Find and replace the entity with the updated one (match by id and type)
    for e in entities.iter_mut() {
        let uid = e.get().get("uid");
        let id_match = uid
            .and_then(|u| u.get("id"))
            .map(|v| v == &Value::String(entity_attribute.entity_id.clone()))
            .unwrap_or(false);
        let type_match = uid
            .and_then(|u| u.get("type"))
            .map(|v| v == &Value::String(full_type.clone()))
            .unwrap_or(false);
        if id_match && type_match {
            *e = entity.clone();
            break;
        }
    }

    // Update entities with schema validation
    data_store
        .inner()
        .update_entities(entities, schema_store.get_cedar_schema().await)
        .await
        .map_err(|err| AgentError::BadRequest {
            reason: err.to_string(),
        })?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(entity.clone()))
}

#[openapi]
#[delete("/data/attribute", format = "json", data = "<entity_attribute>")]
pub async fn delete_entity_attribute(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entity_attribute: Json<schemas::EntityAttribute>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entity>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let full_type = if entity_attribute.namespace.is_empty() {
        entity_attribute.entity_type.clone()
    } else {
        format!(
            "{}::{}",
            entity_attribute.namespace, entity_attribute.entity_type
        )
    };
    info!(
        "Deleting attribute '{}' on entity type='{}' id='{}'",
        entity_attribute.attribute_name, full_type, entity_attribute.entity_id
    );
    let entity = data_store
        .inner()
        .get_entities()
        .await
        .into_iter()
        .find(|e| {
            let uid = e.get().get("uid");
            let id_match = uid
                .and_then(|u| u.get("id"))
                .map(|v| v == &Value::String(entity_attribute.entity_id.clone()))
                .unwrap_or(false);
            let type_match = uid
                .and_then(|u| u.get("type"))
                .map(|v| v == &Value::String(full_type.clone()))
                .unwrap_or(false);
            id_match && type_match
        });
    if entity.is_none() {
        return Err(AgentError::NotFound {
            object: "Entity",
            id: format!("{}::{}", full_type, entity_attribute.entity_id),
        });
    }

    let mut entity = entity.unwrap().clone();
    let removed = entity
        .get_mut()
        .get_mut("attrs")
        .and_then(|attr| attr.as_object_mut())
        .and_then(|attr| attr.remove(&entity_attribute.attribute_name));
    if removed.is_none() {
        return Err(AgentError::NotFound {
            object: "Attribute",
            id: format!(
                "{}::{}#{}",
                entity_attribute.entity_type,
                entity_attribute.entity_id,
                entity_attribute.attribute_name
            ),
        });
    }

    let entities = data_store.inner().get_entities().await;
    let mut entities = entities.clone();
    entities.retain(|e| {
        let uid = e.get().get("uid");
        let id_match = uid
            .and_then(|u| u.get("id"))
            .map(|v| v == &Value::String(entity_attribute.entity_id.clone()))
            .unwrap_or(false);
        let type_match = uid
            .and_then(|u| u.get("type"))
            .map(|v| v == &Value::String(full_type.clone()))
            .unwrap_or(false);
        !(id_match && type_match)
    });
    entities.extend(vec![entity.clone()].into_iter());
    data_store
        .inner()
        .update_entities(entities, schema_store.get_cedar_schema().await)
        .await
        .map_err(|err| AgentError::BadRequest {
            reason: err.to_string(),
        })?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(entity.clone()))
}

#[openapi]
#[patch("/data/entity/attributes", format = "json", data = "<update_request>")]
pub async fn patch_entity_attributes(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    update_request: Json<schemas::UpdateEntityAttributes>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entity>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let full_type = if update_request.namespace.is_empty() {
        update_request.entity_type.clone()
    } else {
        format!(
            "{}::{}",
            update_request.namespace, update_request.entity_type
        )
    };
    info!(
        "Patching attributes on entity type='{}' id='{}'",
        full_type, update_request.entity_id
    );

    // Find the entity
    let entity = data_store
        .inner()
        .get_entities()
        .await
        .into_iter()
        .find(|e| {
            let uid = e.get().get("uid");
            let id_match = uid
                .and_then(|u| u.get("id"))
                .map(|v| v == &Value::String(update_request.entity_id.clone()))
                .unwrap_or(false);
            let type_match = uid
                .and_then(|u| u.get("type"))
                .map(|v| v == &Value::String(full_type.clone()))
                .unwrap_or(false);
            id_match && type_match
        });

    if entity.is_none() {
        return Err(AgentError::NotFound {
            object: "Entity",
            id: format!("{}::{}", full_type, update_request.entity_id),
        });
    }

    let mut entity = entity.unwrap().clone();

    // Ensure the entity has an "attrs" object
    if entity.get().get("attrs").is_none() {
        entity
            .get_mut()
            .as_object_mut()
            .and_then(|obj| obj.insert("attrs".to_string(), Value::Object(Map::new())));
    }

    // Update all attributes from the request
    if let Some(attrs) = entity
        .get_mut()
        .get_mut("attrs")
        .and_then(|attr| attr.as_object_mut())
    {
        for (attr_name, attr_value) in &update_request.attributes {
            attrs.insert(attr_name.clone(), Value::String(attr_value.clone()));
        }
    }

    // Update parents if provided
    if let Some(new_parents) = &update_request.parents {
        let parents_array: Vec<Value> = new_parents
            .iter()
            .map(|parent_map| {
                let mut parent_obj = Map::new();
                for (k, v) in parent_map {
                    parent_obj.insert(k.clone(), v.clone());
                }
                Value::Object(parent_obj)
            })
            .collect();
        entity
            .get_mut()
            .as_object_mut()
            .and_then(|obj| obj.insert("parents".to_string(), Value::Array(parents_array)));
    }

    // Get all entities and update the specific one
    let mut entities = data_store.inner().get_entities().await;

    // Find and replace the entity with the updated one (match by id and type)
    for e in entities.iter_mut() {
        let uid = e.get().get("uid");
        let id_match = uid
            .and_then(|u| u.get("id"))
            .map(|v| v == &Value::String(update_request.entity_id.clone()))
            .unwrap_or(false);
        let type_match = uid
            .and_then(|u| u.get("type"))
            .map(|v| v == &Value::String(full_type.clone()))
            .unwrap_or(false);
        if id_match && type_match {
            *e = entity.clone();
            break;
        }
    }

    // Update entities with schema validation (no duplicate check - this is for updating existing entities)
    data_store
        .inner()
        .update_entities(entities, schema_store.get_cedar_schema().await)
        .await
        .map_err(|err| AgentError::BadRequest {
            reason: err.to_string(),
        })?;

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
                    err
                ),
                Err(rerr) => format!(
                    "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                    err, rerr
                ),
            },
        });
    }

    Ok(Json::from(entity.clone()))
}

/*

*/
#[openapi]
#[put("/data/single", format = "json", data = "<entities>")]
pub async fn add_single_data_entry(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entities: Json<schemas::Entities>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
) -> Result<Json<schemas::Entities>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;

    let schema = schema_store.get_cedar_schema().await;
    info!("Adding a single entity entry");
    if entities.len() != 1 {
        return Err(AgentError::BadRequest {
            reason: "Exactly one entity is required".to_string(),
        });
    }
    let new_entity = entities.into_inner().into_iter().last().unwrap();
    let existing_entities = data_store.get_entities().await;

    // check duplicate
    if let Some(uid) = new_entity.get().get("uid") {
        if let (Some(id), Some(typ)) = (uid.get("id"), uid.get("type")) {
            if let (Some(id_str), Some(typ_str)) = (id.as_str(), typ.as_str()) {
                if existing_entities.clone().into_iter().any(|e| {
                    if let Some(euid) = e.get().get("uid") {
                        return euid.get("id") == Some(&Value::String(id_str.to_string()))
                            && euid.get("type") == Some(&Value::String(typ_str.to_string()));
                    }
                    false
                }) {
                    warn!(
                        "Duplicate entity detected when adding single entry: {}::{}",
                        typ_str, id_str
                    );
                    return Err(AgentError::Duplicate {
                        object: "Entity",
                        id: format!("{}::{}", typ_str, id_str),
                    });
                }
            }
        }
    }

    // add new entities to existing entities atomically
    let updated = match data_store.add_entities(vec![new_entity].into_iter().collect(), schema).await {
        Ok(entities) => entities,
        Err(err) => {
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            })
        }
    };

    let targets = targets_store.list().await;
    if let Err(err) = invalidation.invalidate_all(targets).await {
        let rollback_schema = schema_store.get_cedar_schema().await;
        let rollback_res = data_store
            .update_entities(prev_entities, rollback_schema)
            .await
            .map_err(|e| e.to_string());

        return Err(AgentError::BadRequest {
            reason: match rollback_res {
                Ok(_) => format!(
                    "Authorization cache invalidation failed (data change rolled back): {}",
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
#[put("/data/single/<entity_id>", format = "json", data = "<entities>")]
pub async fn update_single_data_entry(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entity_id: String,
    entities: Json<schemas::Entities>,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
    origin: WriteOrigin,
) -> Result<Json<schemas::Entity>, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;
    let skip_invalidation = origin.is_db_entity_sync();

    info!("Updating single data entry with id: {}", entity_id);
    let new_entity = if entities.len() == 1 {
        entities.into_inner().into_iter().next().unwrap()
    } else {
        warn!("Validation failed: Exactly one entity is required, received {}", entities.len());
        return Err(AgentError::BadRequest {
            reason: "Exactly one entity is required".to_string(),
        });
    };
    let schema = schema_store.get_cedar_schema().await;
    info!("Received entity for update: {:#?}", new_entity);
    // Ensure the provided entity's uid.id or full UID matches the path id
    if let Some(uid) = new_entity.get().get("uid") {
        let payload_id = uid.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let payload_type = uid.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let full_payload_uid = format!("{}::\"{}\"", payload_type, payload_id);

        let mut matched = payload_id == entity_id || full_payload_uid == entity_id;

        if !matched {
            // Try robust comparison by parsing path entity_id as a Cedar UID
            if let Ok(path_uid) = EntityUid::from_str(&entity_id) {
                if path_uid.to_string() == full_payload_uid {
                    matched = true;
                }
            }
        }

        if !matched {
            warn!("Validation failed: Entity id mismatch. Path: {}, Payload: {} (or {})", entity_id, payload_id, full_payload_uid);
            return Err(AgentError::BadRequest {
                reason: format!(
                    "Entity id/UID in payload ('{}' / '{}') does not match path id ('{}')",
                    payload_id, full_payload_uid, entity_id
                ),
            });
        }
    }
    let existing_entities = data_store.get_entities().await;

    // Check if entity already exists - if so, just return success (idempotent)
    if let Some(uid) = new_entity.get().get("uid") {
        if let (Some(id), Some(typ)) = (uid.get("id"), uid.get("type")) {
            if let (Some(id_str), Some(typ_str)) = (id.as_str(), typ.as_str()) {
                if existing_entities.clone().into_iter().any(|e| {
                    if let Some(euid) = e.get().get("uid") {
                        return euid.get("id") == Some(&Value::String(id_str.to_string()))
                            && euid.get("type") == Some(&Value::String(typ_str.to_string()));
                    }
                    false
                }) {
                    info!(
                        "Entity already exists, returning success (idempotent): {}::{}",
                        typ_str, id_str
                    );
                    return Ok(Json::from(new_entity));
                }
            }
        }
    }

    // Entity doesn't exist, add it as new atomically
    info!("Creating new entity: {:#?}", new_entity);

    // Persist the new entity to the data store atomically
    data_store
        .add_entities(vec![new_entity.clone()].into_iter().collect(), schema)
        .await
        .map_err(|err| AgentError::BadRequest {
            reason: err.to_string(),
        })?;

    info!("Successfully added entity: {:?}", new_entity.get().get("uid"));

    if !skip_invalidation {
        let targets = targets_store.list().await;
        if let Err(err) = invalidation.invalidate_all(targets).await {
            let rollback_schema = schema_store.get_cedar_schema().await;
            let rollback_res = data_store
                .update_entities(prev_entities, rollback_schema)
                .await
                .map_err(|e| e.to_string());

            return Err(AgentError::BadRequest {
                reason: match rollback_res {
                    Ok(_) => format!(
                        "Authorization cache invalidation failed (data change rolled back): {}",
                        err
                    ),
                    Err(rerr) => format!(
                        "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                        err, rerr
                    ),
                },
            });
        }
    }

    Ok(Json::from(new_entity))
}

#[openapi]
#[delete("/data/single/<entity_id>", format = "json")]
pub async fn delete_single_data_entry(
    _auth: ApiKey,
    data_store: &State<Box<dyn DataStore>>,
    schema_store: &State<Box<dyn SchemaStore>>,
    entity_id: String,
    targets_store: &State<InvalidationTargetsStore>,
    invalidation: &State<InvalidationService>,
    mutation_lock: &State<tokio::sync::Mutex<()>>,
    origin: WriteOrigin,
) -> Result<status::NoContent, AgentError> {
    let _guard = mutation_lock.lock().await;
    let prev_entities = data_store.get_entities().await;
    let skip_invalidation = origin.is_db_entity_sync();

    let schema = schema_store.get_cedar_schema().await;
    let existing_entities = prev_entities.clone();
    info!("Deleting single entity with id: {}", entity_id);
    let original_len = existing_entities.len();
    let mut entities = existing_entities.clone();
    entities.retain(|e| {
        if let Some(uid) = e.get().get("uid") {
            let payload_id = uid.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let payload_type = uid.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let full_payload_uid = format!("{}::\"{}\"", payload_type, payload_id);

            if entity_id == payload_id || entity_id == full_payload_uid {
                return false;
            }

            // Try robust comparison by parsing path entity_id as a Cedar UID
            if let Ok(path_uid) = EntityUid::from_str(&entity_id) {
                if path_uid.to_string() == full_payload_uid {
                    return false;
                }
            }
        }
        true
    });
    if entities.len() == original_len {
        return Err(AgentError::NotFound {
            object: "Entity",
            id: entity_id,
        });
    }

    match data_store.update_entities(entities, schema).await {
        Ok(_) => {}
        Err(err) => {
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            })
        }
    }

    if !skip_invalidation {
        let targets = targets_store.list().await;
        if let Err(err) = invalidation.invalidate_all(targets).await {
            let rollback_schema = schema_store.get_cedar_schema().await;
            let rollback_res = data_store
                .update_entities(prev_entities, rollback_schema)
                .await
                .map_err(|e| e.to_string());

            return Err(AgentError::BadRequest {
                reason: match rollback_res {
                    Ok(_) => format!(
                        "Authorization cache invalidation failed (data change rolled back): {}",
                        err
                    ),
                    Err(rerr) => format!(
                        "Authorization cache invalidation failed and rollback failed: {} (rollback error: {})",
                        err, rerr
                    ),
                },
            });
        }
    }

    Ok(status::NoContent)
}
