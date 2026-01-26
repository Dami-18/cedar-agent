use async_lock::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use log::{error, info};
use rocket_okapi::okapi::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationTargetKind {
    Postgres,
    Mysql,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InvalidationTarget {
    /// Unique name for this target within the cedar-agent instance.
    pub name: String,
    pub kind: InvalidationTargetKind,
    /// DSN/connection string.
    pub dsn: String,
    /// Allow disabling targets without deleting them.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

pub struct InvalidationTargetsStore {
    targets: RwLock<Vec<InvalidationTarget>>,
}

impl InvalidationTargetsStore {
    pub fn new() -> Self {
        Self {
            targets: RwLock::new(Vec::new()),
        }
    }

    pub fn new_from_env() -> Self {
        match std::env::var("CEDAR_AGENT_INVALIDATION_TARGETS") {
            Ok(raw) if !raw.trim().is_empty() => {
                match rocket::serde::json::serde_json::from_str::<Vec<InvalidationTarget>>(&raw) {
                    Ok(parsed) => {
                        info!(
                            "Loaded {} invalidation targets from CEDAR_AGENT_INVALIDATION_TARGETS",
                            parsed.len()
                        );
                        Self {
                            targets: RwLock::new(parsed),
                        }
                    }
                    Err(err) => {
                        error!(
                            "Failed to parse CEDAR_AGENT_INVALIDATION_TARGETS as JSON array: {}",
                            err
                        );
                        Self::new()
                    }
                }
            }
            _ => Self::new(),
        }
    }

    async fn read(&self) -> RwLockReadGuard<Vec<InvalidationTarget>> {
        self.targets.read().await
    }

    async fn write(&self) -> RwLockWriteGuard<Vec<InvalidationTarget>> {
        self.targets.write().await
    }

    pub async fn list(&self) -> Vec<InvalidationTarget> {
        self.read().await.clone()
    }

    pub async fn replace_all(&self, targets: Vec<InvalidationTarget>) {
        *self.write().await = targets;
    }
}

#[derive(Clone)]
pub struct InvalidationService {
    timeout: Duration,
    pg_pools: Arc<tokio::sync::Mutex<HashMap<String, sqlx::PgPool>>>,
    mysql_pools: Arc<tokio::sync::Mutex<HashMap<String, sqlx::MySqlPool>>>,
}

impl InvalidationService {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            pg_pools: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mysql_pools: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn invalidate_all(&self, targets: Vec<InvalidationTarget>) -> Result<(), String> {
        let enabled: Vec<InvalidationTarget> = targets.into_iter().filter(|t| t.enabled).collect();
        if enabled.is_empty() {
            return Ok(());
        }

        let mut join_set = JoinSet::new();
        for target in enabled {
            let svc = self.clone();
            join_set.spawn(async move { svc.invalidate_one(target).await });
        }

        let mut errors: Vec<String> = Vec::new();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => errors.push(e),
                Err(e) => errors.push(format!("invalidation task join error: {}", e)),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    async fn invalidate_one(&self, target: InvalidationTarget) -> Result<(), String> {
        match target.kind {
            InvalidationTargetKind::Postgres => self.invalidate_postgres(&target).await,
            InvalidationTargetKind::Mysql => self.invalidate_mysql(&target).await,
        }
    }

    async fn invalidate_postgres(&self, target: &InvalidationTarget) -> Result<(), String> {
        let pool = {
            let mut lock = self.pg_pools.lock().await;
            if let Some(pool) = lock.get(&target.dsn) {
                pool.clone()
            } else {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .acquire_timeout(self.timeout)
                    .connect(&target.dsn)
                    .await
                    .map_err(|e| format!("target '{}' postgres connect failed: {}", target.name, e))?;
                lock.insert(target.dsn.clone(), pool.clone());
                pool
            }
        };

        tokio::time::timeout(
            self.timeout,
            sqlx::query("SELECT pg_authorization_cache_reset();").execute(&pool),
        )
        .await
        .map_err(|_| format!("target '{}' postgres invalidate timed out", target.name))?
        .map_err(|e| format!("target '{}' postgres invalidate failed: {}", target.name, e))?;

        Ok(())
    }

    async fn invalidate_mysql(&self, target: &InvalidationTarget) -> Result<(), String> {
        let pool = {
            let mut lock = self.mysql_pools.lock().await;
            if let Some(pool) = lock.get(&target.dsn) {
                pool.clone()
            } else {
                let pool = sqlx::mysql::MySqlPoolOptions::new()
                    .max_connections(1)
                    .acquire_timeout(self.timeout)
                    .connect(&target.dsn)
                    .await
                    .map_err(|e| format!("target '{}' mysql connect failed: {}", target.name, e))?;
                lock.insert(target.dsn.clone(), pool.clone());
                pool
            }
        };

        tokio::time::timeout(
            self.timeout,
            sqlx::query("SET GLOBAL cedar_authorization_cache_flush = 1;").execute(&pool),
        )
        .await
        .map_err(|_| format!("target '{}' mysql invalidate timed out", target.name))?
        .map_err(|e| format!("target '{}' mysql invalidate failed: {}", target.name, e))?;

        Ok(())
    }
}
