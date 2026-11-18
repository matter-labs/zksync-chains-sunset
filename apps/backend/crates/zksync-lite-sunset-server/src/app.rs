use crate::routes;
use crate::state::AppState;
use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};
use zksync_lite_sunset_core::world::World;

pub fn app_with_state(state: AppState) -> axum::Router {
    routes::public_router(state.clone()).with_state(state)
}

pub fn ops_app_with_state(state: AppState) -> axum::Router {
    routes::ops_router(state.clone()).with_state(state)
}

pub fn app_with_world(world: World) -> axum::Router {
    app_with_state(AppState::new(world))
}

pub fn app_with_world_and_cors(world: World, cors: crate::config::CorsConfig) -> axum::Router {
    app_with_state_and_cors(AppState::new(world), cors)
}

pub fn app_pair_with_world(world: World) -> (axum::Router, axum::Router) {
    app_pair_with_state_and_cors(AppState::new(world), crate::config::CorsConfig::default())
}

pub async fn app() -> eyre::Result<axum::Router> {
    let core_config = crate::config::Config::load()?.core;
    let world = World::new(core_config).await?;
    Ok(app_with_world(world))
}

pub async fn app_pair() -> eyre::Result<(axum::Router, axum::Router)> {
    let config = crate::config::Config::load()?;
    let world = World::new(config.core).await?;
    Ok(app_pair_with_state_and_cors(
        AppState::new(world),
        config.cors,
    ))
}

pub async fn app_with_cors(cors: crate::config::CorsConfig) -> eyre::Result<axum::Router> {
    let core_config = crate::config::Config::load()?.core;
    let world = World::new(core_config).await?;
    Ok(app_with_state_and_cors(AppState::new(world), cors))
}

pub fn app_with_state_and_cors(state: AppState, cors: crate::config::CorsConfig) -> axum::Router {
    app_pair_with_state_and_cors(state, cors).0
}

pub fn app_pair_with_state_and_cors(
    state: AppState,
    cors: crate::config::CorsConfig,
) -> (axum::Router, axum::Router) {
    let ops_router = ops_app_with_state(state.clone());
    let router = routes::public_router(state.clone()).with_state(state);
    if !cors.enabled {
        return (router, ops_router);
    }

    let origins: Vec<HeaderValue> = cors
        .allowed_origins
        .iter()
        .map(|s| {
            // Config is validated at load time; this should be infallible.
            s.parse::<HeaderValue>()
                .expect("CORS origin should be a valid HeaderValue")
        })
        .collect();

    let cors_layer = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE]);

    (router.layer(cors_layer), ops_router)
}
