use crate::world::World;
use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::shared::{
    ClaimBatchItem, ClaimError, ClaimRequestPayload, ClaimStatus, canonical_claim_items,
    claim_contract, compute_claims_hash, current_unix_timestamp, parse_payload_json,
};
use super::signature::{ClaimBatchSignatureVerification, verify_claim_batch_signature};
use super::status::{ClaimResponse, load_claim_record, to_claim_response};

/// Pinned server-side nonce for signature verification.
///
/// The on-chain nonce machinery was removed together with `claimManyOnBehalf`.
/// Replay is prevented by the per-leaf bitmap on the distributor plus the
/// `claim_requests` row state transition, so the nonce value in the signed
/// payload is fixed and rejected if it deviates.
const EXPECTED_CLAIM_NONCE: u64 = 0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimSubmitRequest {
    pub address: Address,
    pub items: Vec<ClaimBatchItem>,
    pub claims_hash: B256,
    pub nonce: u64,
    pub expiry: u64,
    pub signature: String,
}

pub async fn submit_claim(
    world: &World,
    req: ClaimSubmitRequest,
) -> Result<ClaimResponse, ClaimError> {
    if current_unix_timestamp() > req.expiry {
        return Err(ClaimError::ExpiredSignature);
    }

    if req.nonce != EXPECTED_CLAIM_NONCE {
        return Err(ClaimError::InvalidNonce {
            expected: EXPECTED_CLAIM_NONCE,
            got: req.nonce,
        });
    }

    let verifying_contract = claim_contract(world)?;
    let canonical_items = canonical_claim_items(req.items)?;
    let computed_hash = compute_claims_hash(&canonical_items);
    if req.claims_hash != computed_hash {
        return Err(ClaimError::InvalidClaimsHash);
    }

    verify_claim_batch_signature(ClaimBatchSignatureVerification {
        world,
        claimant: req.address,
        claims_hash: computed_hash,
        expiry: req.expiry,
        nonce: req.nonce,
        chain_id: world.config().auth.claim_chain_id,
        verifying_contract,
        signature: &req.signature,
    })
    .await?;

    let mut tx = world.db_pool().begin().await.map_err(ClaimError::storage)?;

    let prepared_row = sqlx::query(
        r#"
        SELECT claim_id, payload_json::text AS payload_json
        FROM claim_requests
        WHERE user_address = $1
          AND status = $2
          AND claims_hash = $3
          AND nonce = $4
          AND expiry = $5
        ORDER BY prepared_at DESC NULLS LAST, created_at DESC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(format!("{:#x}", req.address))
    .bind(ClaimStatus::Prepared.as_db())
    .bind(format!("{:#x}", req.claims_hash))
    .bind(req.nonce as i64)
    .bind(req.expiry as i64)
    .fetch_optional(tx.as_mut())
    .await
    .map_err(ClaimError::storage)?;

    let Some(prepared_row) = prepared_row else {
        return Err(ClaimError::InvalidClaimsHash);
    };

    let claim_id: String = prepared_row
        .try_get("claim_id")
        .map_err(ClaimError::storage)?;
    let payload_json: String = prepared_row
        .try_get("payload_json")
        .map_err(ClaimError::storage)?;
    let payload = parse_payload_json(&payload_json)?;
    if payload
        != (ClaimRequestPayload {
            items: canonical_items.clone(),
        })
    {
        return Err(ClaimError::InvalidClaimOrdering);
    }

    // Persist the row as `pending` and return immediately. The background
    // claim-broadcast worker picks pending rows up, signs and broadcasts the
    // sponsor multicall, and transitions the row to `submitted`. This keeps
    // the HTTP handler off the L1 RPC critical path so a tab close (or any
    // mid-flight cancellation) can't strand a half-broadcast claim.
    sqlx::query(
        r#"
        UPDATE claim_requests
        SET
            status = $2,
            signature = $3,
            attempts = 0,
            next_attempt_at = NULL,
            tx_hash = NULL,
            assigned_nonce = NULL,
            submitted_at = NULL,
            error = NULL,
            error_code = NULL,
            error_detail = NULL,
            updated_at = NOW()
        WHERE claim_id = $1
        "#,
    )
    .bind(&claim_id)
    .bind(ClaimStatus::Pending.as_db())
    .bind(req.signature)
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
