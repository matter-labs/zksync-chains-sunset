use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use zksync_lite_sunset_core::health::{
    DependencyHealth, DependencyHealthStatus, ReadinessReport, check_readiness,
};

use crate::state::AppState;

#[derive(Serialize)]
pub struct ReadyzResponse {
    status: &'static str,
    checks: ReadyzChecks,
}

#[derive(Serialize)]
pub struct ReadyzChecks {
    database: DependencyCheck,
    rpc: DependencyCheck,
}

#[derive(Serialize)]
pub struct DependencyCheck {
    status: &'static str,
    latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_number: Option<u64>,
}

pub async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<ReadyzResponse>) {
    let readiness = check_readiness(&state.world).await;
    let status = if readiness.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(readyz_response(readiness)))
}

fn readyz_response(report: ReadinessReport) -> ReadyzResponse {
    ReadyzResponse {
        status: if report.is_ready() { "ok" } else { "error" },
        checks: ReadyzChecks {
            database: dependency_check(report.database),
            rpc: dependency_check(report.execution_rpc),
        },
    }
}

fn dependency_check(health: DependencyHealth) -> DependencyCheck {
    DependencyCheck {
        status: match health.status {
            DependencyHealthStatus::Ok => "ok",
            DependencyHealthStatus::Error => "error",
        },
        latency_ms: health.latency_ms,
        detail: health.detail,
        block_number: health.block_number,
    }
}
