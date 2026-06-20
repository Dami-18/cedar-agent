use std::time::Instant;

use log::{debug, info, warn};

use rocket::serde::json::Json;
use rocket::{post, State};
use rocket_okapi::openapi;

use crate::authn::ApiKey;
use crate::errors::response::AgentError;
use crate::schemas::authorization::{AuthorizationAnswer, AuthorizationCall, AuthorizationRequest};
use crate::services::authorizer::AuthorizerService;
use crate::services::data::DataStore;
use crate::services::policies::PolicyStore;
use crate::services::stats::StatsStore;

/// Standard Cedar authorization using the stateless `Authorizer`.
///
/// Fetches the current policy set and entity store on every call, then
/// evaluates the request directly. Use this as your baseline in benchmarks.
#[openapi]
#[post("/is_authorized", format = "json", data = "<authorization_call>")]
pub async fn is_authorized(
    _auth: ApiKey,
    policy_store: &State<Box<dyn PolicyStore>>,
    data_store: &State<Box<dyn DataStore>>,
    stats_store: &State<Box<dyn StatsStore>>,
    authorizer_service: &State<AuthorizerService>,
    authorization_call: Json<AuthorizationCall>,
) -> Result<Json<AuthorizationAnswer>, AgentError> {
    let total_start = Instant::now();

    stats_store.increment_auth_request().await;

    debug!("Received authorization request (stateless): {:?}", authorization_call);

    let query: AuthorizationRequest = match authorization_call.into_inner().try_into() {
        Ok(query) => query,
        Err(err) => {
            warn!("Invalid authorization request: {}", err);
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            });
        }
    };

    let policies = policy_store.policy_set().await;
    let stored_entities = data_store.entities().await;

    let (request, entities) = if query.is_store_backed() {
        // No inline entities in the call — use the store directly.
        (query.request().clone(), stored_entities)
    } else {
        match query.get_request_entities(stored_entities) {
            Ok(result) => result,
            Err(err) => {
                warn!("Failed to build request/entities: {}", err);
                return Err(AgentError::BadRequest {
                    reason: err.to_string(),
                });
            }
        }
    };

    info!("Querying cedar using stateless authorizer with {:?}", &request);
    let eval_start = Instant::now();
    let answer = authorizer_service
        .fallback
        .is_authorized(&request, &policies, &entities);
    info!(
        "[STATELESS] Authorization evaluated in {:.3}ms (total: {:.3}ms)",
        eval_start.elapsed().as_secs_f64() * 1000.0,
        total_start.elapsed().as_secs_f64() * 1000.0
    );

    debug!("Authorization answer: {:?}", answer);

    match answer.decision() {
        cedar_policy::Decision::Allow => {
            stats_store.increment_allow().await;
        }
        cedar_policy::Decision::Deny => {
            stats_store.increment_deny().await;
        }
    }

    Ok(Json::from(AuthorizationAnswer::from(answer)))
}

/// Poltree-cached Cedar authorization using the `CachedAuthorizer`.
///
/// Always evaluates via the `CachedAuthorizer` — never falls back to the
/// stateless authorizer. If the cache is cold on arrival (e.g. the first
/// request after startup before the fairing has run), the poltree index is
/// built on the spot from the current stores, written into the shared cache,
/// and then used immediately. Subsequent requests will find the cache warm.
///
/// Inline `entities` / `additional_entities` fields in the request body are
/// ignored; the cache is store-backed only.
///
/// Use this endpoint alongside `POST /is_authorized` to benchmark the
/// poltree index against the plain Cedar authorizer.
#[openapi]
#[post("/is_authorized/poltree", format = "json", data = "<authorization_call>")]
pub async fn is_authorized_poltree(
    _auth: ApiKey,
    policy_store: &State<Box<dyn PolicyStore>>,
    data_store: &State<Box<dyn DataStore>>,
    stats_store: &State<Box<dyn StatsStore>>,
    authorizer_service: &State<AuthorizerService>,
    authorization_call: Json<AuthorizationCall>,
) -> Result<Json<AuthorizationAnswer>, AgentError> {
    let total_start = Instant::now();

    stats_store.increment_auth_request().await;

    debug!("Received authorization request (poltree): {:?}", authorization_call);

    let query: AuthorizationRequest = match authorization_call.into_inner().try_into() {
        Ok(query) => query,
        Err(err) => {
            warn!("Invalid authorization request: {}", err);
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            });
        }
    };

    let request = query.request().clone();

    // Take a quick read-lock first (cheap path: cache already built).
    // If it's cold, drop the read-lock, rebuild via the existing service
    // method (which takes a write-lock internally), then re-acquire a fresh
    // read-lock. This way the slow build only happens once even under
    // concurrent requests — the second writer will just overwrite with an
    // equivalent cache.
    {
        let read = authorizer_service.cache.read().await;
        if read.is_none() {
            drop(read);
            warn!("Poltree cache is cold — building now before evaluating");
            let build_start = Instant::now();
            authorizer_service
                .rebuild_cache(policy_store.inner(), data_store.inner())
                .await;
            info!(
                "[POLTREE-CACHE-BUILD] Cache built in {:.3}ms",
                build_start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    let cache = authorizer_service.cache.read().await;
    let eval_start = Instant::now();

    let answer = match cache.as_ref() {
        Some(cached) => {
            info!("Querying cedar using poltree cached authorizer with {:?}", &request);
            let answer = cached.is_authorized(&request);
            info!(
                "[POLTREE-CACHED] Authorization evaluated in {:.3}ms (total: {:.3}ms)",
                eval_start.elapsed().as_secs_f64() * 1000.0,
                total_start.elapsed().as_secs_f64() * 1000.0
            );
            answer
        }
        // rebuild_cache logs a warning internally if CachedAuthorizer::new fails
        // (e.g. policy parse error). Return a clear error rather than silently
        // degrading to stateless — the caller should know poltree wasn't used.
        None => {
            return Err(AgentError::BadRequest {
                reason: "Failed to build poltree cache — check server logs for details"
                    .to_string(),
            });
        }
    };

    debug!("Authorization answer: {:?}", answer);

    match answer.decision() {
        cedar_policy::Decision::Allow => {
            stats_store.increment_allow().await;
        }
        cedar_policy::Decision::Deny => {
            stats_store.increment_deny().await;
        }
    }

    Ok(Json::from(AuthorizationAnswer::from(answer)))
}