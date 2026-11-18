use crate::signed_batches::{ClaimHashItemInput, compute_claims_hash as compute_claims_hash_inner};
use crate::world::World;
use alloy::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ClaimId(String);

impl ClaimId {
    pub const PREFIX: &str = "clm_";
    const WIDTH: usize = 8;

    pub(crate) fn from_sequence(seq: u64) -> Self {
        Self(format!(
            "{}{seq:0width$}",
            Self::PREFIX,
            width = Self::WIDTH
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaimIdParseError {
    #[error("invalid claim id format, expected `{}`########", ClaimId::PREFIX)]
    InvalidFormat,
}

impl FromStr for ClaimId {
    type Err = ClaimIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !value.starts_with(Self::PREFIX) {
            return Err(ClaimIdParseError::InvalidFormat);
        }
        let suffix = &value[Self::PREFIX.len()..];
        if suffix.len() != Self::WIDTH || !suffix.chars().all(|c| c.is_ascii_digit()) {
            return Err(ClaimIdParseError::InvalidFormat);
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for ClaimId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl AsRef<str> for ClaimId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ClaimId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum ClaimError {
    #[error("invalid address in field `{field}`: {input}")]
    InvalidAddress { field: &'static str, input: String },
    #[error("no tokens selected for claim")]
    NoClaimsSelected,
    #[error("too many claims in batch: max {max}, got {got}")]
    TooManyClaims { max: usize, got: usize },
    #[error("token is not eligible for claim: {token_address}")]
    TokenNotEligible { token_address: Address },
    #[error("NFT claims are not supported: {token_address}")]
    NftClaimNotSupported { token_address: Address },
    #[error("claim item list is not canonical (must be sorted and deduplicated)")]
    InvalidClaimOrdering,
    #[error("claimsHash does not match canonical claim item list")]
    InvalidClaimsHash,
    #[error("signature has expired")]
    ExpiredSignature,
    #[error("invalid action signature")]
    InvalidSignature,
    #[error(
        "claim snapshot merkle root mismatch: expected contract root {expected:#x}, active snapshot root {got:#x}"
    )]
    MerkleRootMismatch { expected: B256, got: B256 },
    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("onchain config error")]
    OnchainConfig { detail: String },
    #[error("onchain rpc error")]
    OnchainRpc { detail: String },
    #[error("claim not found: {claim_id}")]
    ClaimNotFound { claim_id: ClaimId },
    #[error("invalid tx hash: {input}")]
    InvalidTxHash { input: String },
    #[error("submitted transaction not found onchain: {tx_hash:#x}")]
    SubmittedTransactionNotFound { tx_hash: B256 },
    #[error("submitted transaction mismatch: {detail}")]
    SubmittedTransactionMismatch { detail: String },
    #[error("storage error")]
    Storage { detail: String },
}

impl ClaimError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAddress { .. } => "INVALID_ADDRESS",
            Self::NoClaimsSelected => "NO_CLAIMS_SELECTED",
            Self::TooManyClaims { .. } => "TOO_MANY_CLAIMS",
            Self::TokenNotEligible { .. } => "TOKEN_NOT_ELIGIBLE",
            Self::NftClaimNotSupported { .. } => "NFT_CLAIM_NOT_SUPPORTED",
            Self::InvalidClaimOrdering => "INVALID_CLAIM_ORDERING",
            Self::InvalidClaimsHash => "INVALID_CLAIMS_HASH",
            Self::ExpiredSignature => "EXPIRED_SIGNATURE",
            Self::InvalidSignature => "INVALID_SIGNATURE",
            Self::MerkleRootMismatch { .. } => "MERKLE_ROOT_MISMATCH",
            Self::InvalidNonce { .. } => "INVALID_NONCE",
            Self::OnchainConfig { .. } => "ONCHAIN_CONFIG_ERROR",
            Self::OnchainRpc { .. } => "ONCHAIN_RPC_ERROR",
            Self::ClaimNotFound { .. } => "CLAIM_NOT_FOUND",
            Self::InvalidTxHash { .. } => "INVALID_TX_HASH",
            Self::SubmittedTransactionNotFound { .. } => "SUBMITTED_TX_NOT_FOUND",
            Self::SubmittedTransactionMismatch { .. } => "SUBMITTED_TX_MISMATCH",
            Self::Storage { .. } => "INTERNAL_STORAGE",
        }
    }

    pub(crate) fn onchain_config(detail: impl Into<String>) -> Self {
        Self::OnchainConfig {
            detail: detail.into(),
        }
    }

    pub(crate) fn onchain_rpc(detail: impl fmt::Display) -> Self {
        Self::OnchainRpc {
            detail: detail.to_string(),
        }
    }

    pub(crate) fn storage(detail: impl fmt::Display) -> Self {
        Self::Storage {
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimFailureCode {
    TxReverted,
    ReceiptTimeout,
    OnchainStateMismatch,
    BroadcastFailed,
}

impl ClaimFailureCode {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::TxReverted => "TX_REVERTED",
            Self::ReceiptTimeout => "RECEIPT_TIMEOUT",
            Self::OnchainStateMismatch => "ONCHAIN_STATE_MISMATCH",
            Self::BroadcastFailed => "BROADCAST_FAILED",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, ClaimError> {
        match value {
            "TX_REVERTED" => Ok(Self::TxReverted),
            "RECEIPT_TIMEOUT" => Ok(Self::ReceiptTimeout),
            "ONCHAIN_STATE_MISMATCH" => Ok(Self::OnchainStateMismatch),
            "BROADCAST_FAILED" => Ok(Self::BroadcastFailed),
            other => Err(ClaimError::storage(format!(
                "unknown claim error_code `{other}`"
            ))),
        }
    }

    pub(crate) fn public_message(self) -> &'static str {
        match self {
            Self::TxReverted => "Claim transaction reverted",
            Self::ReceiptTimeout => "Timed out while waiting for receipt",
            Self::OnchainStateMismatch => "Claim finalization could not be verified",
            Self::BroadcastFailed => "Claim transaction could not be broadcast",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Prepared,
    Pending,
    Submitted,
    Mining,
    MinedSuccess,
    MinedReverted,
}

impl ClaimStatus {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Pending => "pending",
            Self::Submitted => "submitted",
            Self::Mining => "mining",
            Self::MinedSuccess => "mined_success",
            Self::MinedReverted => "mined_reverted",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, ClaimError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "pending" => Ok(Self::Pending),
            "submitted" => Ok(Self::Submitted),
            "mining" => Ok(Self::Mining),
            "mined_success" => Ok(Self::MinedSuccess),
            "mined_reverted" => Ok(Self::MinedReverted),
            other => Err(ClaimError::storage(format!(
                "unknown claim status `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimBatchItem {
    pub index: u64,
    pub token_address: Address,
    #[serde(with = "crate::eligibility::serde_u256_dec_str")]
    pub amount: U256,
    pub merkle_proof: Vec<B256>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimReceipt {
    pub block_number: u64,
    pub block_hash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimRequestPayload {
    pub items: Vec<ClaimBatchItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimRequestRecord {
    pub claim_id: ClaimId,
    pub status: ClaimStatus,
    pub tx_hash: Option<B256>,
    pub block_number: Option<u64>,
    pub block_hash: Option<B256>,
    pub failure: Option<ClaimFailureCode>,
    pub failure_detail: Option<String>,
}

pub(crate) fn claim_contract(world: &World) -> Result<Address, ClaimError> {
    world
        .config()
        .execution
        .claim_contract_address
        .ok_or_else(|| ClaimError::onchain_config("missing execution.claim_contract_address"))
}

pub(crate) fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn canonical_claim_items(
    items: Vec<ClaimBatchItem>,
) -> Result<Vec<ClaimBatchItem>, ClaimError> {
    if items.is_empty() {
        return Err(ClaimError::NoClaimsSelected);
    }

    let mut last = None;
    for item in &items {
        if let Some(previous) = last
            && item.token_address <= previous
        {
            return Err(ClaimError::InvalidClaimOrdering);
        }
        last = Some(item.token_address);
    }

    Ok(items)
}

pub(crate) fn compute_claims_hash(items: &[ClaimBatchItem]) -> B256 {
    let hash_items = items
        .iter()
        .map(|item| ClaimHashItemInput {
            index: item.index,
            token: item.token_address,
            amount: item.amount,
        })
        .collect::<Vec<_>>();
    compute_claims_hash_inner(&hash_items)
}

pub(crate) fn parse_payload_json(payload_json: &str) -> Result<ClaimRequestPayload, ClaimError> {
    serde_json::from_str(payload_json)
        .map_err(|err| ClaimError::storage(format!("invalid claim payload_json: {err}")))
}

pub(crate) fn serialize_payload(payload: &ClaimRequestPayload) -> Result<String, ClaimError> {
    serde_json::to_string(payload).map_err(|err| {
        ClaimError::storage(format!("failed to serialize claim payload_json: {err}"))
    })
}
