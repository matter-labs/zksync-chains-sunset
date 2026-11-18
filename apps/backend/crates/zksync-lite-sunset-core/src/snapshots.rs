use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use url::Url;

use alloy::primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SnapshotImportCounts {
    pub accounts: usize,
    pub tokens: usize,
    pub balances: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotImportRequest {
    pub database_url: Url,
    pub data_dir: PathBuf,
    pub chunk_size: NonZeroUsize,
    pub truncate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimMaterializationRequest {
    pub database_url: Url,
    pub snapshot_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimMaterializationResult {
    pub label: String,
    pub merkle_root: B256,
    pub source_checksum: B256,
    pub entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimsImportRequest {
    pub database_url: Url,
    pub data_dir: PathBuf,
    pub chunk_size: NonZeroUsize,
    pub truncate: bool,
    pub snapshot_label: String,
    pub reset_runtime_state: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimsImportResult {
    pub imported: SnapshotImportCounts,
    pub materialized: ClaimMaterializationResult,
}

pub async fn initialize_snapshot_storage(database_url: &Url) -> eyre::Result<()> {
    crate::persistence::init_database(database_url).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimEntriesTokenTotal {
    pub token_address: String,
    pub total: String,
}

pub async fn sum_claim_entries_for_token(
    database_url: &Url,
    token_address: &str,
) -> eyre::Result<ClaimEntriesTokenTotal> {
    let total =
        crate::persistence::sum_claim_entries_amount_for_token(database_url, token_address).await?;
    Ok(ClaimEntriesTokenTotal {
        token_address: token_address.to_string(),
        total,
    })
}

pub async fn import_snapshot_state(
    req: &SnapshotImportRequest,
) -> eyre::Result<SnapshotImportCounts> {
    let counts = crate::persistence::import_snapshots(
        &req.database_url,
        &req.data_dir,
        req.chunk_size,
        req.truncate,
    )
    .await?;

    Ok(SnapshotImportCounts {
        accounts: counts.accounts,
        tokens: counts.tokens,
        balances: counts.balances,
    })
}

pub async fn materialize_claims(
    req: &ClaimMaterializationRequest,
) -> eyre::Result<ClaimMaterializationResult> {
    let pool = crate::persistence::connect_pool(&req.database_url, 5).await?;
    crate::persistence::run_migrations(&pool).await?;

    let result = crate::persistence::materialize_claim_snapshot_with_pool(
        &pool,
        &crate::persistence::ClaimMaterializationRequest {
            label: req.snapshot_label.clone(),
        },
    )
    .await?;

    Ok(ClaimMaterializationResult {
        label: result.label,
        merkle_root: result.merkle_root,
        source_checksum: result.source_checksum,
        entry_count: result.entry_count,
    })
}

pub async fn import_and_materialize_claims(
    req: &ClaimsImportRequest,
) -> eyre::Result<ClaimsImportResult> {
    let pool = crate::persistence::connect_pool(&req.database_url, 5).await?;
    crate::persistence::run_migrations(&pool).await?;

    if req.reset_runtime_state {
        crate::persistence::reset_runtime_state(&pool).await?;
    }

    let imported = crate::persistence::import_snapshots_with_pool(
        &pool,
        &req.data_dir,
        req.chunk_size,
        req.truncate,
    )
    .await?;

    let materialized = crate::persistence::materialize_claim_snapshot_with_pool(
        &pool,
        &crate::persistence::ClaimMaterializationRequest {
            label: req.snapshot_label.clone(),
        },
    )
    .await?;

    Ok(ClaimsImportResult {
        imported: SnapshotImportCounts {
            accounts: imported.accounts,
            tokens: imported.tokens,
            balances: imported.balances,
        },
        materialized: ClaimMaterializationResult {
            label: materialized.label,
            merkle_root: materialized.merkle_root,
            source_checksum: materialized.source_checksum,
            entry_count: materialized.entry_count,
        },
    })
}
