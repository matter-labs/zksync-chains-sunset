use axum::{
    Json, Router,
    extract::{Path, State},
    middleware,
    routing::{get, post},
};
use serde::Deserialize;
use zksync_lite_sunset_core::migrations::{
    lookup::{MigrationTokensResponse, list_migration_tokens},
    prepare::{MigrationPrepareRequest, MigrationPrepareResponse, prepare_migration},
    shared::{MigrationError, MigrationId},
    status::{MigrationResponse, get_migration_status},
    submit::{MigrationSubmitRequest, submit_migration},
};

use crate::state::AppState;

use super::{error::ApiError, throttle};

pub fn router(state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route(
            "/migrations/tokens",
            get(tokens).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .route(
            "/migrations/prepare",
            post(prepare).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .route(
            "/migrations",
            post(migrations).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::migration_writes,
            )),
        )
        .route(
            "/migrations/{migration_id}",
            get(migration_status).route_layer(middleware::from_fn_with_state(
                state,
                throttle::read_requests,
            )),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPrepareHttpRequest {
    pub address: String,
    pub token_address: String,
}

impl TryFrom<MigrationPrepareHttpRequest> for MigrationPrepareRequest {
    type Error = MigrationError;

    fn try_from(value: MigrationPrepareHttpRequest) -> Result<Self, Self::Error> {
        Ok(MigrationPrepareRequest {
            address: value
                .address
                .parse()
                .map_err(|_| MigrationError::InvalidAddress {
                    field: "address",
                    input: value.address,
                })?,
            token_address: value.token_address.parse().map_err(|_| {
                MigrationError::InvalidAddress {
                    field: "tokenAddress",
                    input: value.token_address,
                }
            })?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSubmitHttpRequest {
    pub migration_id: MigrationId,
    pub tx_hash: String,
}

impl TryFrom<MigrationSubmitHttpRequest> for MigrationSubmitRequest {
    type Error = MigrationError;

    fn try_from(value: MigrationSubmitHttpRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            migration_id: value.migration_id,
            tx_hash: value
                .tx_hash
                .parse()
                .map_err(|_| MigrationError::InvalidTxHash {
                    input: value.tx_hash,
                })?,
        })
    }
}

pub async fn tokens(
    State(state): State<AppState>,
) -> Result<Json<MigrationTokensResponse>, ApiError> {
    Ok(Json(list_migration_tokens(&state.world).await?))
}

pub async fn prepare(
    State(state): State<AppState>,
    Json(req): Json<MigrationPrepareHttpRequest>,
) -> Result<Json<MigrationPrepareResponse>, ApiError> {
    let req: MigrationPrepareRequest = req.try_into()?;
    Ok(Json(prepare_migration(&state.world, req).await?))
}

pub async fn migrations(
    State(state): State<AppState>,
    Json(req): Json<MigrationSubmitHttpRequest>,
) -> Result<Json<MigrationResponse>, ApiError> {
    let req: MigrationSubmitRequest = req.try_into()?;
    Ok(Json(submit_migration(&state.world, req).await?))
}

pub async fn migration_status(
    State(state): State<AppState>,
    Path(migration_id): Path<MigrationId>,
) -> Result<Json<MigrationResponse>, ApiError> {
    Ok(Json(
        get_migration_status(&state.world, &migration_id).await?,
    ))
}
