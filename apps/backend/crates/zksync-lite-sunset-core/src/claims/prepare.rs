use crate::claim_gateway::{self, ClaimGatewayItem, PreparedProbeResult};
use crate::signed_batches::{CLAIM_EIP712_DOMAIN_NAME, EIP712_DOMAIN_VERSION};
use crate::world::World;
use alloy::primitives::{Address, B256, Bytes, U256, address};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::shared::{
    ClaimBatchItem, ClaimError, ClaimId, ClaimRequestPayload, claim_contract, compute_claims_hash,
    current_unix_timestamp, serialize_payload,
};

pub const ETH: Address = address!("0x0000000000000000000000000000000000000000");
pub const STETH: Address = address!("0xae7ab96520de3a18e5e111b5eaab095312d7fe84");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPrepareRequest {
    pub address: Address,
    #[serde(default)]
    pub token_address: Option<Address>,
    #[serde(default)]
    pub token_addresses: Vec<Address>,
    #[serde(default)]
    pub all_eligible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypedDataDomain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimTypedDataMessage {
    pub claimant: Address,
    pub claims_hash: B256,
    pub expiry: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimTypedData {
    pub primary_type: String,
    pub domain: TypedDataDomain,
    pub message: ClaimTypedDataMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPrepareResponse {
    pub address: Address,
    pub chain_id: u64,
    pub verifying_contract: Address,
    pub claims_hash: B256,
    pub nonce: u64,
    pub expiry: u64,
    pub items: Vec<ClaimBatchItem>,
    pub typed_data: ClaimTypedData,
    /// Identifier of the persisted claim request. The direct-claim submit
    /// endpoint (`POST /api/claims/direct`) requires this to associate the
    /// user-signed transaction hash with the prepared row.
    pub claim_id: ClaimId,
    /// Transaction the user's wallet must submit to execute the claim without
    /// the sponsored backend path. Single-item batches target the distributor
    /// directly; multi-item batches are routed through Multicall3.
    pub direct_tx: DirectClaimTransaction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectClaimTransaction {
    pub chain_id: u64,
    pub contract_address: Address,
    pub call_data: Bytes,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ActiveClaimRow {
    claim_index: i64,
    token_address: String,
    amount: String,
    proof_hashes: Vec<String>,
}

pub async fn prepare_claim(
    world: &World,
    req: ClaimPrepareRequest,
) -> Result<ClaimPrepareResponse, ClaimError> {
    let chain_id = world.config().auth.claim_chain_id;
    let verifying_contract = claim_contract(world)?;
    let migration_contract = world
        .config()
        .execution
        .migration_contract_address
        .ok_or_else(|| {
            ClaimError::onchain_config("missing execution.migration_contract_address")
        })?;

    let items =
        resolve_prepare_claim_items(world, &req, verifying_contract, migration_contract).await?;
    let claims_hash = compute_claims_hash(&items);
    // Nonce is pinned to zero: the contract no longer tracks per-account nonces
    // (the signature-gated `claimManyOnBehalf` flow was removed). Replay protection
    // comes from the leaf-level bitmap on-chain and from the `claim_requests` row
    // state machine on our side. `nonce` stays in the signed payload so any
    // previously-cached front-end signer still produces a stable digest.
    let nonce = 0u64;
    let expiry =
        current_unix_timestamp().saturating_add(world.config().claims.signature_ttl_seconds);

    let claim_id =
        persist_prepared_claim_request(world, req.address, claims_hash, nonce, expiry, &items)
            .await?;

    let gateway_items = ClaimGatewayItem::from_batch_items(&items);
    let (direct_contract, direct_call_data) = claim_gateway::encode_direct_claim_transaction(
        world,
        verifying_contract,
        req.address,
        &gateway_items,
    );

    Ok(ClaimPrepareResponse {
        address: req.address,
        chain_id,
        verifying_contract,
        claims_hash,
        nonce,
        expiry,
        items: items.clone(),
        typed_data: ClaimTypedData {
            primary_type: "ClaimBatch".to_string(),
            domain: TypedDataDomain {
                name: CLAIM_EIP712_DOMAIN_NAME.to_string(),
                version: EIP712_DOMAIN_VERSION.to_string(),
                chain_id,
                verifying_contract,
            },
            message: ClaimTypedDataMessage {
                claimant: req.address,
                claims_hash,
                expiry,
                nonce,
            },
        },
        claim_id,
        direct_tx: DirectClaimTransaction {
            chain_id,
            contract_address: direct_contract,
            call_data: direct_call_data,
        },
    })
}

async fn resolve_prepare_claim_items(
    world: &World,
    req: &ClaimPrepareRequest,
    claim_distributor: Address,
    migration_contract: Address,
) -> Result<Vec<ClaimBatchItem>, ClaimError> {
    if req.all_eligible {
        let raw = load_active_claim_rows(world, req.address, None).await?;
        if raw.is_empty() {
            return Err(ClaimError::NoClaimsSelected);
        }
        let rows =
            filter_and_verify_rows(world, claim_distributor, migration_contract, raw).await?;
        if rows.is_empty() {
            return Err(ClaimError::NoClaimsSelected);
        }
        return rows_to_items(rows);
    }

    let mut requested = BTreeSet::new();
    if let Some(token) = req.token_address {
        requested.insert(token);
    }
    for token in &req.token_addresses {
        requested.insert(*token);
    }

    if requested.is_empty() {
        return Err(ClaimError::NoClaimsSelected);
    }

    let tokens = requested.into_iter().collect::<Vec<_>>();
    ensure_no_nft_tokens(world, &tokens).await?;
    let raw = load_active_claim_rows(world, req.address, Some(&tokens)).await?;
    let rows = filter_and_verify_rows(world, claim_distributor, migration_contract, raw).await?;
    if rows.len() != tokens.len() {
        let present = rows
            .iter()
            .map(parse_claim_row_token_address)
            .collect::<Result<BTreeSet<_>, _>>()?;

        let missing = tokens
            .into_iter()
            .find(|token| !present.contains(token))
            .unwrap_or(ETH);
        return Err(ClaimError::TokenNotEligible {
            token_address: missing,
        });
    }

    rows_to_items(rows)
}

/// Bundles the on-chain reads needed to filter a candidate row set into a
/// single Multicall3 `aggregate3` eth_call: the distributor's merkle root, one
/// `isClaimed` per row, and one `isTokenMigrated` per unique token. Drops rows
/// whose leaf has already been finalized or whose token has not yet been
/// migrated, and verifies the on-chain merkle root matches the snapshot when
/// the filtered set is non-empty (preserving the previous error ordering).
async fn filter_and_verify_rows(
    world: &World,
    claim_distributor: Address,
    migration_contract: Address,
    rows: Vec<ActiveClaimRow>,
) -> Result<Vec<ActiveClaimRow>, ClaimError> {
    if rows.is_empty() {
        return Ok(rows);
    }

    let mut indices = Vec::with_capacity(rows.len());
    let mut row_tokens = Vec::with_capacity(rows.len());
    for row in &rows {
        let token = parse_claim_row_token_address(row)?;
        let index = u64::try_from(row.claim_index).map_err(|_| {
            ClaimError::storage(format!(
                "invalid claim_index in claim_entries: {}",
                row.claim_index
            ))
        })?;
        indices.push(index);
        row_tokens.push(token);
    }
    let unique_tokens = row_tokens
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let probes = claim_gateway::batched_prepare_probes(
        world,
        claim_distributor,
        migration_contract,
        &indices,
        &unique_tokens,
    )
    .await
    .map_err(ClaimError::onchain_rpc)?;

    let mut claimable_rows = Vec::with_capacity(rows.len());
    for ((row, index), token) in rows.into_iter().zip(indices.iter()).zip(row_tokens.iter()) {
        let finalized = probes.claim_finalized.get(index).copied().unwrap_or(false);
        if finalized {
            continue;
        }
        let migrated = *token == STETH
            // stETH migration was just deposited, but not migrated.
            || probes.token_migrated.get(token).copied().unwrap_or(false);
        if migrated {
            claimable_rows.push(row);
        }
    }

    if !claimable_rows.is_empty() {
        verify_snapshot_merkle_root(world, &probes).await?;
    }

    Ok(claimable_rows)
}

fn parse_claim_row_token_address(row: &ActiveClaimRow) -> Result<Address, ClaimError> {
    row.token_address.parse::<Address>().map_err(|_| {
        ClaimError::storage(format!(
            "invalid token_address in claim_entries: {}",
            row.token_address
        ))
    })
}

/// Reject any requested token that maps to an NFT in `snapshot_tokens`. The
/// snapshot ID space reserves `id < 1000` for fungible tokens (see
/// `eligibility.rs` and `migrations/lookup.rs`); anything at or above that
/// threshold is an NFT and the claim distributor flow does not handle it.
async fn ensure_no_nft_tokens(world: &World, tokens: &[Address]) -> Result<(), ClaimError> {
    let token_strings = tokens
        .iter()
        .map(|token| format!("{token:#x}"))
        .collect::<Vec<_>>();

    let nft: Option<String> = sqlx::query_scalar(
        r#"
        SELECT requested.address
        FROM unnest($1::text[]) AS requested(address)
        WHERE EXISTS (
            SELECT 1
            FROM snapshot_tokens st
            WHERE st.id >= 1000
              AND LOWER(st.address) = requested.address
        )
        LIMIT 1
        "#,
    )
    .bind(&token_strings)
    .fetch_optional(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    if let Some(address) = nft {
        let token_address = address.parse::<Address>().map_err(|_| {
            ClaimError::storage(format!(
                "invalid token_address in snapshot_tokens: {address}"
            ))
        })?;
        return Err(ClaimError::NftClaimNotSupported { token_address });
    }

    Ok(())
}

async fn load_active_claim_rows(
    world: &World,
    claimant: Address,
    tokens: Option<&[Address]>,
) -> Result<Vec<ActiveClaimRow>, ClaimError> {
    let claimant = format!("{claimant:#x}");
    let rows = if let Some(tokens) = tokens {
        let tokens = tokens
            .iter()
            .map(|token| format!("{token:#x}"))
            .collect::<Vec<_>>();

        sqlx::query_as::<_, ActiveClaimRow>(
            r#"
            SELECT
                ce.claim_index,
                ce.token_address,
                ce.amount,
                cp.proof_hashes
            FROM claim_entries ce
            INNER JOIN claim_proofs cp
                ON cp.claim_index = ce.claim_index
            WHERE ce.claimant_address = $1
              AND ce.token_address = ANY($2)
            ORDER BY ce.token_address ASC
            "#,
        )
        .bind(claimant)
        .bind(tokens)
        .fetch_all(world.db_pool())
        .await
        .map_err(ClaimError::storage)?
    } else {
        sqlx::query_as::<_, ActiveClaimRow>(
            r#"
            SELECT
                ce.claim_index,
                ce.token_address,
                ce.amount,
                cp.proof_hashes
            FROM claim_entries ce
            INNER JOIN claim_proofs cp
                ON cp.claim_index = ce.claim_index
            WHERE ce.claimant_address = $1
            ORDER BY ce.token_address ASC
            "#,
        )
        .bind(claimant)
        .fetch_all(world.db_pool())
        .await
        .map_err(ClaimError::storage)?
    };

    Ok(rows)
}

fn rows_to_items(rows: Vec<ActiveClaimRow>) -> Result<Vec<ClaimBatchItem>, ClaimError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let token_address: Address = row.token_address.parse().map_err(|_| {
            ClaimError::storage(format!(
                "invalid token_address in claim_entries: {}",
                row.token_address
            ))
        })?;
        let amount = U256::from_str_radix(&row.amount, 10).map_err(|_| {
            ClaimError::storage(format!("invalid amount in claim_entries: {}", row.amount))
        })?;
        let merkle_proof = row
            .proof_hashes
            .into_iter()
            .map(|value| {
                value.parse().map_err(|_| {
                    ClaimError::storage(format!("invalid proof hash in claim_proofs: {value}"))
                })
            })
            .collect::<Result<Vec<B256>, _>>()?;

        out.push(ClaimBatchItem {
            index: row.claim_index as u64,
            token_address,
            amount,
            merkle_proof,
        });
    }

    Ok(out)
}

async fn verify_snapshot_merkle_root(
    world: &World,
    probes: &PreparedProbeResult,
) -> Result<(), ClaimError> {
    let stored_root: String = sqlx::query_scalar(
        "SELECT merkle_root FROM claim_snapshot_metadata WHERE singleton = TRUE",
    )
    .fetch_optional(world.db_pool())
    .await
    .map_err(ClaimError::storage)?
    .ok_or_else(|| ClaimError::storage("claim snapshot metadata not initialized"))?;

    let snapshot_root: B256 = stored_root
        .parse()
        .map_err(|_| ClaimError::storage("invalid merkle_root in claim_snapshot_metadata"))?;

    if probes.merkle_root != snapshot_root {
        return Err(ClaimError::MerkleRootMismatch {
            expected: probes.merkle_root,
            got: snapshot_root,
        });
    }

    Ok(())
}

async fn persist_prepared_claim_request(
    world: &World,
    user: Address,
    claims_hash: B256,
    nonce: u64,
    expiry: u64,
    items: &[ClaimBatchItem],
) -> Result<ClaimId, ClaimError> {
    let sequence = sqlx::query_scalar::<_, i64>("SELECT nextval('claim_request_id_seq')")
        .fetch_one(world.db_pool())
        .await
        .map_err(ClaimError::storage)?;
    let claim_id = ClaimId::from_sequence(sequence as u64);

    let payload_json = serialize_payload(&ClaimRequestPayload {
        items: items.to_vec(),
    })?;

    sqlx::query(
        r#"
        INSERT INTO claim_requests (
            claim_id,
            user_address,
            status,
            claims_hash,
            nonce,
            expiry,
            prepared_at,
            payload_json,
            updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, NOW(), $7::jsonb, NOW())
        "#,
    )
    .bind(claim_id.as_str())
    .bind(format!("{user:#x}"))
    .bind("prepared")
    .bind(format!("{claims_hash:#x}"))
    .bind(nonce as i64)
    .bind(expiry as i64)
    .bind(payload_json)
    .execute(world.db_pool())
    .await
    .map_err(ClaimError::storage)?;

    Ok(claim_id)
}
