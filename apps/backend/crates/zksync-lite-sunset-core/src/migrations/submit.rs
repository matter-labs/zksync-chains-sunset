use crate::migration_gateway;
use crate::world::World;
use alloy::{
    consensus::Transaction,
    network::TransactionResponse,
    primitives::{Address, B256},
    providers::Provider,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::shared::{MigrationError, MigrationId, MigrationStatus, parse_payload_json};
use super::status::{MigrationResponse, load_migration_record, to_migration_response};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSubmitRequest {
    pub migration_id: MigrationId,
    pub tx_hash: B256,
}

pub async fn submit_migration(
    world: &World,
    req: MigrationSubmitRequest,
) -> Result<MigrationResponse, MigrationError> {
    let contract = super::shared::migration_contract(world)?;

    let mut tx = world
        .db_pool()
        .begin()
        .await
        .map_err(MigrationError::storage)?;

    let prepared_row = sqlx::query(
        r#"
        SELECT migration_id, user_address, payload_json::text AS payload_json
        FROM migration_requests
        WHERE migration_id = $1
          AND status = $2
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(req.migration_id.as_str())
    .bind(MigrationStatus::Prepared.as_db())
    .fetch_optional(tx.as_mut())
    .await
    .map_err(MigrationError::storage)?;

    let Some(prepared_row) = prepared_row else {
        return Err(MigrationError::MigrationNotFound {
            migration_id: req.migration_id,
        });
    };

    let migration_id: String = prepared_row
        .try_get("migration_id")
        .map_err(MigrationError::storage)?;
    let payload_json: String = prepared_row
        .try_get("payload_json")
        .map_err(MigrationError::storage)?;
    let expected_user: Address = prepared_row
        .try_get::<String, _>("user_address")
        .map_err(MigrationError::storage)?
        .parse()
        .map_err(|_| MigrationError::storage("invalid user_address in migration_requests"))?;
    let payload = parse_payload_json(&payload_json)?;

    verify_submitted_transaction(world, contract, req.tx_hash, &payload, expected_user).await?;

    sqlx::query(
        r#"
        UPDATE migration_requests
        SET
            status = $2,
            tx_hash = $3,
            submitted_at = NOW(),
            error = NULL,
            error_code = NULL,
            error_detail = NULL,
            updated_at = NOW()
        WHERE migration_id = $1
        "#,
    )
    .bind(&migration_id)
    .bind(MigrationStatus::Submitted.as_db())
    .bind(format!("{:#x}", req.tx_hash))
    .execute(tx.as_mut())
    .await
    .map_err(MigrationError::storage)?;

    tx.commit().await.map_err(MigrationError::storage)?;

    let record = load_migration_record(
        world,
        &migration_id.parse().map_err(|_| {
            MigrationError::storage(format!("invalid migration id stored in db: {migration_id}"))
        })?,
    )
    .await?;
    to_migration_response(world, record).await
}

async fn verify_submitted_transaction(
    world: &World,
    contract: Address,
    tx_hash: B256,
    payload: &super::shared::MigrationRequestPayload,
    expected_user: Address,
) -> Result<(), MigrationError> {
    let tx = world
        .l1_provider()
        .get_transaction_by_hash(tx_hash)
        .await
        .map_err(MigrationError::onchain_rpc)?
        .ok_or(MigrationError::SubmittedTransactionNotFound { tx_hash })?;

    if tx.to() != Some(contract) {
        return Err(MigrationError::SubmittedTransactionMismatch {
            detail: format!("expected destination {contract:#x}, got {:?}", tx.to()),
        });
    }

    let user_address = tx.from();
    if user_address != expected_user {
        return Err(MigrationError::SubmittedTransactionMismatch {
            detail: format!("expected sender {expected_user:#x}, got {user_address:#x}"),
        });
    }

    let expected_input =
        migration_gateway::encode_migrate_token_calldata(payload.item.token_address);

    if tx.input() != &expected_input {
        return Err(MigrationError::SubmittedTransactionMismatch {
            detail: format!("unexpected calldata for submitted migration tx {tx_hash:#x}"),
        });
    }

    Ok(())
}
