use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGaugeVec,
    Opts, Registry, TextEncoder,
};
use sqlx::{PgPool, Row};

use crate::claims::shared::{ClaimError, ClaimFailureCode};

const CLAIM_BACKLOG_STATUSES: [&str; 6] = [
    "prepared",
    "pending",
    "submitted",
    "mining",
    "mined_success",
    "mined_reverted",
];
const CLAIM_ACTIVE_AGE_STATUSES: [&str; 3] = ["pending", "submitted", "mining"];

pub const CLAIM_RESULT_SUCCESS: &str = "success";
pub const CLAIM_RESULT_THROTTLED: &str = "throttled";

pub struct Observability {
    registry: Registry,
    http_requests_total: IntCounterVec,
    http_request_duration_seconds: HistogramVec,
    claim_prepare_requests_total: IntCounterVec,
    claim_prepare_duration_seconds: Histogram,
    claim_submit_requests_total: IntCounterVec,
    claim_submit_duration_seconds: Histogram,
    claim_status_checks_total: IntCounterVec,
    claim_status_duration_seconds: Histogram,
    claim_finalizations_total: IntCounterVec,
    claim_finalization_duration_seconds: Histogram,
    claim_items_per_request: Histogram,
    claim_write_rejections_total: IntCounter,
    claim_receipt_timeouts_total: IntCounter,
    claim_worker_heartbeat_timestamp_seconds: Gauge,
    claim_backlog: IntGaugeVec,
    claim_oldest_active_age_seconds: IntGaugeVec,
}

impl Observability {
    pub fn new() -> Self {
        let http_requests_total = IntCounterVec::new(
            Opts::new(
                "sunset_http_requests_total",
                "HTTP requests served by the sunset backend.",
            ),
            &["route", "status"],
        )
        .expect("http request counter definition should be valid");
        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "sunset_http_request_duration_seconds",
                "HTTP request latency for the sunset backend.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
            &["route", "status"],
        )
        .expect("http request duration histogram definition should be valid");
        let claim_prepare_requests_total = IntCounterVec::new(
            Opts::new(
                "sunset_claim_prepare_requests_total",
                "Claim prepare requests partitioned by result.",
            ),
            &["result"],
        )
        .expect("claim prepare counter definition should be valid");
        let claim_prepare_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "sunset_claim_prepare_duration_seconds",
                "Latency for claim prepare requests.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
        )
        .expect("claim prepare duration histogram definition should be valid");
        let claim_submit_requests_total = IntCounterVec::new(
            Opts::new(
                "sunset_claim_submit_requests_total",
                "Claim submit requests partitioned by result.",
            ),
            &["result"],
        )
        .expect("claim submit counter definition should be valid");
        let claim_submit_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "sunset_claim_submit_duration_seconds",
                "Latency for claim submit requests.",
            )
            .buckets(vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        )
        .expect("claim submit duration histogram definition should be valid");
        let claim_status_checks_total = IntCounterVec::new(
            Opts::new(
                "sunset_claim_status_checks_total",
                "Claim status checks partitioned by result.",
            ),
            &["result"],
        )
        .expect("claim status counter definition should be valid");
        let claim_status_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "sunset_claim_status_duration_seconds",
                "Latency for claim status requests.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
        )
        .expect("claim status duration histogram definition should be valid");
        let claim_finalizations_total = IntCounterVec::new(
            Opts::new(
                "sunset_claim_finalizations_total",
                "Claim finalization outcomes partitioned by result.",
            ),
            &["result"],
        )
        .expect("claim finalization counter definition should be valid");
        let claim_finalization_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "sunset_claim_finalization_duration_seconds",
                "Elapsed time from claim submission to terminal finalization.",
            )
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0]),
        )
        .expect("claim finalization duration histogram definition should be valid");
        let claim_items_per_request = Histogram::with_opts(
            HistogramOpts::new(
                "sunset_claim_items_per_request",
                "Number of claim items submitted per request.",
            )
            .buckets(vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0]),
        )
        .expect("claim items per request histogram definition should be valid");
        let claim_write_rejections_total = IntCounter::with_opts(Opts::new(
            "sunset_claim_write_rejections_total",
            "Rejected claim write requests due to concurrency throttling.",
        ))
        .expect("claim write rejection counter definition should be valid");
        let claim_receipt_timeouts_total = IntCounter::with_opts(Opts::new(
            "sunset_claim_receipt_timeouts_total",
            "Claim receipt timeouts observed during finalization polling.",
        ))
        .expect("claim receipt timeout counter definition should be valid");
        let claim_worker_heartbeat_timestamp_seconds = Gauge::with_opts(Opts::new(
            "sunset_claim_worker_heartbeat_timestamp_seconds",
            "Unix timestamp for the latest healthy claim worker loop heartbeat.",
        ))
        .expect("claim worker heartbeat gauge definition should be valid");
        let claim_backlog = IntGaugeVec::new(
            Opts::new(
                "sunset_claim_backlog",
                "Current claim backlog grouped by claim status.",
            ),
            &["status"],
        )
        .expect("claim backlog gauge definition should be valid");
        let claim_oldest_active_age_seconds = IntGaugeVec::new(
            Opts::new(
                "sunset_claim_oldest_active_age_seconds",
                "Age in seconds of the oldest non-terminal claim grouped by claim status.",
            ),
            &["status"],
        )
        .expect("claim oldest active age gauge definition should be valid");

        let mut registry = Registry::new();
        register_metric(&mut registry, http_requests_total.clone());
        register_metric(&mut registry, http_request_duration_seconds.clone());
        register_metric(&mut registry, claim_prepare_requests_total.clone());
        register_metric(&mut registry, claim_prepare_duration_seconds.clone());
        register_metric(&mut registry, claim_submit_requests_total.clone());
        register_metric(&mut registry, claim_submit_duration_seconds.clone());
        register_metric(&mut registry, claim_status_checks_total.clone());
        register_metric(&mut registry, claim_status_duration_seconds.clone());
        register_metric(&mut registry, claim_finalizations_total.clone());
        register_metric(&mut registry, claim_finalization_duration_seconds.clone());
        register_metric(&mut registry, claim_items_per_request.clone());
        register_metric(&mut registry, claim_write_rejections_total.clone());
        register_metric(&mut registry, claim_receipt_timeouts_total.clone());
        register_metric(
            &mut registry,
            claim_worker_heartbeat_timestamp_seconds.clone(),
        );
        register_metric(&mut registry, claim_backlog.clone());
        register_metric(&mut registry, claim_oldest_active_age_seconds.clone());

        for status in CLAIM_BACKLOG_STATUSES {
            claim_backlog.with_label_values(&[status]).set(0);
        }
        for status in CLAIM_ACTIVE_AGE_STATUSES {
            claim_oldest_active_age_seconds
                .with_label_values(&[status])
                .set(0);
        }

        Self {
            registry,
            http_requests_total,
            http_request_duration_seconds,
            claim_prepare_requests_total,
            claim_prepare_duration_seconds,
            claim_submit_requests_total,
            claim_submit_duration_seconds,
            claim_status_checks_total,
            claim_status_duration_seconds,
            claim_finalizations_total,
            claim_finalization_duration_seconds,
            claim_items_per_request,
            claim_write_rejections_total,
            claim_receipt_timeouts_total,
            claim_worker_heartbeat_timestamp_seconds,
            claim_backlog,
            claim_oldest_active_age_seconds,
        }
    }

    pub fn render_text(&self) -> String {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("metrics encoding should succeed");
        String::from_utf8(buffer).expect("metrics text output should be utf-8")
    }

    pub fn observe_http_request(&self, route: &str, status: u16, duration: Duration) {
        let status = status.to_string();
        self.http_requests_total
            .with_label_values(&[route, status.as_str()])
            .inc();
        self.http_request_duration_seconds
            .with_label_values(&[route, status.as_str()])
            .observe(duration.as_secs_f64());
    }

    pub fn observe_claim_prepare_request(&self, result: &str, duration: Duration) {
        self.claim_prepare_requests_total
            .with_label_values(&[result])
            .inc();
        self.claim_prepare_duration_seconds
            .observe(duration.as_secs_f64());
    }

    pub fn observe_claim_submit_request(&self, result: &str, duration: Duration) {
        self.claim_submit_requests_total
            .with_label_values(&[result])
            .inc();
        self.claim_submit_duration_seconds
            .observe(duration.as_secs_f64());
    }

    pub fn observe_claim_status_check(&self, result: &str, duration: Duration) {
        self.claim_status_checks_total
            .with_label_values(&[result])
            .inc();
        self.claim_status_duration_seconds
            .observe(duration.as_secs_f64());
    }

    pub fn observe_claim_items_per_request(&self, item_count: usize) {
        self.claim_items_per_request.observe(item_count as f64);
    }

    pub fn observe_claim_finalization(&self, result: &str, duration_seconds: f64) {
        self.claim_finalizations_total
            .with_label_values(&[result])
            .inc();
        self.claim_finalization_duration_seconds
            .observe(duration_seconds.max(0.0));
    }

    pub fn observe_claim_write_rejection(&self) {
        self.claim_write_rejections_total.inc();
        self.claim_submit_requests_total
            .with_label_values(&[CLAIM_RESULT_THROTTLED])
            .inc();
    }

    pub fn observe_claim_receipt_timeout(&self) {
        self.claim_receipt_timeouts_total.inc();
    }

    pub fn record_claim_worker_heartbeat(&self) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.claim_worker_heartbeat_timestamp_seconds.set(timestamp);
    }

    pub async fn refresh_claim_backlog(&self, db_pool: &PgPool) -> Result<(), sqlx::Error> {
        for status in CLAIM_BACKLOG_STATUSES {
            self.claim_backlog.with_label_values(&[status]).set(0);
        }
        for status in CLAIM_ACTIVE_AGE_STATUSES {
            self.claim_oldest_active_age_seconds
                .with_label_values(&[status])
                .set(0);
        }

        let rows = sqlx::query(
            r#"
            SELECT status, COUNT(*)::BIGINT AS total
            FROM claim_requests
            GROUP BY status
            "#,
        )
        .fetch_all(db_pool)
        .await?;

        for row in rows {
            let status: String = row.try_get("status")?;
            let total: i64 = row.try_get("total")?;
            self.claim_backlog
                .with_label_values(&[status.as_str()])
                .set(total);
        }

        let rows = sqlx::query(
            r#"
            SELECT
                status,
                EXTRACT(EPOCH FROM (NOW() - MIN(COALESCE(submitted_at, updated_at, created_at))))::BIGINT AS oldest_age_seconds
            FROM claim_requests
            WHERE status IN ('pending', 'submitted', 'mining')
            GROUP BY status
            "#,
        )
        .fetch_all(db_pool)
        .await?;

        for row in rows {
            let status: String = row.try_get("status")?;
            let oldest_age_seconds: i64 = row.try_get("oldest_age_seconds")?;
            self.claim_oldest_active_age_seconds
                .with_label_values(&[status.as_str()])
                .set(oldest_age_seconds.max(0));
        }

        Ok(())
    }
}

impl Default for Observability {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Observability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Observability")
    }
}

pub fn claim_result_label_for_error(error: &ClaimError) -> &'static str {
    match error {
        ClaimError::InvalidAddress { .. } => "invalid_address",
        ClaimError::NoClaimsSelected => "no_claims_selected",
        ClaimError::TooManyClaims { .. } => "too_many_claims",
        ClaimError::TokenNotEligible { .. } => "token_not_eligible",
        ClaimError::NftClaimNotSupported { .. } => "nft_claim_not_supported",
        ClaimError::InvalidClaimOrdering => "invalid_claim_ordering",
        ClaimError::InvalidClaimsHash => "invalid_claims_hash",
        ClaimError::ExpiredSignature => "expired_signature",
        ClaimError::InvalidSignature => "invalid_signature",
        ClaimError::MerkleRootMismatch { .. } => "merkle_root_mismatch",
        ClaimError::InvalidNonce { .. } => "invalid_nonce",
        ClaimError::OnchainConfig { .. } => "onchain_config_error",
        ClaimError::OnchainRpc { .. } => "onchain_rpc_error",
        ClaimError::ClaimNotFound { .. } => "claim_not_found",
        ClaimError::InvalidTxHash { .. } => "invalid_tx_hash",
        ClaimError::SubmittedTransactionNotFound { .. } => "submitted_tx_not_found",
        ClaimError::SubmittedTransactionMismatch { .. } => "submitted_tx_mismatch",
        ClaimError::Storage { .. } => "storage_error",
    }
}

pub(crate) fn claim_finalization_result_label(failure: Option<ClaimFailureCode>) -> &'static str {
    match failure {
        None => "mined_success",
        Some(ClaimFailureCode::TxReverted) => "tx_reverted",
        Some(ClaimFailureCode::ReceiptTimeout) => "receipt_timeout",
        Some(ClaimFailureCode::OnchainStateMismatch) => "state_mismatch",
        Some(ClaimFailureCode::BroadcastFailed) => "broadcast_failed",
    }
}

fn register_metric<M>(registry: &mut Registry, metric: M)
where
    M: prometheus::core::Collector + Clone + Send + Sync + 'static,
{
    registry
        .register(Box::new(metric))
        .expect("metric registration should succeed");
}
