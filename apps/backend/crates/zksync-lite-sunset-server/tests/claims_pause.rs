use alloy::primitives::Address;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use eyre::{Result, WrapErr as _, ensure};
use http_body_util::BodyExt as _;
use serde_json::json;
use serial_test::serial;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt as _;
use zksync_lite_sunset_core::{claims::worker as claim_worker, world::World};
use zksync_lite_sunset_server::{
    app_with_world,
    claims::{prepare::ClaimPrepareResponse, shared::ClaimStatus, status::ClaimResponse},
};

#[path = "support/claims_pause_env.rs"]
mod claims_pause_env;

#[tokio::test]
#[serial]
async fn claims_pause_blocks_submit_flow() -> Result<()> {
    claims_pause_env::setup().await?;
    let signer = claims_pause_env::test_signer_address();
    claims_pause_env::mark_all_claim_tokens_migrated(signer).await?;

    let core_config = zksync_lite_sunset_server::config::Config::load()?.core;
    let world = World::new(core_config).await?;
    let app = app_with_world(world.clone());

    // The worker is required to drain the new `pending` queue. Spawn it for
    // the duration of the test so submitted rows progress through broadcast.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let worker_handle = tokio::spawn({
        let world = world.clone();
        async move { claim_worker::run(world, shutdown_rx).await }
    });

    let token = claim_token_at_position(&app, signer, 0).await?;
    let prepared = prepare_single_claim(&app, &format!("{token:#x}"), signer).await?;
    let signature = claims_pause_env::sign_claim_batch(&prepared)?;

    claims_pause_env::pause_claims()?;

    let paused_submit_req = Request::builder()
        .method("POST")
        .uri("/api/claims")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "address": signer,
                "items": prepared.items,
                "claimsHash": prepared.claims_hash,
                "nonce": prepared.nonce,
                "expiry": prepared.expiry,
                "signature": signature
            })
            .to_string(),
        ))
        .unwrap();
    let paused_submit_res = app
        .clone()
        .oneshot(paused_submit_req)
        .await
        .wrap_err("paused submit request failed")?;

    ensure!(
        paused_submit_res.status() == StatusCode::OK,
        "unexpected paused submit status: {}",
        paused_submit_res.status()
    );
    let created = parse_claim_response(paused_submit_res).await?;
    ensure!(
        created.status == ClaimStatus::Pending,
        "expected pending status from submit, got: {:?}",
        created.status
    );

    // Wait for the worker to broadcast and the status poller to reconcile.
    // The paused contract either reverts on-chain (TX_REVERTED) or rejects the
    // broadcast (BROADCAST_FAILED) — both are valid terminal outcomes here.
    let terminal = poll_until_terminal(&app, created.claim_id.as_ref()).await?;
    ensure!(
        terminal.status == ClaimStatus::MinedReverted,
        "expected paused claim to reach mined_reverted, got: {:?}",
        terminal.status
    );
    let error_code = terminal.error_code.as_deref();
    ensure!(
        matches!(error_code, Some("TX_REVERTED" | "BROADCAST_FAILED")),
        "expected TX_REVERTED or BROADCAST_FAILED error code, got: {:?}",
        error_code
    );

    claims_pause_env::unpause_claims()?;

    let _ = shutdown_tx.send(true);
    let _ = worker_handle.await;

    Ok(())
}

async fn poll_until_terminal(app: &axum::Router, claim_id: &str) -> Result<ClaimResponse> {
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/claims/{claim_id}"))
            .body(Body::empty())
            .unwrap();
        let res = app
            .clone()
            .oneshot(req)
            .await
            .wrap_err("status request failed")?;
        ensure!(
            res.status() == StatusCode::OK,
            "unexpected status code: {}",
            res.status()
        );
        let parsed = parse_claim_response(res).await?;
        if matches!(
            parsed.status,
            ClaimStatus::MinedSuccess | ClaimStatus::MinedReverted
        ) {
            return Ok(parsed);
        }
    }
    Err(eyre::eyre!(
        "claim {claim_id} did not reach a terminal status within polling window"
    ))
}

async fn claim_token_at_position(
    app: &axum::Router,
    address: Address,
    position: usize,
) -> Result<Address> {
    let req = Request::builder()
        .method("POST")
        .uri("/api/claims/prepare")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "address": address,
                "allEligible": true
            })
            .to_string(),
        ))
        .unwrap();

    let res = app
        .clone()
        .oneshot(req)
        .await
        .wrap_err("prepare-all request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );
    let parsed = parse_claim_prepare_response(res).await?;
    let token = parsed
        .items
        .get(position)
        .ok_or_else(|| eyre::eyre!("missing claim token at position {position}"))?
        .token_address;
    Ok(token)
}

async fn prepare_single_claim(
    app: &axum::Router,
    token_address: &str,
    address: Address,
) -> Result<ClaimPrepareResponse> {
    let req = Request::builder()
        .method("POST")
        .uri("/api/claims/prepare")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "address": address,
                "tokenAddress": token_address
            })
            .to_string(),
        ))
        .unwrap();

    let res = app
        .clone()
        .oneshot(req)
        .await
        .wrap_err("prepare request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );

    parse_claim_prepare_response(res).await
}

async fn parse_claim_prepare_response(
    res: axum::response::Response,
) -> Result<ClaimPrepareResponse> {
    serde_json::from_slice(
        &res.into_body()
            .collect()
            .await
            .wrap_err("failed to read response body")?
            .to_bytes(),
    )
    .wrap_err("failed to parse claim prepare response")
}

async fn parse_claim_response(res: axum::response::Response) -> Result<ClaimResponse> {
    serde_json::from_slice(
        &res.into_body()
            .collect()
            .await
            .wrap_err("failed to read response body")?
            .to_bytes(),
    )
    .wrap_err("failed to parse claim response")
}
