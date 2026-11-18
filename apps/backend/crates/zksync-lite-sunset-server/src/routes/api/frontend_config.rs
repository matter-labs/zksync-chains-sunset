use axum::{Json, extract::State};
use serde::Serialize;
use zksync_lite_sunset_core::config::ClaimFrontendMode;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendConfigResponse {
    pub claim_mode: ClaimFrontendMode,
}

pub async fn frontend_config(State(state): State<AppState>) -> Json<FrontendConfigResponse> {
    Json(FrontendConfigResponse {
        claim_mode: state.world.config().claims.frontend_mode,
    })
}
