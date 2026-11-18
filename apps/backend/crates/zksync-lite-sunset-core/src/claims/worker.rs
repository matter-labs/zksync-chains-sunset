use crate::claim_gateway::{
    BroadcastError, ClaimGatewayItem, allocate_sponsor_nonce, broadcast_claim_multicall,
    reset_sponsor_nonce,
};
use crate::observability::claim_finalization_result_label;
use crate::world::World;
use alloy::primitives::{Address, B256};
use sqlx::Row;
use tokio::sync::watch;
use tokio::time::{Duration, Instant, sleep};
use tracing::{debug, error, info, warn};

use super::finalize::finalize_once;
use super::shared::{
    ClaimError, ClaimFailureCode, ClaimRequestPayload, ClaimStatus, claim_contract,
    parse_payload_json,
};

/// Drives the sponsor-claim background loop. Each tick performs two sweeps:
///
/// 1. **Broadcast drain** — picks pending rows whose `next_attempt_at` is due,
///    broadcasts the sponsor multicall, and advances them to `submitted` (or
///    retries / fails terminally).
/// 2. **Finalization sweep** — picks `submitted`/`mining` rows on a slower
///    cadence (`finalize_poll_interval_ms`) and resolves them to a terminal
///    `mined_*` state by checking the L1 receipt.
///
/// The loop exits cleanly when the supplied shutdown channel flips to `true`.
/// Multi-replica deployments must run this on exactly one instance — sponsor
/// nonce allocation in the broadcast drain is in-process and only safe under
/// that constraint.
pub async fn run(world: World, mut shutdown: watch::Receiver<bool>) {
    let cfg = world.config().claims.worker.clone();
    if !cfg.enabled {
        info!(component = "claim-worker", "worker disabled via config");
        return;
    }

    info!(
        component = "claim-worker",
        tick_interval_ms = cfg.tick_interval_ms,
        batch_size = cfg.batch_size,
        max_attempts = cfg.max_attempts,
        backoff_cap_seconds = cfg.backoff_cap_seconds,
        finalize_poll_interval_ms = cfg.finalize_poll_interval_ms,
        "starting claim broadcast worker",
    );

    let finalize_interval = Duration::from_millis(cfg.finalize_poll_interval_ms);
    // Run the first finalize sweep immediately on startup so any backlog of
    // `submitted`/`mining` rows left behind by a previous process is picked up
    // straight away rather than waiting a full interval.
    let mut next_finalize_at = Instant::now();

    loop {
        if *shutdown.borrow() {
            break;
        }

        world.observability().record_claim_worker_heartbeat();

        let drained = match drain_once(&world, cfg.batch_size, cfg.max_attempts).await {
            Ok(n) => n,
            Err(err) => {
                error!(component = "claim-worker", error = %err, "broadcast drain failed");
                0
            }
        };

        let now = Instant::now();
        let finalize_due = now >= next_finalize_at;
        let finalized = if finalize_due {
            next_finalize_at = now + finalize_interval;
            match finalize_once(&world, cfg.batch_size).await {
                Ok(n) => n,
                Err(err) => {
                    error!(component = "claim-worker", error = %err, "finalize sweep failed");
                    0
                }
            }
        } else {
            0
        };

        if drained == 0 && finalized == 0 {
            // Wake up for whichever comes first: the next broadcast tick or
            // the next finalize sweep.
            let until_finalize = next_finalize_at.saturating_duration_since(Instant::now());
            let sleep_for = Duration::from_millis(cfg.tick_interval_ms).min(until_finalize);
            tokio::select! {
                _ = sleep(sleep_for) => {}
                _ = shutdown.changed() => {}
            }
        }
    }

    info!(component = "claim-worker", "claim broadcast worker stopped");
}

async fn drain_once(
    world: &World,
    batch_size: u32,
    max_attempts: u32,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            claim_id,
            user_address,
            attempts,
            assigned_nonce,
            payload_json::text AS payload_json
        FROM claim_requests
        WHERE status = 'pending'
          AND (next_attempt_at IS NULL OR next_attempt_at <= NOW())
        ORDER BY created_at ASC
        LIMIT $1
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(batch_size as i64)
    .fetch_all(world.db_pool())
    .await?;

    let count = rows.len();
    for row in rows {
        let claim_id: String = row.try_get("claim_id")?;
        let user_address: String = row.try_get("user_address")?;
        let attempts: i32 = row.try_get("attempts")?;
        let assigned_nonce: Option<i64> = row.try_get("assigned_nonce")?;
        let payload_json: String = row.try_get("payload_json")?;

        if let Err(err) = process_row(
            world,
            &claim_id,
            &user_address,
            attempts as u32,
            assigned_nonce.map(|n| n as u64),
            &payload_json,
            max_attempts,
        )
        .await
        {
            error!(
                component = "claim-worker",
                claim_id = %claim_id,
                error = %err,
                "failed to process pending claim",
            );
        }
    }

    Ok(count)
}

async fn process_row(
    world: &World,
    claim_id: &str,
    user_address: &str,
    attempts: u32,
    assigned_nonce: Option<u64>,
    payload_json: &str,
    max_attempts: u32,
) -> Result<(), ClaimError> {
    let claimant: Address = user_address
        .parse()
        .map_err(|_| ClaimError::storage(format!("invalid user_address `{user_address}`")))?;
    let payload: ClaimRequestPayload = parse_payload_json(payload_json)?;
    let claim_contract = claim_contract(world)?;
    let gateway_items = ClaimGatewayItem::from_batch_items(&payload.items);

    // Pin the sponsor nonce on the very first attempt and persist it before any
    // broadcast, so retries reuse the same slot. Reusing the slot avoids gaps
    // in the sponsor nonce sequence (which would stall every later sponsor tx
    // until the missing nonce mines) and stops a transient failure from
    // double-spending the claim — if the original broadcast quietly made it
    // into the mempool, the retry collides on the same nonce instead of
    // landing a second tx that reverts post-claim.
    let (nonce, freshly_allocated) = match assigned_nonce {
        Some(n) => (n, false),
        None => {
            let allocated = allocate_sponsor_nonce(world)
                .await
                .map_err(ClaimError::onchain_rpc)?;
            persist_assigned_nonce(world, claim_id, allocated as i64).await?;
            (allocated, true)
        }
    };

    debug!(
        component = "claim-worker",
        claim_id,
        attempt = attempts + 1,
        nonce,
        reused_nonce = !freshly_allocated,
        "broadcasting claim"
    );

    match broadcast_claim_multicall(world, claim_contract, claimant, &gateway_items, nonce).await {
        Ok(tx_hash) => mark_submitted(world, claim_id, tx_hash, nonce as i64).await,
        Err(BroadcastError::Retryable { detail }) => {
            // Keep the persisted nonce; the next attempt will reuse it. We do
            // NOT reset the in-memory sponsor counter here — the slot is still
            // in flight (either queued, mempool-stuck, or about to be re-sent),
            // so other claims should continue allocating subsequent nonces.
            handle_failure(
                world,
                claim_id,
                attempts,
                max_attempts,
                detail,
                /* terminal = */ false,
            )
            .await
        }
        Err(BroadcastError::Terminal { detail }) => {
            // The chain rejected the call (revert / bad calldata / etc.). The
            // assigned slot may or may not have been consumed depending on the
            // specific error, so reseed the sponsor counter from chain on the
            // next allocation rather than keeping a possibly-stale local value.
            // Only meaningful when this attempt freshly allocated — for retry
            // attempts the cache was already advanced past this slot.
            if freshly_allocated {
                reset_sponsor_nonce(world).await;
            }
            handle_failure(
                world,
                claim_id,
                attempts,
                max_attempts,
                detail,
                /* terminal = */ true,
            )
            .await
        }
    }
}

async fn persist_assigned_nonce(
    world: &World,
    claim_id: &str,
    nonce: i64,
) -> Result<(), ClaimError> {
    sqlx::query(
        r#"
        UPDATE claim_requests
        SET assigned_nonce = $2, updated_at = NOW()
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id)
    .bind(nonce)
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;
    Ok(())
}

async fn mark_submitted(
    world: &World,
    claim_id: &str,
    tx_hash: B256,
    nonce: i64,
) -> Result<(), ClaimError> {
    sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            status = $2,
            tx_hash = $3,
            assigned_nonce = $4,
            submitted_at = NOW(),
            error = NULL,
            error_code = NULL,
            error_detail = NULL,
            next_attempt_at = NULL,
            updated_at = NOW()
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id)
    .bind(ClaimStatus::Submitted.as_db())
    .bind(format!("{tx_hash:#x}"))
    .bind(nonce)
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    info!(
        component = "claim-worker",
        claim_id, %tx_hash, "claim broadcast submitted"
    );
    Ok(())
}

async fn handle_failure(
    world: &World,
    claim_id: &str,
    attempts: u32,
    max_attempts: u32,
    detail: String,
    terminal: bool,
) -> Result<(), ClaimError> {
    let next_attempts = attempts + 1;
    let exhausted = next_attempts >= max_attempts;
    if terminal || exhausted {
        let reason = if terminal {
            "terminal broadcast error"
        } else {
            "broadcast retries exhausted"
        };
        warn!(
            component = "claim-worker",
            claim_id,
            attempts = next_attempts,
            error = %detail,
            "{reason}; marking claim as broadcast_failed",
        );
        let failure = ClaimFailureCode::BroadcastFailed;
        sqlx::query(
            r#"
            UPDATE claim_requests
            SET
                status = $2,
                attempts = $3,
                next_attempt_at = NULL,
                error_code = $4,
                error_detail = $5,
                error = $5,
                updated_at = NOW()
            WHERE claim_id = $1
            "#,
        )
        .bind(claim_id)
        .bind(ClaimStatus::MinedReverted.as_db())
        .bind(next_attempts as i32)
        .bind(failure.as_db())
        .bind(detail)
        .execute(world.db_pool())
        .await
        .map_err(ClaimError::storage)?;
        world
            .observability()
            .observe_claim_finalization(claim_finalization_result_label(Some(failure)), 0.0);
        return Ok(());
    }

    let cap = world.config().claims.worker.backoff_cap_seconds;
    let delay_seconds = backoff_seconds(next_attempts, cap);
    warn!(
        component = "claim-worker",
        claim_id,
        attempts = next_attempts,
        delay_seconds,
        error = %detail,
        "broadcast failed; rescheduling",
    );
    sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            attempts = $2,
            next_attempt_at = NOW() + ($3::BIGINT * INTERVAL '1 second'),
            error_detail = $4,
            updated_at = NOW()
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id)
    .bind(next_attempts as i32)
    .bind(delay_seconds as i64)
    .bind(detail)
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;
    Ok(())
}

fn backoff_seconds(attempt: u32, cap_seconds: u64) -> u64 {
    // 2^attempt with the configured cap. attempt=1 → 2s, 2 → 4s, 3 → 8s, …
    let exp = attempt.min(20);
    let raw = 1u64.checked_shl(exp).unwrap_or(u64::MAX);
    raw.min(cap_seconds.max(1))
}

#[cfg(test)]
mod tests {
    use super::backoff_seconds;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_seconds(1, 60), 2);
        assert_eq!(backoff_seconds(2, 60), 4);
        assert_eq!(backoff_seconds(3, 60), 8);
        assert_eq!(backoff_seconds(4, 60), 16);
        assert_eq!(backoff_seconds(5, 60), 32);
        assert_eq!(backoff_seconds(6, 60), 60);
        assert_eq!(backoff_seconds(20, 60), 60);
    }

    #[test]
    fn backoff_respects_cap_one() {
        assert_eq!(backoff_seconds(5, 1), 1);
    }
}
