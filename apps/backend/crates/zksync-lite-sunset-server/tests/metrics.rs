use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use eyre::{ContextCompat as _, Result, WrapErr as _, ensure};
use tower::ServiceExt as _;
use zksync_lite_sunset_core::observability::{
    CLAIM_RESULT_SUCCESS, CLAIM_RESULT_THROTTLED, Observability,
};
use zksync_lite_sunset_server::app_with_world;

mod support;

#[test]
fn observability_records_claim_flow_metrics_without_legacy_harness() -> Result<()> {
    let observability = Observability::new();

    observability.observe_claim_prepare_request(CLAIM_RESULT_SUCCESS, Duration::from_millis(12));
    observability.observe_claim_submit_request("invalid_signature", Duration::from_millis(34));
    observability.observe_claim_status_check("claim_not_found", Duration::from_millis(5));
    observability.observe_claim_items_per_request(2);
    observability.observe_claim_finalization("mined_success", 1.5);
    observability.observe_claim_receipt_timeout();
    observability.observe_claim_finalization("receipt_timeout", 3600.0);
    observability.observe_claim_write_rejection();
    observability.observe_http_request("/api/claims/prepare", 200, Duration::from_millis(6));
    observability.record_claim_worker_heartbeat();

    let metrics = observability.render_text();

    ensure!(
        metrics.contains("sunset_claim_prepare_requests_total{result=\"success\"} 1"),
        "expected successful prepare counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_submit_requests_total{result=\"invalid_signature\"} 1"),
        "expected invalid signature submit counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains(&format!(
            "sunset_claim_submit_requests_total{{result=\"{CLAIM_RESULT_THROTTLED}\"}} 1"
        )),
        "expected throttled submit counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_status_checks_total{result=\"claim_not_found\"} 1"),
        "expected claim_not_found status counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_items_per_request_count 1"),
        "expected claim items histogram count in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_finalizations_total{result=\"mined_success\"} 1"),
        "expected mined_success finalization counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_finalizations_total{result=\"receipt_timeout\"} 1"),
        "expected receipt_timeout finalization counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_receipt_timeouts_total 1"),
        "expected receipt timeout counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics.contains("sunset_claim_write_rejections_total 1"),
        "expected claim write rejection counter in metrics output:\n{metrics}"
    );
    ensure!(
        metrics
            .contains("sunset_http_requests_total{route=\"/api/claims/prepare\",status=\"200\"} 1"),
        "expected HTTP request counter in metrics output:\n{metrics}"
    );
    let worker_heartbeat = metrics
        .lines()
        .find(|line| line.starts_with("sunset_claim_worker_heartbeat_timestamp_seconds "))
        .wrap_err("expected claim worker heartbeat timestamp gauge in metrics output")?;
    let worker_heartbeat = worker_heartbeat
        .split_whitespace()
        .nth(1)
        .wrap_err("expected heartbeat timestamp value")?
        .parse::<f64>()
        .wrap_err("expected heartbeat timestamp to be numeric")?;
    ensure!(
        worker_heartbeat > 0.0,
        "expected positive worker heartbeat timestamp, got {worker_heartbeat}"
    );

    Ok(())
}

#[test]
fn observability_initializes_claim_backlog_gauges_without_legacy_harness() -> Result<()> {
    let metrics = Observability::new().render_text();

    for status in [
        "pending",
        "prepared",
        "submitted",
        "mining",
        "mined_success",
        "mined_reverted",
    ] {
        ensure!(
            metrics.contains(&format!("sunset_claim_backlog{{status=\"{status}\"}} 0")),
            "expected backlog gauge for {status} in metrics output:\n{metrics}"
        );
    }
    for status in ["pending", "submitted", "mining"] {
        ensure!(
            metrics.contains(&format!(
                "sunset_claim_oldest_active_age_seconds{{status=\"{status}\"}} 0"
            )),
            "expected oldest active age gauge for {status} in metrics output:\n{metrics}"
        );
    }

    Ok(())
}

#[tokio::test]
async fn public_router_hides_metrics_without_legacy_harness() -> Result<()> {
    let app = app_with_world(support::dummy_world()?);

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let res = app
        .oneshot(req)
        .await
        .wrap_err("public metrics request failed")?;
    ensure!(
        res.status() == StatusCode::NOT_FOUND,
        "expected public app to hide /metrics, got {}",
        res.status()
    );

    Ok(())
}
