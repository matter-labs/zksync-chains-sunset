use alloy::primitives::Address;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use eyre::{Result, WrapErr as _, ensure};
use http_body_util::BodyExt as _;
use serde_json::json;
use serial_test::serial;
use tower::ServiceExt as _;
use zksync_lite_sunset_core::claims::{
    prepare::ClaimPrepareResponse, shared::ClaimStatus, status::ClaimResponse,
};

#[path = "support/claims_smart_account_env.rs"]
mod claims_smart_account_env;

#[tokio::test]
#[serial]
async fn sponsored_claim_accepts_erc1271_smart_account_signature() -> Result<()> {
    let smart_account = claims_smart_account_env::setup_smart_account().await?;
    claims_smart_account_env::mark_all_claim_tokens_migrated(smart_account).await?;
    let app = zksync_lite_sunset_server::app().await?;

    let prepared = prepare_all_claims(&app, smart_account).await?;
    let signature = claims_smart_account_env::sign_claim_batch(&prepared)?;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/claims")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "address": smart_account,
                        "items": prepared.items,
                        "claimsHash": prepared.claims_hash,
                        "nonce": prepared.nonce,
                        "expiry": prepared.expiry,
                        "signature": signature
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .wrap_err("smart-account sponsored submit failed")?;

    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}, body: {}",
        res.status(),
        read_body_text(res).await?
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn sponsored_claim_rejects_erc1271_signature_from_wrong_owner() -> Result<()> {
    let smart_account = claims_smart_account_env::setup_smart_account().await?;
    claims_smart_account_env::mark_all_claim_tokens_migrated(smart_account).await?;
    let app = zksync_lite_sunset_server::app().await?;

    let prepared = prepare_all_claims(&app, smart_account).await?;
    let signature = claims_smart_account_env::sign_claim_batch_as_sponsor(&prepared)?;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/claims")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "address": smart_account,
                        "items": prepared.items,
                        "claimsHash": prepared.claims_hash,
                        "nonce": prepared.nonce,
                        "expiry": prepared.expiry,
                        "signature": signature
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .wrap_err("smart-account sponsored submit failed")?;

    assert_error_response(res, StatusCode::BAD_REQUEST, "INVALID_SIGNATURE").await
}

#[tokio::test]
#[serial]
async fn direct_claim_accepts_smart_account_execution_receipt() -> Result<()> {
    let smart_account = claims_smart_account_env::setup_smart_account().await?;
    claims_smart_account_env::mark_all_claim_tokens_migrated(smart_account).await?;
    let app = zksync_lite_sunset_server::app().await?;

    let prepared = prepare_all_claims(&app, smart_account).await?;
    let tx_hash = claims_smart_account_env::execute_smart_wallet(
        smart_account,
        prepared.direct_tx.contract_address,
        &format!("{:#x}", prepared.direct_tx.call_data),
    )?;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/claims/direct")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "claimId": prepared.claim_id,
                        "txHash": tx_hash,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .wrap_err("smart-account direct submit failed")?;

    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}, body: {}",
        res.status(),
        read_body_text(res).await?
    );
    let parsed: ClaimResponse = serde_json::from_slice(&read_body_bytes(res).await?)
        .wrap_err("failed to parse response")?;
    ensure!(
        parsed.status == ClaimStatus::Submitted,
        "unexpected claim status: {:?}",
        parsed.status
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn direct_claim_rejects_smart_account_receipt_without_expected_claim_logs() -> Result<()> {
    let smart_account = claims_smart_account_env::setup_smart_account().await?;
    claims_smart_account_env::mark_all_claim_tokens_migrated(smart_account).await?;
    let app = zksync_lite_sunset_server::app().await?;

    let prepared = prepare_all_claims(&app, smart_account).await?;
    let claim_id = prepared.claim_id.clone();
    let tx_hash =
        claims_smart_account_env::execute_smart_wallet(smart_account, smart_account, "0x")?;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/claims/direct")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "claimId": claim_id.as_str(),
                        "txHash": tx_hash,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .wrap_err("smart-account direct submit failed")?;

    assert_error_response(res, StatusCode::CONFLICT, "SUBMITTED_TX_MISMATCH").await?;

    let claim = load_claim(&app, claim_id.as_str()).await?;
    ensure!(
        claim.status == ClaimStatus::Prepared,
        "expected failed direct submit to leave claim prepared, got: {:?}",
        claim.status
    );
    ensure!(
        claim.tx_hash.is_none(),
        "expected failed direct submit not to persist tx hash, got: {:?}",
        claim.tx_hash
    );

    Ok(())
}

async fn prepare_all_claims(app: &axum::Router, address: Address) -> Result<ClaimPrepareResponse> {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
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
                .unwrap(),
        )
        .await
        .wrap_err("prepare-all request failed")?;

    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}, body: {}",
        res.status(),
        read_body_text(res).await?
    );

    serde_json::from_slice(&read_body_bytes(res).await?)
        .wrap_err("failed to parse prepare response")
}

async fn load_claim(app: &axum::Router, claim_id: &str) -> Result<ClaimResponse> {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/claims/{claim_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .wrap_err("claim status request failed")?;

    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}, body: {}",
        res.status(),
        read_body_text(res).await?
    );

    serde_json::from_slice(&read_body_bytes(res).await?).wrap_err("failed to parse claim response")
}

async fn assert_error_response(
    res: axum::response::Response,
    expected_status: StatusCode,
    expected_code: &str,
) -> Result<()> {
    let status = res.status();
    let body = read_json_body(res).await?;
    ensure!(
        status == expected_status,
        "expected status {expected_status}, got {status}, body: {body}"
    );
    ensure!(
        body.get("code").and_then(|value| value.as_str()) == Some(expected_code),
        "expected code {expected_code}, got: {body}"
    );
    Ok(())
}

async fn read_json_body(res: axum::response::Response) -> Result<serde_json::Value> {
    serde_json::from_slice(&read_body_bytes(res).await?).wrap_err("failed to parse JSON response")
}

async fn read_body_text(res: axum::response::Response) -> Result<String> {
    let bytes = read_body_bytes(res).await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn read_body_bytes(res: axum::response::Response) -> Result<axum::body::Bytes> {
    Ok(res
        .into_body()
        .collect()
        .await
        .wrap_err("failed to read response body")?
        .to_bytes())
}
