mod claims;
mod eligibility;
mod error;
mod frontend_config;
mod migrations;
mod throttle;

use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::state::AppState;

pub fn router(state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route(
            "/config",
            get(frontend_config::frontend_config).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .route(
            "/eligibility",
            post(eligibility::eligibility).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .merge(claims::router(state.clone()))
        .merge(migrations::router(state))
}
