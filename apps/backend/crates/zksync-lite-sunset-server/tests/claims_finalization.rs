//! Regression tests for the poll-driven finalization fix.
//!
//! Before the fix, finalization happened as a side effect of
//! `GET /api/claims/{id}`. If the client closed the tab mid-poll, the row was
//! left stuck in `submitted` / `mining` even after the L1 tx landed. These
//! tests assert the inverse: the row reaches `mined_success` purely from the
//! background claim worker, with **zero** calls to `GET /api/claims/{id}`
//! during the wait, and the GET endpoint never mutates DB state.

use alloy::primitives::Address;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use eyre::{Result, WrapErr as _, ensure};
use http_body_util::BodyExt as _;
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt as _;
use zksync_lite_sunset_core::{claims::worker as claim_worker, world::World};
use zksync_lite_sunset_server::{
    app_with_world,
    claims::{prepare::ClaimPrepareResponse, shared::ClaimStatus, status::ClaimResponse},
};

#[path = "support/claims_eoa_env.rs"]
mod claims_eoa_env;

/// Snapshot of every mutable field on a `claim_requests` row. Used to assert
/// that `GET /api/claims/{id}` does not write to the DB after the fix.
#[derive(Debug, PartialEq, Eq)]
struct ClaimRowSnapshot {
    status: String,
    tx_hash: Option<String>,
    block_number: Option<i64>,
    block_hash: Option<String>,
    error: Option<String>,
    error_code: Option<String>,
    error_detail: Option<String>,
    attempts: i32,
    next_attempt_at_epoch: Option<i64>,
    submitted_at_epoch: Option<i64>,
    updated_at_epoch: i64,
}

#[tokio::test]
#[serial]
async fn worker_drives_claim_to_terminal_without_status_polling() -> Result<()> {
    claims_eoa_env::setup().await?;
    let signer = claims_eoa_env::test_signer_address();
    claims_eoa_env::mark_all_claim_tokens_migrated(signer).await?;

    let core_config = zksync_lite_sunset_server::config::Config::load()?.core;
    let world = World::new(core_config).await?;
    let app = app_with_world(world.clone());

    // Spin up the claim worker for the test. With the fix in place, this is
    // the only thing that should be moving the row through `submitted` ->
    // `mining` -> `mined_success` — the test never touches `GET /api/claims`.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let worker_handle = tokio::spawn({
        let world = world.clone();
        async move { claim_worker::run(world, shutdown_rx).await }
    });

    let token = claim_token_at_position(&app, signer, 0).await?;
    let prepared = prepare_single_claim(&app, &format!("{token:#x}"), signer).await?;
    let signature = claims_eoa_env::sign_claim_batch(&prepared)?;

    let submit_req = Request::builder()
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

    let submit_res = app
        .clone()
        .oneshot(submit_req)
        .await
        .wrap_err("submit request failed")?;
    ensure!(
        submit_res.status() == StatusCode::OK,
        "unexpected submit status: {}",
        submit_res.status()
    );
    let created = parse_claim_response(submit_res).await?;
    ensure!(
        created.status == ClaimStatus::Pending,
        "expected pending status from submit, got: {:?}",
        created.status
    );
    let claim_id = created.claim_id.as_ref().to_string();

    let pool = world.db_pool().clone();

    // Critical part of the regression test: poll the **DB**, not the API.
    // If the row reaches terminal state, only the worker could have driven it.
    let terminal_status = wait_for_terminal_status(&pool, &claim_id).await?;
    ensure!(
        terminal_status == "mined_success",
        "expected worker to drive claim to mined_success without any GET calls, got: {terminal_status}",
    );

    // After the worker terminalizes the row, `GET /api/claims/{id}` must (a)
    // return the terminal state and (b) leave the DB row exactly as it was.
    // The before/after snapshot is what locks in the "pure read" property.
    let before = snapshot_claim_row(&pool, &claim_id).await?;

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/api/claims/{claim_id}"))
        .body(Body::empty())
        .unwrap();
    let get_res = app
        .clone()
        .oneshot(get_req)
        .await
        .wrap_err("status request failed")?;
    ensure!(
        get_res.status() == StatusCode::OK,
        "unexpected status code: {}",
        get_res.status()
    );
    let parsed = parse_claim_response(get_res).await?;
    ensure!(
        parsed.status == ClaimStatus::MinedSuccess,
        "expected GET to report MinedSuccess, got: {:?}",
        parsed.status,
    );

    let after = snapshot_claim_row(&pool, &claim_id).await?;
    ensure!(
        before == after,
        "GET /api/claims mutated the row — pure-read invariant violated\nbefore: {before:?}\nafter:  {after:?}",
    );

    let _ = shutdown_tx.send(true);
    let _ = worker_handle.await;

    Ok(())
}

/// Polls the `claim_requests` row directly until it lands in a terminal
/// status. The polling cap matches the existing API-level helper so we don't
/// time out faster than the worker can drain + finalize + auto-mine on anvil.
async fn wait_for_terminal_status(pool: &PgPool, claim_id: &str) -> Result<String> {
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT status FROM claim_requests WHERE claim_id = $1")
                .bind(claim_id)
                .fetch_optional(pool)
                .await
                .wrap_err("failed to read claim_requests status during DB poll")?;
        let Some((status,)) = row else {
            continue;
        };
        if matches!(status.as_str(), "mined_success" | "mined_reverted") {
            return Ok(status);
        }
    }
    Err(eyre::eyre!(
        "claim {claim_id} never reached terminal status via worker-only path",
    ))
}

async fn snapshot_claim_row(pool: &PgPool, claim_id: &str) -> Result<ClaimRowSnapshot> {
    let row = sqlx::query(
        r#"
        SELECT
            status,
            tx_hash,
            block_number,
            block_hash,
            error,
            error_code,
            error_detail,
            attempts,
            EXTRACT(EPOCH FROM next_attempt_at)::BIGINT AS next_attempt_at_epoch,
            EXTRACT(EPOCH FROM submitted_at)::BIGINT AS submitted_at_epoch,
            EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_epoch
        FROM claim_requests
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id)
    .fetch_one(pool)
    .await
    .wrap_err("failed to snapshot claim_requests row")?;

    Ok(ClaimRowSnapshot {
        status: row.try_get("status")?,
        tx_hash: row.try_get("tx_hash")?,
        block_number: row.try_get("block_number")?,
        block_hash: row.try_get("block_hash")?,
        error: row.try_get("error")?,
        error_code: row.try_get("error_code")?,
        error_detail: row.try_get("error_detail")?,
        attempts: row.try_get("attempts")?,
        next_attempt_at_epoch: row.try_get("next_attempt_at_epoch")?,
        submitted_at_epoch: row.try_get("submitted_at_epoch")?,
        updated_at_epoch: row
            .try_get::<Option<i64>, _>("updated_at_epoch")?
            .unwrap_or(0),
    })
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
