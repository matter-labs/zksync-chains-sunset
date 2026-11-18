use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::Deserialize;
use std::time::Instant;
use zksync_lite_sunset_core::claims::{
    prepare::{ClaimPrepareRequest, ClaimPrepareResponse, prepare_claim},
    shared::{ClaimBatchItem, ClaimError, ClaimId},
    status::{ClaimResponse, get_claim_status},
    submit::{ClaimSubmitRequest, submit_claim},
    submit_direct::{SubmitDirectClaimRequest, submit_direct_claim},
};
use zksync_lite_sunset_core::observability::{CLAIM_RESULT_SUCCESS, claim_result_label_for_error};

use crate::state::AppState;

use super::{error::ApiError, throttle};

pub fn router(state: AppState) -> Router<AppState> {
    Router::<AppState>::new()
        .route(
            "/claims/prepare",
            post(prepare).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .route(
            "/claims",
            post(claims).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::sponsored_claim_writes,
            )),
        )
        .route(
            "/claims/direct",
            post(claims_direct).route_layer(middleware::from_fn_with_state(
                state.clone(),
                throttle::read_requests,
            )),
        )
        .route(
            "/claims/{claim_id}",
            get(claim_status).route_layer(middleware::from_fn_with_state(
                state,
                throttle::read_requests,
            )),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPrepareHttpRequest {
    pub address: String,
    #[serde(default)]
    pub token_address: Option<String>,
    #[serde(default)]
    pub token_addresses: Vec<String>,
    #[serde(default)]
    pub all_eligible: bool,
}

impl TryFrom<ClaimPrepareHttpRequest> for ClaimPrepareRequest {
    type Error = ClaimError;

    fn try_from(value: ClaimPrepareHttpRequest) -> Result<Self, Self::Error> {
        let address = value
            .address
            .parse()
            .map_err(|_| ClaimError::InvalidAddress {
                field: "address",
                input: value.address,
            })?;
        let token_address = value
            .token_address
            .map(|input| {
                input.parse().map_err(|_| ClaimError::InvalidAddress {
                    field: "tokenAddress",
                    input,
                })
            })
            .transpose()?;

        let token_addresses = value
            .token_addresses
            .into_iter()
            .map(|input| {
                input.parse().map_err(|_| ClaimError::InvalidAddress {
                    field: "tokenAddresses",
                    input,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ClaimPrepareRequest {
            address,
            token_address,
            token_addresses,
            all_eligible: value.all_eligible,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimSubmitDirectHttpRequest {
    pub claim_id: ClaimId,
    pub tx_hash: String,
}

impl TryFrom<ClaimSubmitDirectHttpRequest> for SubmitDirectClaimRequest {
    type Error = ClaimError;

    fn try_from(value: ClaimSubmitDirectHttpRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            claim_id: value.claim_id,
            tx_hash: value
                .tx_hash
                .parse()
                .map_err(|_| ClaimError::InvalidTxHash {
                    input: value.tx_hash,
                })?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimSubmitHttpRequest {
    pub address: String,
    pub items: Vec<ClaimBatchItem>,
    pub claims_hash: String,
    pub nonce: u64,
    pub expiry: u64,
    pub signature: String,
}

impl TryFrom<ClaimSubmitHttpRequest> for ClaimSubmitRequest {
    type Error = ClaimError;

    fn try_from(value: ClaimSubmitHttpRequest) -> Result<Self, Self::Error> {
        let address = value
            .address
            .parse()
            .map_err(|_| ClaimError::InvalidAddress {
                field: "address",
                input: value.address,
            })?;

        let claims_hash = value
            .claims_hash
            .parse()
            .map_err(|_| ClaimError::InvalidClaimsHash)?;

        Ok(ClaimSubmitRequest {
            address,
            items: value.items,
            claims_hash,
            nonce: value.nonce,
            expiry: value.expiry,
            signature: value.signature,
        })
    }
}

pub async fn prepare(
    State(state): State<AppState>,
    Json(req): Json<ClaimPrepareHttpRequest>,
) -> Result<Json<ClaimPrepareResponse>, ApiError> {
    let started_at = Instant::now();
    let observability = state.world.observability();
    async {
        let req: ClaimPrepareRequest = req.try_into()?;
        prepare_claim(&state.world, req).await
    }
    .await
    .inspect(|_| {
        observability.observe_claim_prepare_request(CLAIM_RESULT_SUCCESS, started_at.elapsed());
    })
    .inspect_err(|error| {
        observability.observe_claim_prepare_request(
            claim_result_label_for_error(error),
            started_at.elapsed(),
        );
    })
    .map(Json)
    .map_err(Into::into)
}

pub async fn claims(
    State(state): State<AppState>,
    Json(req): Json<ClaimSubmitHttpRequest>,
) -> Result<Json<ClaimResponse>, ApiError> {
    if !state.world.config().claims.frontend_mode.is_sponsored() {
        return Err(ApiError::public(
            StatusCode::CONFLICT,
            "SPONSORED_CLAIMS_DISABLED",
            "sponsored withdraws are disabled",
        ));
    }

    let started_at = Instant::now();
    let observability = state.world.observability();
    observability.observe_claim_items_per_request(req.items.len());

    async {
        let req: ClaimSubmitRequest = req.try_into()?;
        submit_claim(&state.world, req).await
    }
    .await
    .inspect(|_| {
        observability.observe_claim_submit_request(CLAIM_RESULT_SUCCESS, started_at.elapsed());
    })
    .inspect_err(|error| {
        observability.observe_claim_submit_request(
            claim_result_label_for_error(error),
            started_at.elapsed(),
        );
    })
    .map(Json)
    .map_err(Into::into)
}

pub async fn claims_direct(
    State(state): State<AppState>,
    Json(req): Json<ClaimSubmitDirectHttpRequest>,
) -> Result<Json<ClaimResponse>, ApiError> {
    let started_at = Instant::now();
    let observability = state.world.observability();
    async {
        let req: SubmitDirectClaimRequest = req.try_into()?;
        submit_direct_claim(&state.world, req).await
    }
    .await
    .inspect(|_| {
        observability.observe_claim_submit_request(CLAIM_RESULT_SUCCESS, started_at.elapsed());
    })
    .inspect_err(|error| {
        observability.observe_claim_submit_request(
            claim_result_label_for_error(error),
            started_at.elapsed(),
        );
    })
    .map(Json)
    .map_err(Into::into)
}

pub async fn claim_status(
    State(state): State<AppState>,
    Path(claim_id): Path<ClaimId>,
) -> Result<Json<ClaimResponse>, ApiError> {
    let started_at = Instant::now();
    let observability = state.world.observability();
    get_claim_status(&state.world, &claim_id)
        .await
        .inspect(|_| {
            observability.observe_claim_status_check(CLAIM_RESULT_SUCCESS, started_at.elapsed());
        })
        .inspect_err(|error| {
            observability.observe_claim_status_check(
                claim_result_label_for_error(error),
                started_at.elapsed(),
            );
        })
        .map(Json)
        .map_err(Into::into)
}
