use crate::claim_gateway;
use crate::observability::claim_finalization_result_label;
use crate::world::World;
use alloy::network::ReceiptResponse;
use alloy::primitives::B256;
use alloy::providers::Provider;
use sqlx::Row;
use tracing::error;

use super::shared::{
    ClaimError, ClaimFailureCode, ClaimId, ClaimRequestPayload, ClaimStatus, claim_contract,
    current_unix_timestamp, parse_payload_json,
};

/// One sweep across rows eligible for finalization. The worker decides _when_
/// to run this (see [`super::worker::run`]) and we just process whatever is
/// in flight right now — we deliberately do **not** persist any per-row
/// "next check" cursor. The expected backlog is small (sunset traffic), so
/// re-checking every in-flight row each sweep is cheap and avoids carrying
/// extra schema for purely-cosmetic scheduling.
pub(crate) async fn finalize_once(world: &World, batch_size: u32) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT claim_id
        FROM claim_requests
        WHERE status IN ('submitted', 'mining')
        ORDER BY updated_at ASC
        LIMIT $1
        "#,
    )
    .bind(batch_size as i64)
    .fetch_all(world.db_pool())
    .await?;

    let count = rows.len();
    for row in rows {
        let claim_id_str: String = row.try_get("claim_id")?;
        let claim_id = match claim_id_str.parse::<ClaimId>() {
            Ok(id) => id,
            Err(_) => {
                error!(
                    component = "claim-worker",
                    claim_id = %claim_id_str,
                    "skipping finalize sweep: invalid claim_id in db",
                );
                continue;
            }
        };

        if let Err(err) = finalize_row(world, &claim_id).await {
            error!(
                component = "claim-worker",
                claim_id = %claim_id,
                error = %err,
                "failed to finalize claim",
            );
        }
    }

    Ok(count)
}

async fn finalize_row(world: &World, claim_id: &ClaimId) -> Result<(), ClaimError> {
    let Some(record) = load_finalize_record(world, claim_id).await? else {
        // Row terminalized between SELECT and our follow-up read.
        return Ok(());
    };

    if record.status == ClaimStatus::Submitted {
        bump_submitted_to_mining(world, claim_id).await?;
    }

    finalize_claim_if_ready(world, &record).await
}

#[derive(Debug, Clone)]
struct FinalizeRecord {
    claim_id: ClaimId,
    status: ClaimStatus,
    tx_hash: Option<B256>,
    payload: ClaimRequestPayload,
}

async fn load_finalize_record(
    world: &World,
    claim_id: &ClaimId,
) -> Result<Option<FinalizeRecord>, ClaimError> {
    let row = sqlx::query(
        r#"
        SELECT
            status,
            tx_hash,
            payload_json::text AS payload_json
        FROM claim_requests
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id.as_str())
    .fetch_optional(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let status: String = row.try_get("status").map_err(ClaimError::storage)?;
    let status = ClaimStatus::from_db(&status)?;
    if !matches!(status, ClaimStatus::Submitted | ClaimStatus::Mining) {
        return Ok(None);
    }

    let tx_hash: Option<String> = row.try_get("tx_hash").map_err(ClaimError::storage)?;
    let payload_json: String = row.try_get("payload_json").map_err(ClaimError::storage)?;
    Ok(Some(FinalizeRecord {
        claim_id: claim_id.clone(),
        status,
        tx_hash: tx_hash
            .map(|value| value.parse::<B256>())
            .transpose()
            .map_err(|_| ClaimError::storage("invalid tx_hash in claim_requests"))?,
        payload: parse_payload_json(&payload_json)?,
    }))
}

async fn bump_submitted_to_mining(world: &World, claim_id: &ClaimId) -> Result<(), ClaimError> {
    sqlx::query(
        r#"
        UPDATE claim_requests
        SET status = $2, updated_at = NOW()
        WHERE claim_id = $1 AND status = $3
        "#,
    )
    .bind(claim_id.as_str())
    .bind(ClaimStatus::Mining.as_db())
    .bind(ClaimStatus::Submitted.as_db())
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;
    Ok(())
}

async fn finalize_claim_if_ready(
    world: &World,
    request: &FinalizeRecord,
) -> Result<(), ClaimError> {
    let Some(tx_hash) = request.tx_hash else {
        // No tx_hash yet — the broadcast worker hasn't recorded it.
        return Ok(());
    };

    let receipt = world
        .l1_provider()
        .get_transaction_receipt(tx_hash)
        .await
        .map_err(ClaimError::onchain_rpc)?;

    let Some(receipt) = receipt else {
        maybe_mark_timeout(world, &request.claim_id).await?;
        return Ok(());
    };

    let receipt_block_number = receipt.block_number().unwrap_or_default();
    let current_block = world
        .l1_provider()
        .get_block_number()
        .await
        .map_err(ClaimError::onchain_rpc)?;
    let confirmations = current_block.saturating_sub(receipt_block_number) + 1;
    if confirmations < world.config().execution.min_confirmations {
        return Ok(());
    }

    let tx_success = receipt.status();
    let reconciliation_error = if tx_success {
        reconcile_claim_state(world, request).await?
    } else {
        None
    };
    let state_verified = reconciliation_error.is_none();
    let next_status = if tx_success && state_verified {
        ClaimStatus::MinedSuccess
    } else {
        ClaimStatus::MinedReverted
    };
    let failure = match (tx_success, state_verified) {
        (false, _) => Some(ClaimFailureCode::TxReverted),
        (true, false) => Some(ClaimFailureCode::OnchainStateMismatch),
        (true, true) => None,
    };
    let failure_detail = match (tx_success, reconciliation_error) {
        (false, _) => Some("Claim transaction reverted".to_string()),
        (true, Some(detail)) => Some(detail),
        (true, None) => None,
    };
    let finalization_duration_seconds = submitted_elapsed_seconds(world, &request.claim_id).await?;

    let block_hash = receipt.block_hash().unwrap_or(B256::ZERO);
    let mut tx = world.db_pool().begin().await.map_err(ClaimError::storage)?;

    // The status guard prevents a stale finalize from regressing a row that
    // another path (e.g. a previous sweep race) already terminalized.
    let updated = sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            status = $2,
            block_number = $3,
            block_hash = $4,
            error_code = $5,
            error_detail = $6,
            error = $6,
            updated_at = NOW()
        WHERE claim_id = $1 AND status IN ($7, $8)
        "#,
    )
    .bind(request.claim_id.as_str())
    .bind(next_status.as_db())
    .bind(receipt_block_number as i64)
    .bind(format!("{block_hash:#x}"))
    .bind(failure.map(ClaimFailureCode::as_db))
    .bind(failure_detail)
    .bind(ClaimStatus::Submitted.as_db())
    .bind(ClaimStatus::Mining.as_db())
    .execute(tx.as_mut())
    .await
    .map_err(ClaimError::storage)?;

    tx.commit().await.map_err(ClaimError::storage)?;

    if updated.rows_affected() > 0 {
        world.observability().observe_claim_finalization(
            claim_finalization_result_label(failure),
            finalization_duration_seconds,
        );
    }

    Ok(())
}

async fn reconcile_claim_state(
    world: &World,
    request: &FinalizeRecord,
) -> Result<Option<String>, ClaimError> {
    let claim_contract = claim_contract(world)?;
    for item in &request.payload.items {
        let is_claimed = claim_gateway::query_is_claimed(world, claim_contract, item.index)
            .await
            .map_err(ClaimError::onchain_rpc)?;
        if !is_claimed {
            return Ok(Some(format!(
                "claim tx succeeded but claim index is not marked claimed onchain: {}",
                item.index
            )));
        }
    }

    Ok(None)
}

async fn maybe_mark_timeout(world: &World, claim_id: &ClaimId) -> Result<(), ClaimError> {
    let finalization_duration_seconds = submitted_elapsed_seconds(world, claim_id).await?;
    if finalization_duration_seconds < world.config().execution.receipt_timeout_seconds as f64 {
        return Ok(());
    }

    let result = sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            status = $2,
            error_code = $3,
            error_detail = $4,
            error = $4,
            updated_at = NOW()
        WHERE claim_id = $1 AND status IN ($5, $6)
        "#,
    )
    .bind(claim_id.as_str())
    .bind(ClaimStatus::MinedReverted.as_db())
    .bind(ClaimFailureCode::ReceiptTimeout.as_db())
    .bind(ClaimFailureCode::ReceiptTimeout.public_message())
    .bind(ClaimStatus::Submitted.as_db())
    .bind(ClaimStatus::Mining.as_db())
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    if result.rows_affected() > 0 {
        world.observability().observe_claim_receipt_timeout();
        world.observability().observe_claim_finalization(
            claim_finalization_result_label(Some(ClaimFailureCode::ReceiptTimeout)),
            finalization_duration_seconds,
        );
    }

    Ok(())
}

async fn submitted_elapsed_seconds(world: &World, claim_id: &ClaimId) -> Result<f64, ClaimError> {
    let submitted_at_epoch = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT EXTRACT(EPOCH FROM submitted_at)::BIGINT FROM claim_requests WHERE claim_id = $1",
    )
    .bind(claim_id.as_str())
    .fetch_one(world.db_pool())
    .await
    .map_err(ClaimError::storage)?
    .unwrap_or(0) as u64;

    let now = current_unix_timestamp();
    Ok(now.saturating_sub(submitted_at_epoch) as f64)
}
