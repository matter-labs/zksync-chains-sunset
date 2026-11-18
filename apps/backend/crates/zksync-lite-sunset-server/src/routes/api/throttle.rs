use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::state::{AppState, InFlightRequestPermit};

use super::error::ApiError;

pub async fn read_requests(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    run_with_permit(
        &state,
        state.try_acquire_read_request(),
        ThrottleMetric::None,
        request,
        next,
    )
    .await
}

pub async fn sponsored_claim_writes(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    run_with_permit(
        &state,
        state.try_acquire_sponsored_claim_write(),
        ThrottleMetric::SponsoredClaimWrite,
        request,
        next,
    )
    .await
}

pub async fn migration_writes(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    run_with_permit(
        &state,
        state.try_acquire_migration_write(),
        ThrottleMetric::None,
        request,
        next,
    )
    .await
}

async fn run_with_permit(
    state: &AppState,
    permit: Option<InFlightRequestPermit>,
    metric: ThrottleMetric,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let _permit = permit_or_error(state, permit, metric)?;
    Ok(next.run(request).await)
}

fn permit_or_error(
    state: &AppState,
    permit: Option<InFlightRequestPermit>,
    metric: ThrottleMetric,
) -> Result<InFlightRequestPermit, ApiError> {
    permit.ok_or_else(ApiError::throttled).inspect_err(|_| {
        if metric == ThrottleMetric::SponsoredClaimWrite {
            state.world.observability().observe_claim_write_rejection();
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThrottleMetric {
    None,
    SponsoredClaimWrite,
}
