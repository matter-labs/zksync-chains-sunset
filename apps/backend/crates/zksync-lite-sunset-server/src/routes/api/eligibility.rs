use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use zksync_lite_sunset_core::eligibility::{EligibilityRequest, EligibilityResponse};

use crate::state::AppState;

use super::error::ApiError;

pub async fn eligibility(
    State(state): State<AppState>,
    req: Result<Json<EligibilityRequest>, JsonRejection>,
) -> Result<Json<EligibilityResponse>, ApiError> {
    let Json(req) = req?;
    Ok(Json(
        zksync_lite_sunset_core::eligibility::check_eligibility(&state.world, req).await?,
    ))
}
