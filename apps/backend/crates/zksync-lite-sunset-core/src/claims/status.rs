use crate::world::World;
use alloy::primitives::B256;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::shared::{
    ClaimError, ClaimFailureCode, ClaimId, ClaimReceipt, ClaimRequestRecord, ClaimStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimResponse {
    pub claim_id: ClaimId,
    pub status: ClaimStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<B256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ClaimReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Pure DB read. All state transitions for `submitted`/`mining` rows are owned
/// by the background claim worker (see `claims::finalize::finalize_once`), so
/// closing the browser tab no longer leaves rows stuck mid-flight.
pub async fn get_claim_status(
    world: &World,
    claim_id: &ClaimId,
) -> Result<ClaimResponse, ClaimError> {
    let record = load_claim_record(world, claim_id).await?;
    to_claim_response(record)
}

pub(crate) async fn load_claim_record(
    world: &World,
    claim_id: &ClaimId,
) -> Result<ClaimRequestRecord, ClaimError> {
    let row = sqlx::query(
        r#"
        SELECT
            claim_id,
            user_address,
            status,
            tx_hash,
            block_number,
            block_hash,
            error,
            error_code,
            error_detail
        FROM claim_requests
        WHERE claim_id = $1
        "#,
    )
    .bind(claim_id.as_str())
    .fetch_optional(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    let Some(row) = row else {
        return Err(ClaimError::ClaimNotFound {
            claim_id: claim_id.clone(),
        });
    };

    let claim_id: String = row.try_get("claim_id").map_err(ClaimError::storage)?;
    let status: String = row.try_get("status").map_err(ClaimError::storage)?;
    let tx_hash: Option<String> = row.try_get("tx_hash").map_err(ClaimError::storage)?;
    let block_number: Option<i64> = row.try_get("block_number").map_err(ClaimError::storage)?;
    let block_hash: Option<String> = row.try_get("block_hash").map_err(ClaimError::storage)?;
    let error: Option<String> = row.try_get("error").map_err(ClaimError::storage)?;
    let error_code: Option<String> = row.try_get("error_code").map_err(ClaimError::storage)?;
    let error_detail: Option<String> = row.try_get("error_detail").map_err(ClaimError::storage)?;
    let failure = error_code
        .as_deref()
        .map(ClaimFailureCode::from_db)
        .transpose()?;

    Ok(ClaimRequestRecord {
        claim_id: claim_id
            .parse()
            .map_err(|_| ClaimError::storage(format!("invalid claim_id in db: {claim_id}")))?,
        status: ClaimStatus::from_db(&status)?,
        tx_hash: tx_hash
            .map(|value| value.parse::<B256>())
            .transpose()
            .map_err(|_| ClaimError::storage("invalid tx_hash in claim_requests"))?,
        block_number: block_number.map(|value| value as u64),
        block_hash: block_hash
            .map(|value| value.parse::<B256>())
            .transpose()
            .map_err(|_| ClaimError::storage("invalid block_hash in claim_requests"))?,
        failure,
        failure_detail: error_detail.or(error),
    })
}

pub(crate) fn to_claim_response(record: ClaimRequestRecord) -> Result<ClaimResponse, ClaimError> {
    let receipt = match (record.block_number, record.block_hash) {
        (Some(block_number), Some(block_hash)) => Some(ClaimReceipt {
            block_number,
            block_hash,
        }),
        _ => None,
    };
    let error = record
        .failure
        .map(|failure| format_failure_message(failure, record.tx_hash))
        .or_else(|| {
            record
                .failure_detail
                .as_ref()
                .map(|_| "Claim failed".to_string())
        });
    let error_code = record.failure.map(|failure| failure.as_db().to_string());

    Ok(ClaimResponse {
        claim_id: record.claim_id,
        status: record.status,
        tx_hash: record.tx_hash,
        receipt,
        error_code,
        error,
    })
}

fn format_failure_message(failure: ClaimFailureCode, tx_hash: Option<B256>) -> String {
    let base = failure.public_message();
    match tx_hash {
        Some(hash) => format!("{base} (tx: {hash:#x})"),
        None => base.to_string(),
    }
}
