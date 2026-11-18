use crate::world::World;
use alloy::providers::{Provider, RootProvider};
use sqlx::Row;
use std::time::Instant;
use tokio::time::{Duration, timeout};

const READINESS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessReport {
    pub database: DependencyHealth,
    pub execution_rpc: DependencyHealth,
}

impl ReadinessReport {
    pub fn is_ready(&self) -> bool {
        self.database.is_ok() && self.execution_rpc.is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyHealth {
    pub status: DependencyHealthStatus,
    pub latency_ms: u64,
    pub detail: Option<String>,
    pub block_number: Option<u64>,
}

impl DependencyHealth {
    pub fn is_ok(&self) -> bool {
        self.status == DependencyHealthStatus::Ok
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyHealthStatus {
    Ok,
    Error,
}

pub async fn check_readiness(world: &World) -> ReadinessReport {
    let (database, execution_rpc) = tokio::join!(
        check_database_health(world),
        check_execution_rpc_health(world)
    );

    ReadinessReport {
        database,
        execution_rpc,
    }
}

async fn check_database_health(world: &World) -> DependencyHealth {
    let start = Instant::now();
    let query = sqlx::query("SELECT 1 AS ready").fetch_one(world.db_pool());
    match timeout(READINESS_TIMEOUT, query).await {
        Ok(Ok(row)) => {
            let ready: i32 = row.get("ready");
            if ready == 1 {
                ok_health(start.elapsed().as_millis() as u64)
            } else {
                error_health(
                    start.elapsed().as_millis() as u64,
                    format!("unexpected database readiness value: {ready}"),
                )
            }
        }
        Ok(Err(err)) => error_health(start.elapsed().as_millis() as u64, err.to_string()),
        Err(_) => error_health(
            start.elapsed().as_millis() as u64,
            format!(
                "database readiness check timed out after {}s",
                READINESS_TIMEOUT.as_secs()
            ),
        ),
    }
}

async fn check_execution_rpc_health(world: &World) -> DependencyHealth {
    let rpc = check_provider_health(world.provider()).await;
    if rpc.is_ok() {
        return DependencyHealth {
            status: DependencyHealthStatus::Ok,
            latency_ms: rpc.latency_ms,
            detail: None,
            block_number: rpc.block_number,
        };
    }

    error_health(
        rpc.latency_ms,
        format!(
            "execution rpc: {}",
            rpc.detail.unwrap_or_else(|| "unknown error".to_string())
        ),
    )
}

async fn check_provider_health(provider: &RootProvider) -> DependencyHealth {
    let start = Instant::now();
    match timeout(READINESS_TIMEOUT, provider.get_block_number()).await {
        Ok(Ok(block_number)) => DependencyHealth {
            block_number: Some(block_number),
            ..ok_health(start.elapsed().as_millis() as u64)
        },
        Ok(Err(err)) => error_health(start.elapsed().as_millis() as u64, err.to_string()),
        Err(_) => error_health(
            start.elapsed().as_millis() as u64,
            format!(
                "rpc readiness check timed out after {}s",
                READINESS_TIMEOUT.as_secs()
            ),
        ),
    }
}

fn ok_health(latency_ms: u64) -> DependencyHealth {
    DependencyHealth {
        status: DependencyHealthStatus::Ok,
        latency_ms,
        detail: None,
        block_number: None,
    }
}

fn error_health(latency_ms: u64, detail: String) -> DependencyHealth {
    DependencyHealth {
        status: DependencyHealthStatus::Error,
        latency_ms,
        detail: Some(detail),
        block_number: None,
    }
}
