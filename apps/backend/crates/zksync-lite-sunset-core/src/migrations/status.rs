use crate::migration_gateway;
use crate::world::World;
use alloy::network::ReceiptResponse;
use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::shared::{
    MigrationError, MigrationId, MigrationReceipt, MigrationRequestRecord, MigrationStatus,
    current_unix_timestamp, migration_contract, parse_payload_json,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResponse {
    pub migration_id: MigrationId,
    pub status: MigrationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<B256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<MigrationReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<MigrationFailureStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<MigrationDeliveryDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationFailureStage {
    L1Execution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationDeliveryPath {
    DirectL1ClaimContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationDeliveryDetails {
    pub token_address: Address,
    pub path: MigrationDeliveryPath,
    pub claim_distributor_address: Address,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReconciliationFailure {
    code: &'static str,
    detail: String,
}

pub async fn get_migration_status(
    world: &World,
    migration_id: &MigrationId,
) -> Result<MigrationResponse, MigrationError> {
    let mut current = load_migration_record(world, migration_id).await?;

    if current.status == MigrationStatus::Submitted {
        sqlx::query(
            r#"
            UPDATE migration_requests
            SET status = $2, updated_at = NOW()
            WHERE migration_id = $1 AND status = $3
            "#,
        )
        .bind(migration_id.as_str())
        .bind(MigrationStatus::Mining.as_db())
        .bind(MigrationStatus::Submitted.as_db())
        .execute(world.db_pool())
        .await
        .map_err(MigrationError::storage)?;
        current = load_migration_record(world, migration_id).await?;
    }

    if current.status == MigrationStatus::Mining {
        finalize_migration_if_ready(world, &current).await?;
        current = load_migration_record(world, migration_id).await?;
    }

    to_migration_response(world, current).await
}

pub(crate) async fn load_migration_record(
    world: &World,
    migration_id: &MigrationId,
) -> Result<MigrationRequestRecord, MigrationError> {
    let row = sqlx::query(
        r#"
        SELECT
            migration_id,
            status,
            tx_hash,
            block_number,
            block_hash,
            error,
            error_code,
            error_detail,
            payload_json::text AS payload_json
        FROM migration_requests
        WHERE migration_id = $1
        "#,
    )
    .bind(migration_id.as_str())
    .fetch_optional(world.db_pool())
    .await
    .map_err(MigrationError::storage)?;

    let Some(row) = row else {
        return Err(MigrationError::MigrationNotFound {
            migration_id: migration_id.clone(),
        });
    };

    let migration_id: String = row
        .try_get("migration_id")
        .map_err(MigrationError::storage)?;
    let status: String = row.try_get("status").map_err(MigrationError::storage)?;
    let tx_hash: Option<String> = row.try_get("tx_hash").map_err(MigrationError::storage)?;
    let block_number: Option<i64> = row
        .try_get("block_number")
        .map_err(MigrationError::storage)?;
    let block_hash: Option<String> = row.try_get("block_hash").map_err(MigrationError::storage)?;
    let error: Option<String> = row.try_get("error").map_err(MigrationError::storage)?;
    let error_code: Option<String> = row.try_get("error_code").map_err(MigrationError::storage)?;
    let payload_json: String = row
        .try_get("payload_json")
        .map_err(MigrationError::storage)?;

    Ok(MigrationRequestRecord {
        migration_id: migration_id.parse().map_err(|_| {
            MigrationError::storage(format!("invalid migration id in db: {migration_id}"))
        })?,
        status: MigrationStatus::from_db(&status)?,
        tx_hash: tx_hash
            .map(|value| value.parse())
            .transpose()
            .map_err(|_| MigrationError::storage("invalid tx_hash in migration_requests"))?,
        block_number: block_number.map(|value| value as u64),
        block_hash: block_hash
            .map(|value| value.parse())
            .transpose()
            .map_err(|_| MigrationError::storage("invalid block_hash in migration_requests"))?,
        error,
        error_code,
        payload: parse_payload_json(&payload_json)?,
    })
}

pub(crate) async fn to_migration_response(
    world: &World,
    record: MigrationRequestRecord,
) -> Result<MigrationResponse, MigrationError> {
    let failure_stage = infer_failure_stage(record.error_code.as_deref());
    let receipt = match (record.block_number, record.block_hash) {
        (Some(block_number), Some(block_hash)) => Some(MigrationReceipt {
            block_number,
            block_hash,
        }),
        _ => None,
    };
    let delivery = load_migration_delivery_details(world, &record).await;

    Ok(MigrationResponse {
        migration_id: record.migration_id,
        status: record.status,
        tx_hash: record.tx_hash,
        receipt,
        error_code: record.error_code,
        error: record.error,
        failure_stage,
        delivery,
    })
}

fn infer_failure_stage(error_code: Option<&str>) -> Option<MigrationFailureStage> {
    match error_code {
        Some("TX_REVERTED" | "RECEIPT_TIMEOUT" | "L1_MIGRATED_FLAG_MISSING") => {
            Some(MigrationFailureStage::L1Execution)
        }
        _ => None,
    }
}

async fn load_migration_delivery_details(
    world: &World,
    record: &MigrationRequestRecord,
) -> Option<MigrationDeliveryDetails> {
    // Prefer the config-supplied claim distributor address: it's set once at
    // deploy time and identical across every status response. Only fall back
    // to an on-chain probe when the config is missing — that's a
    // misconfiguration, not the steady state.
    let claim_distributor_address = match world.config().execution.claim_contract_address {
        Some(addr) => addr,
        None => {
            let migration_contract = super::shared::migration_contract(world).ok()?;
            migration_gateway::query_l1_claim_distributor(world, migration_contract)
                .await
                .ok()
                .filter(|address| *address != Address::ZERO)?
        }
    };

    Some(MigrationDeliveryDetails {
        token_address: record.payload.item.token_address,
        path: MigrationDeliveryPath::DirectL1ClaimContract,
        claim_distributor_address,
    })
}

async fn finalize_migration_if_ready(
    world: &World,
    request: &MigrationRequestRecord,
) -> Result<(), MigrationError> {
    let Some(tx_hash) = request.tx_hash else {
        return Ok(());
    };

    let receipt = world
        .l1_provider()
        .get_transaction_receipt(tx_hash)
        .await
        .map_err(MigrationError::onchain_rpc)?;

    let Some(receipt) = receipt else {
        maybe_mark_timeout(world, &request.migration_id).await?;
        return Ok(());
    };

    let receipt_block_number = receipt.block_number().unwrap_or_default();
    let current_block = world
        .l1_provider()
        .get_block_number()
        .await
        .map_err(MigrationError::onchain_rpc)?;
    let confirmations = current_block.saturating_sub(receipt_block_number) + 1;
    if confirmations < world.config().execution.min_confirmations {
        return Ok(());
    }

    let tx_success = receipt.status();
    let reconciliation_error = if tx_success {
        reconcile_migration_state(world, request).await?
    } else {
        None
    };
    let state_verified = reconciliation_error.is_none();
    let next_status = if tx_success && state_verified {
        MigrationStatus::MinedSuccess
    } else {
        MigrationStatus::MinedReverted
    };
    let error_code = match (tx_success, reconciliation_error.as_ref()) {
        (false, _) => Some("TX_REVERTED".to_string()),
        (true, Some(failure)) => Some(failure.code.to_string()),
        (true, None) => None,
    };
    let failure_detail = match (tx_success, reconciliation_error) {
        (false, _) => Some("Migration transaction reverted".to_string()),
        (true, Some(failure)) => Some(failure.detail),
        (true, None) => None,
    };

    let block_hash = receipt.block_hash().unwrap_or(B256::ZERO);
    let mut tx = world
        .db_pool()
        .begin()
        .await
        .map_err(MigrationError::storage)?;

    sqlx::query(
        r#"
        UPDATE migration_requests
        SET
            status = $2,
            block_number = $3,
            block_hash = $4,
            error_code = $5,
            error_detail = $6,
            error = $6,
            updated_at = NOW()
        WHERE migration_id = $1
        "#,
    )
    .bind(request.migration_id.as_str())
    .bind(next_status.as_db())
    .bind(receipt_block_number as i64)
    .bind(format!("{block_hash:#x}"))
    .bind(error_code)
    .bind(failure_detail)
    .execute(tx.as_mut())
    .await
    .map_err(MigrationError::storage)?;

    tx.commit().await.map_err(MigrationError::storage)?;

    Ok(())
}

async fn reconcile_migration_state(
    world: &World,
    request: &MigrationRequestRecord,
) -> Result<Option<ReconciliationFailure>, MigrationError> {
    let migration_contract = migration_contract(world)?;
    let item = &request.payload.item;

    let migrated =
        migration_gateway::query_is_token_migrated(world, migration_contract, item.token_address)
            .await
            .map_err(MigrationError::onchain_rpc)?;
    if !migrated {
        return Ok(Some(ReconciliationFailure {
            code: "L1_MIGRATED_FLAG_MISSING",
            detail: format!(
                "migration tx succeeded but token is not marked migrated onchain: {:#x}",
                item.token_address
            ),
        }));
    }

    Ok(None)
}

async fn maybe_mark_timeout(
    world: &World,
    migration_id: &MigrationId,
) -> Result<(), MigrationError> {
    let submitted_at_epoch = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT EXTRACT(EPOCH FROM submitted_at)::BIGINT FROM migration_requests WHERE migration_id = $1",
    )
    .bind(migration_id.as_str())
    .fetch_one(world.db_pool())
    .await
    .map_err(MigrationError::storage)?
    .unwrap_or(0) as u64;

    let now = current_unix_timestamp();
    if now.saturating_sub(submitted_at_epoch) < world.config().execution.receipt_timeout_seconds {
        return Ok(());
    }

    sqlx::query(
        r#"
        UPDATE migration_requests
        SET
            status = $2,
            error_code = $3,
            error_detail = $4,
            error = $4,
            updated_at = NOW()
        WHERE migration_id = $1 AND status IN ($5, $6)
        "#,
    )
    .bind(migration_id.as_str())
    .bind(MigrationStatus::MinedReverted.as_db())
    .bind("RECEIPT_TIMEOUT")
    .bind("Timed out while waiting for migration receipt")
    .bind(MigrationStatus::Submitted.as_db())
    .bind(MigrationStatus::Mining.as_db())
    .execute(world.db_pool())
    .await
    .map_err(MigrationError::storage)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MigrationFailureStage, infer_failure_stage};

    #[test]
    fn infer_failure_stage_maps_execution_failures() {
        assert_eq!(
            infer_failure_stage(Some("TX_REVERTED")),
            Some(MigrationFailureStage::L1Execution)
        );
        assert_eq!(
            infer_failure_stage(Some("RECEIPT_TIMEOUT")),
            Some(MigrationFailureStage::L1Execution)
        );
        assert_eq!(
            infer_failure_stage(Some("L1_MIGRATED_FLAG_MISSING")),
            Some(MigrationFailureStage::L1Execution)
        );
    }

    #[test]
    fn infer_failure_stage_ignores_removed_bridge_failures() {
        assert_eq!(
            infer_failure_stage(Some("BRIDGEHUB_PENDING_MISMATCH")),
            None
        );
        assert_eq!(
            infer_failure_stage(Some("L2_DELIVERY_TX_HASH_MISSING")),
            None
        );
        assert_eq!(infer_failure_stage(Some("UNKNOWN")), None);
    }
}
