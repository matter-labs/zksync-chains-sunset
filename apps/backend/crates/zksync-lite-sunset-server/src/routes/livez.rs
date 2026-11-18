use axum::{Json, extract::State};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct LivezResponse {
    status: &'static str,
    uptime_seconds: u64,
}

pub async fn livez(State(state): State<AppState>) -> Json<LivezResponse> {
    Json(LivezResponse {
        status: "ok",
        uptime_seconds: state.uptime_seconds(),
    })
}
