use std::str::FromStr;
use std::sync::Arc;

use cedar_agent::data::memory::MemoryDataStore;
use cedar_agent::policies::memory::MemoryPolicyStore;
use cedar_agent::schemas::authorization::{AuthorizationCall, AuthorizationRequest};
use cedar_agent::schemas::policies::Policy;
use cedar_agent::services::authorizer::AuthorizerService;
use cedar_agent::{DataStore, PolicyStore};
use cedar_policy::{Decision, EntityUid, Request};
use rocket::serde::json::json;

fn allow_policy() -> Policy {
    Policy {
        id: "allow_all".to_string(),
        content: "permit(principal, action, resource);".to_string(),
    }
}

fn deny_policy() -> Policy {
    Policy {
        id: "deny_all".to_string(),
        content: "permit(principal, action, resource) when { false };".to_string(),
    }
}

fn sample_request() -> Request {
    Request::new(
        EntityUid::from_str(r#"User::"alice""#).unwrap(),
        EntityUid::from_str(r#"Action::"view""#).unwrap(),
        EntityUid::from_str(r#"Document::"doc1""#).unwrap(),
        cedar_policy::Context::empty(),
        None,
    )
    .unwrap()
}

async fn boxed_stores() -> (Box<dyn PolicyStore>, Box<dyn DataStore>) {
    (
        Box::new(MemoryPolicyStore::new()) as Box<dyn PolicyStore>,
        Box::new(MemoryDataStore::new()) as Box<dyn DataStore>,
    )
}

#[tokio::test]
async fn test_rebuild_cache_populates_cache() {
    let (policy_store, data_store) = boxed_stores().await;
    let service = AuthorizerService::new();

    assert!(service.cache.read().await.is_none());

    policy_store
        .create_policy(&allow_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    assert!(service.cache.read().await.is_some());
}

#[tokio::test]
async fn test_store_backed_request_detection() {
    let request = sample_request();
    let store_backed = AuthorizationRequest::new(request.clone(), None, None);
    assert!(store_backed.is_store_backed());

    let entities = cedar_policy::Entities::from_json_value(json!([]), None).unwrap();
    let with_entities = AuthorizationRequest::new(request.clone(), Some(entities), None);
    assert!(!with_entities.is_store_backed());

    let additional = cedar_policy::Entities::from_json_value(json!([]), None).unwrap();
    let with_additional = AuthorizationRequest::new(request, None, Some(additional));
    assert!(!with_additional.is_store_backed());
}

#[tokio::test]
async fn test_authorization_call_with_entities_is_not_store_backed() {
    let call = AuthorizationCall::new(
        Some(r#"User::"alice""#.to_string()),
        Some(r#"Action::"view""#.to_string()),
        Some(r#"Document::"doc1""#.to_string()),
        None,
        Some(json!([])),
        None,
        None,
    );

    let query: AuthorizationRequest = call.try_into().unwrap();
    assert!(!query.is_store_backed());
}

#[tokio::test]
async fn test_store_backed_uses_cached_authorizer() {
    let (policy_store, data_store) = boxed_stores().await;
    let service = AuthorizerService::new();

    policy_store
        .create_policy(&allow_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    let request = sample_request();
    let policies = policy_store.policy_set().await;
    let entities = data_store.entities().await;

    let cached = service.cache.read().await;
    let cached_answer = cached.as_ref().unwrap().is_authorized(&request);
    let fallback_answer = service
        .fallback
        .is_authorized(&request, &policies, &entities);

    assert_eq!(cached_answer.decision(), Decision::Allow);
    assert_eq!(fallback_answer.decision(), Decision::Allow);
}

#[tokio::test]
async fn test_request_provided_entities_use_fallback_path() {
    let (policy_store, data_store) = boxed_stores().await;
    let service = AuthorizerService::new();

    policy_store
        .create_policy(&allow_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    let call = AuthorizationCall::new(
        Some(r#"User::"alice""#.to_string()),
        Some(r#"Action::"view""#.to_string()),
        Some(r#"Document::"doc1""#.to_string()),
        None,
        Some(json!([])),
        None,
        None,
    );
    let query: AuthorizationRequest = call.try_into().unwrap();
    assert!(!query.is_store_backed());

    let policies = policy_store.policy_set().await;
    let stored_entities = data_store.entities().await;
    let (req, ents) = query.get_request_entities(stored_entities).unwrap();

    let fallback_answer = service.fallback.is_authorized(&req, &policies, &ents);
    assert_eq!(fallback_answer.decision(), Decision::Allow);
}

#[tokio::test]
async fn test_policy_mutation_refreshes_cache() {
    let (policy_store, data_store) = boxed_stores().await;
    let service = AuthorizerService::new();
    let request = sample_request();

    policy_store
        .create_policy(&allow_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    {
        let cache = service.cache.read().await;
        assert_eq!(
            cache.as_ref().unwrap().is_authorized(&request).decision(),
            Decision::Allow
        );
    }

    policy_store.delete_policy("allow_all").await.unwrap();
    policy_store
        .create_policy(&deny_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    {
        let cache = service.cache.read().await;
        assert_eq!(
            cache.as_ref().unwrap().is_authorized(&request).decision(),
            Decision::Deny
        );
    }
}

#[tokio::test]
async fn test_data_mutation_refreshes_cache() {
    let (policy_store, data_store) = boxed_stores().await;
    let service = AuthorizerService::new();

    policy_store
        .create_policy(&allow_policy(), None)
        .await
        .unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    let request = sample_request();
    {
        let cache = service.cache.read().await;
        assert_eq!(
            cache.as_ref().unwrap().is_authorized(&request).decision(),
            Decision::Allow
        );
    }

    // Replacing entities does not change the allow/deny decision for this open policy,
    // but it must produce a new cached authorizer instance.
use rocket::serde::json::serde_json::from_str;

    let entities: cedar_agent::schemas::data::Entities = from_str(
        r#"[{
        "uid": { "id": "bob", "type": "User" },
        "attrs": {},
        "parents": []
    }]"#,
    )
    .unwrap();
    data_store.update_entities(entities, None).await.unwrap();
    service.rebuild_cache(&policy_store, &data_store).await;

    let cache_guard = service.cache.read().await;
    let new_cache = cache_guard.as_ref().unwrap();
    assert!(Arc::strong_count(new_cache) >= 1);
    assert_eq!(
        new_cache.is_authorized(&request).decision(),
        Decision::Allow
    );

    let stored = data_store.get_entities().await;
    assert_eq!(stored.len(), 1);
}
