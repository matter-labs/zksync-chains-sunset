use crate::claims::direct_verifier::verify_submitted_claim_transaction;
use crate::world::World;
use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::shared::{ClaimError, ClaimId, ClaimStatus, claim_contract, parse_payload_json};
use super::status::{ClaimResponse, load_claim_record, to_claim_response};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitDirectClaimRequest {
    pub claim_id: ClaimId,
    pub tx_hash: B256,
}

/// Records a user-signed claim transaction against a previously prepared claim
/// request. EOA transactions are accepted when sender, destination, and calldata
/// exactly match the prepared direct transaction. Smart-account transactions may
/// wrap that call, so they are accepted by receipt evidence: the submitted
/// transaction must have succeeded and emitted every expected distributor
/// `Claimed` log for the prepared payload. The per-leaf bitmap on the
/// distributor remains the ultimate replay-protection authority; this record is
/// what the status-polling endpoint consumes.
pub async fn submit_direct_claim(
    world: &World,
    req: SubmitDirectClaimRequest,
) -> Result<ClaimResponse, ClaimError> {
    let claim_contract = claim_contract(world)?;

    let mut tx = world.db_pool().begin().await.map_err(ClaimError::storage)?;

    let prepared_row = sqlx::query(
        r#"
        SELECT claim_id, user_address, payload_json::text AS payload_json
        FROM claim_requests
        WHERE claim_id = $1
          AND status = $2
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(req.claim_id.as_str())
    .bind(ClaimStatus::Prepared.as_db())
    .fetch_optional(tx.as_mut())
    .await
    .map_err(ClaimError::storage)?;

    let Some(prepared_row) = prepared_row else {
        return Err(ClaimError::ClaimNotFound {
            claim_id: req.claim_id,
        });
    };

    let claim_id: String = prepared_row
        .try_get("claim_id")
        .map_err(ClaimError::storage)?;
    let payload_json: String = prepared_row
        .try_get("payload_json")
        .map_err(ClaimError::storage)?;
    let expected_user: Address = prepared_row
        .try_get::<String, _>("user_address")
        .map_err(ClaimError::storage)?
        .parse()
        .map_err(|_| ClaimError::storage("invalid user_address in claim_requests"))?;
    let payload = parse_payload_json(&payload_json)?;

    verify_submitted_claim_transaction(world, claim_contract, req.tx_hash, &payload, expected_user)
        .await?;

    sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            status = $2,
            tx_hash = $3,
            submitted_at = NOW(),
            error = NULL,
            error_code = NULL,
            error_detail = NULL,
            updated_at = NOW()
        WHERE claim_id = $1
        "#,
    )
    .bind(&claim_id)
    .bind(ClaimStatus::Submitted.as_db())
    .bind(format!("{:#x}", req.tx_hash))
    .execute(tx.as_mut())
    .await
    .map_err(ClaimError::storage)?;

    tx.commit().await.map_err(ClaimError::storage)?;

    let record = load_claim_record(
        world,
        &claim_id.parse().map_err(|_| {
            ClaimError::storage(format!("invalid claim_id stored in db: {claim_id}"))
        })?,
    )
    .await?;
    to_claim_response(record)
}
