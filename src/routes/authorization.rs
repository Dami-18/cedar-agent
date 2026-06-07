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
    stats_store.increment_auth_request().await;

    debug!("Received authorization request: {:?}", authorization_call);

    let query: AuthorizationRequest = match authorization_call.into_inner().try_into() {
        Ok(query) => query,
        Err(err) => {
            warn!("Invalid authorization request: {}", err);
            return Err(AgentError::BadRequest {
                reason: err.to_string(),
            });
        }
    };

    let answer = if query.is_store_backed() {
        let request = query.request().clone();
        info!("Querying cedar using cached authorizer with {:?}", &request);

        let cache = authorizer_service.cache.read().await;
        match cache.as_ref() {
            Some(cached) => cached.is_authorized(&request),
            None => {
                warn!("Cached authorizer not ready; falling back to stateless authorizer");
                let policies = policy_store.policy_set().await;
                let entities = data_store.entities().await;
                authorizer_service
                    .fallback
                    .is_authorized(&request, &policies, &entities)
            }
        }
    } else {
        let policies = policy_store.policy_set().await;
        let stored_entities = data_store.entities().await;
        let (request, entities) = match query.get_request_entities(stored_entities) {
            Ok(result) => result,
            Err(err) => {
                warn!("Failed to build request/entities: {}", err);
                return Err(AgentError::BadRequest {
                    reason: err.to_string(),
                });
            }
        };

        info!("Querying cedar using stateless authorizer with {:?}", &request);
        authorizer_service
            .fallback
            .is_authorized(&request, &policies, &entities)
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
