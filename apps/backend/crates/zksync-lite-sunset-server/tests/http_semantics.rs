use axum::{
    body::Body,
    http::{HeaderValue, Request, StatusCode, header},
};
use eyre::{Result, WrapErr as _, ensure};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt as _;
use zksync_lite_sunset_core::config::ClaimFrontendMode;
use zksync_lite_sunset_server::{
    CorsConfig, app_pair_with_world, app_with_world, app_with_world_and_cors,
};

mod support;
use support::{dummy_world, dummy_world_with};

#[tokio::test]
async fn eligibility_invalid_address_returns_structured_error_without_legacy_harness() -> Result<()>
{
    let app = app_with_world(dummy_world()?);

    let req = Request::builder()
        .method("POST")
        .uri("/api/eligibility")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"address":"not-an-address"}"#))
        .unwrap();

    let res = app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::BAD_REQUEST,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("code").and_then(|value| value.as_str()) == Some("INVALID_ADDRESS"),
        "expected code=INVALID_ADDRESS, got: {body}"
    );
    ensure!(
        body.get("message")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .contains("invalid address"),
        "expected invalid-address message, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn frontend_config_reports_paid_default() -> Result<()> {
    let app = app_with_world(dummy_world()?);

    let req = Request::builder()
        .method("GET")
        .uri("/api/config")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("claimMode").and_then(|value| value.as_str()) == Some("paid"),
        "expected claimMode=paid, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn frontend_config_reports_sponsored_mode() -> Result<()> {
    let app = app_with_world(dummy_world_with(|config| {
        config.claims.frontend_mode = ClaimFrontendMode::Sponsored;
    })?);

    let req = Request::builder()
        .method("GET")
        .uri("/api/config")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("claimMode").and_then(|value| value.as_str()) == Some("sponsored"),
        "expected claimMode=sponsored, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn frontend_config_reports_unavailable() -> Result<()> {
    let app = app_with_world(dummy_world_with(|config| {
        config.claims.frontend_mode = ClaimFrontendMode::Unavailable;
    })?);

    let req = Request::builder()
        .method("GET")
        .uri("/api/config")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("claimMode").and_then(|value| value.as_str()) == Some("unavailable"),
        "expected claimMode=unavailable, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn sponsored_claim_submit_returns_structured_error_when_disabled() -> Result<()> {
    let app = app_with_world(dummy_world_with(|config| {
        config.claims.frontend_mode = ClaimFrontendMode::Paid;
    })?);

    let req = Request::builder()
        .method("POST")
        .uri("/api/claims")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "address": "0x0000000000000000000000000000000000000001",
                "items": [],
                "claimsHash": format!("0x{}", "0".repeat(64)),
                "nonce": 0,
                "expiry": 1,
                "signature": "0x"
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::CONFLICT,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("code").and_then(|value| value.as_str()) == Some("SPONSORED_CLAIMS_DISABLED"),
        "expected code=SPONSORED_CLAIMS_DISABLED, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn cors_preflight_allows_claim_and_migrate_origins_without_legacy_harness() -> Result<()> {
    let app = app_with_world_and_cors(dummy_world()?, CorsConfig::enabled_for_dev());

    for origin in ["http://localhost:4321", "http://localhost:4322"] {
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/eligibility")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .body(Body::empty())
            .unwrap();

        let res = app
            .clone()
            .oneshot(req)
            .await
            .wrap_err("preflight request failed")?;
        ensure!(
            res.status().is_success(),
            "unexpected preflight status for {origin}: {}",
            res.status()
        );
        ensure!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                == Some(&HeaderValue::from_str(origin).unwrap()),
            "unexpected Access-Control-Allow-Origin for {origin}: {:?}",
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
    }

    Ok(())
}

#[tokio::test]
async fn cors_invalid_post_includes_allow_origin_without_legacy_harness() -> Result<()> {
    let app = app_with_world_and_cors(dummy_world()?, CorsConfig::enabled_for_dev());

    for origin in ["http://127.0.0.1:4321", "http://127.0.0.1:4322"] {
        let req = Request::builder()
            .method("POST")
            .uri("/api/eligibility")
            .header(header::ORIGIN, origin)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"not-an-address"}"#))
            .unwrap();

        let res = app
            .clone()
            .oneshot(req)
            .await
            .wrap_err("cors request failed")?;
        ensure!(
            res.status() == StatusCode::BAD_REQUEST,
            "unexpected status for {origin}: {}",
            res.status()
        );
        ensure!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                == Some(&HeaderValue::from_str(origin).unwrap()),
            "unexpected Access-Control-Allow-Origin for {origin}: {:?}",
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
    }

    Ok(())
}

#[tokio::test]
async fn cors_allows_custom_seeded_stack_origin_without_legacy_harness() -> Result<()> {
    let app = app_with_world_and_cors(
        dummy_world()?,
        CorsConfig {
            enabled: true,
            allowed_origins: vec![
                "http://localhost:4324".to_string(),
                "http://127.0.0.1:4324".to_string(),
            ],
        },
    );

    for origin in ["http://localhost:4324", "http://127.0.0.1:4324"] {
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/eligibility")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .body(Body::empty())
            .unwrap();

        let res = app
            .clone()
            .oneshot(req)
            .await
            .wrap_err("custom preflight request failed")?;
        ensure!(
            res.status().is_success(),
            "unexpected preflight status for {origin}: {}",
            res.status()
        );
        ensure!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                == Some(&HeaderValue::from_str(origin).unwrap()),
            "unexpected Access-Control-Allow-Origin for {origin}: {:?}",
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
    }

    Ok(())
}

#[tokio::test]
async fn livez_reports_process_uptime_without_legacy_harness() -> Result<()> {
    let (_app, ops_app) = app_pair_with_world(dummy_world()?);

    let req = Request::builder()
        .method("GET")
        .uri("/livez")
        .body(Body::empty())
        .unwrap();

    let res = ops_app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::OK,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("status").and_then(|value| value.as_str()) == Some("ok"),
        "unexpected response body: {body}"
    );
    ensure!(
        body.get("uptime_seconds")
            .and_then(|value| value.as_u64())
            .is_some(),
        "expected uptime_seconds in response body: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn readyz_reports_unhealthy_dependencies_without_legacy_harness() -> Result<()> {
    let (_app, ops_app) = app_pair_with_world(dummy_world()?);

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let res = ops_app.oneshot(req).await.wrap_err("request failed")?;
    ensure!(
        res.status() == StatusCode::SERVICE_UNAVAILABLE,
        "unexpected status: {}",
        res.status()
    );

    let body = read_json_body(res).await?;
    ensure!(
        body.get("status").and_then(|value| value.as_str()) == Some("error"),
        "unexpected response body: {body}"
    );
    ensure!(
        body.pointer("/checks/database/status")
            .and_then(|value| value.as_str())
            == Some("error"),
        "expected unhealthy database check: {body}"
    );
    ensure!(
        body.pointer("/checks/rpc/status")
            .and_then(|value| value.as_str())
            == Some("error"),
        "expected unhealthy rpc check: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn healthz_reports_public_uptime_and_readyz() -> Result<()> {
    let app = app_with_world(dummy_world()?);

    let healthz_req = Request::builder()
        .method("GET")
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();

    let healthz_res = app
        .clone()
        .oneshot(healthz_req)
        .await
        .wrap_err("healthz request failed")?;
    ensure!(
        healthz_res.status() == StatusCode::OK,
        "unexpected healthz status: {}",
        healthz_res.status()
    );

    let body = read_json_body(healthz_res).await?;
    ensure!(
        body.get("status").and_then(|value| value.as_str()) == Some("ok"),
        "unexpected response body: {body}"
    );
    ensure!(
        body.get("uptime_seconds")
            .and_then(|value| value.as_u64())
            .is_some(),
        "expected uptime_seconds in response body: {body}"
    );

    let readyz_req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let readyz_res = app
        .oneshot(readyz_req)
        .await
        .wrap_err("readyz request failed")?;
    ensure!(
        readyz_res.status() == StatusCode::SERVICE_UNAVAILABLE,
        "expected public /readyz to expose readiness status, got {}",
        readyz_res.status()
    );

    let readyz_body = read_json_body(readyz_res).await?;
    ensure!(
        readyz_body.get("status").and_then(|value| value.as_str()) == Some("error"),
        "unexpected readyz response body: {readyz_body}"
    );

    Ok(())
}
async fn read_json_body(res: axum::response::Response) -> Result<serde_json::Value> {
    let bytes = res
        .into_body()
        .collect()
        .await
        .wrap_err("failed to read response body")?
        .to_bytes();
    serde_json::from_slice(&bytes).wrap_err("failed to parse JSON response")
}
