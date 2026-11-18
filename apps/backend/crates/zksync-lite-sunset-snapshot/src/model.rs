use alloy::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceClaimRow {
    pub claimant_address: Address,
    pub token_address: Address,
    pub amount: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotTokenRow {
    pub id: i64,
    pub address: Address,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedClaimEntry {
    pub claim_index: u64,
    pub claimant_address: Address,
    pub token_address: Address,
    pub amount: U256,
    pub leaf_hash: B256,
    pub proof_hashes: Vec<B256>,
}
