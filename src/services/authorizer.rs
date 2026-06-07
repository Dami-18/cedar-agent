use std::sync::Arc;

use cedar_policy::{Authorizer, CachedAuthorizer};
use log::{info, warn};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::serde::json::serde_json;
use rocket::{Build, Rocket};
use tokio::sync::RwLock;

use crate::services::data::DataStore;
use crate::services::policies::PolicyStore;

pub struct AuthorizerService {
    pub cache: RwLock<Option<Arc<CachedAuthorizer>>>,
    pub fallback: Authorizer,
}

impl AuthorizerService {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(None),
            fallback: Authorizer::new(),
        }
    }

    pub async fn rebuild_cache(
        &self,
        policy_store: &Box<dyn PolicyStore>,
        data_store: &Box<dyn DataStore>,
    ) {
        let policies = policy_store.policy_set().await;
        let entities = data_store.entities().await;

        let policy_str = match serde_json::to_string(&policy_store.get_policies().await) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to serialize policies for cache rebuild: {e}");
                return;
            }
        };
        let entities_str = match serde_json::to_string(&data_store.get_entities().await) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to serialize entities for cache rebuild: {e}");
                return;
            }
        };

        match CachedAuthorizer::new_with_string_hash(
            policies,
            entities,
            &policy_str,
            &entities_str,
            None,
            None,
        ) {
            Ok(cached) => {
                *self.cache.write().await = Some(Arc::new(cached));
            }
            Err(e) => {
                warn!("Failed to build cached authorizer: {e}");
            }
        }
    }
}

pub struct InitAuthorizerCacheFairing;

#[async_trait::async_trait]
impl Fairing for InitAuthorizerCacheFairing {
    fn info(&self) -> Info {
        Info {
            name: "Init Authorizer Cache",
            kind: Kind::Ignite,
        }
    }

    async fn on_ignite(&self, rocket: Rocket<Build>) -> Result<Rocket<Build>, Rocket<Build>> {
        let authorizer_service = rocket.state::<AuthorizerService>().unwrap();
        let policy_store = rocket.state::<Box<dyn PolicyStore>>().unwrap();
        let data_store = rocket.state::<Box<dyn DataStore>>().unwrap();

        authorizer_service
            .rebuild_cache(policy_store, data_store)
            .await;
        info!("Authorizer cache initialized");

        Ok(rocket)
    }
}
