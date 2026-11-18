use crate::world::World;
use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct MigrationId(String);

impl MigrationId {
    pub const PREFIX: &str = "mig_";
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
pub enum MigrationIdParseError {
    #[error(
        "invalid migration id format, expected `{}`########",
        MigrationId::PREFIX
    )]
    InvalidFormat,
}

impl FromStr for MigrationId {
    type Err = MigrationIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !value.starts_with(Self::PREFIX) {
            return Err(MigrationIdParseError::InvalidFormat);
        }
        let suffix = &value[Self::PREFIX.len()..];
        if suffix.len() != Self::WIDTH || !suffix.chars().all(|c| c.is_ascii_digit()) {
            return Err(MigrationIdParseError::InvalidFormat);
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for MigrationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for MigrationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("invalid address in field `{field}`: {input}")]
    InvalidAddress { field: &'static str, input: String },
    #[error("invalid tx hash: {input}")]
    InvalidTxHash { input: String },
    #[error("missing token selection for migrate")]
    MissingTokenSelection,
    #[error("token already migrated: {token_address}")]
    AlreadyMigrated { token_address: Address },
    #[error("onchain config error")]
    OnchainConfig { detail: String },
    #[error("onchain rpc error")]
    OnchainRpc { detail: String },
    #[error("migration request not found: {migration_id}")]
    MigrationNotFound { migration_id: MigrationId },
    #[error("submitted transaction not found onchain: {tx_hash:#x}")]
    SubmittedTransactionNotFound { tx_hash: B256 },
    #[error("submitted transaction mismatch: {detail}")]
    SubmittedTransactionMismatch { detail: String },
    #[error("storage error")]
    Storage { detail: String },
}

impl MigrationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAddress { .. } => "INVALID_ADDRESS",
            Self::InvalidTxHash { .. } => "INVALID_TX_HASH",
            Self::MissingTokenSelection => "MISSING_TOKEN_SELECTION",
            Self::AlreadyMigrated { .. } => "ALREADY_MIGRATED",
            Self::OnchainConfig { .. } => "ONCHAIN_CONFIG_ERROR",
            Self::OnchainRpc { .. } => "ONCHAIN_RPC_ERROR",
            Self::MigrationNotFound { .. } => "MIGRATION_NOT_FOUND",
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStatus {
    Prepared,
    Submitted,
    Mining,
    MinedSuccess,
    MinedReverted,
}

impl MigrationStatus {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Submitted => "submitted",
            Self::Mining => "mining",
            Self::MinedSuccess => "mined_success",
            Self::MinedReverted => "mined_reverted",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, MigrationError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "submitted" => Ok(Self::Submitted),
            "mining" => Ok(Self::Mining),
            "mined_success" => Ok(Self::MinedSuccess),
            "mined_reverted" => Ok(Self::MinedReverted),
            other => Err(MigrationError::storage(format!(
                "unknown migration status `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReceipt {
    pub block_number: u64,
    pub block_hash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationItem {
    pub token_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationRequestPayload {
    pub item: MigrationItem,
}

#[derive(Debug, Clone)]
pub(crate) struct MigrationRequestRecord {
    pub migration_id: MigrationId,
    pub status: MigrationStatus,
    pub tx_hash: Option<B256>,
    pub block_number: Option<u64>,
    pub block_hash: Option<B256>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub payload: MigrationRequestPayload,
}

pub(crate) fn migration_contract(world: &World) -> Result<Address, MigrationError> {
    world
        .config()
        .execution
        .migration_contract_address
        .ok_or_else(|| {
            MigrationError::onchain_config("missing execution.migration_contract_address")
        })
}

pub(crate) fn parse_payload_json(
    payload_json: &str,
) -> Result<MigrationRequestPayload, MigrationError> {
    serde_json::from_str(payload_json)
        .map_err(|err| MigrationError::storage(format!("invalid migration payload_json: {err}")))
}

pub(crate) fn serialize_payload(
    payload: &MigrationRequestPayload,
) -> Result<String, MigrationError> {
    serde_json::to_string(payload).map_err(|err| {
        MigrationError::storage(format!("failed to serialize migration payload_json: {err}"))
    })
}

pub(crate) fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
