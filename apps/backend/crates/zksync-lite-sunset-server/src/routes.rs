mod api;
mod livez;
mod metrics;
mod readyz;

use axum::{Router, middleware, routing::get};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

pub fn public_router(state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route("/healthz", get(livez::livez))
        .route("/readyz", get(readyz::readyz))
        .nest("/api", api::router(state.clone()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            metrics::http_metrics,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}

pub fn ops_router(_state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route("/livez", get(livez::livez))
        .route("/readyz", get(readyz::readyz))
        .route("/metrics", get(metrics::metrics))
}
