use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::error;

use crate::state::AppState;

pub async fn metrics(State(state): State<AppState>) -> Response {
    if let Err(err) = state
        .world
        .observability()
        .refresh_claim_backlog(state.world.db_pool())
        .await
    {
        error!(error = ?err, "failed to refresh claim backlog metrics");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to render metrics",
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.world.observability().render_text(),
    )
        .into_response()
}

pub async fn http_metrics(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| request.uri().path())
        .to_string();
    let started_at = Instant::now();
    let response = next.run(request).await;

    if route != "/metrics" {
        state.world.observability().observe_http_request(
            &route,
            response.status().as_u16(),
            started_at.elapsed(),
        );
    }

    response
}
