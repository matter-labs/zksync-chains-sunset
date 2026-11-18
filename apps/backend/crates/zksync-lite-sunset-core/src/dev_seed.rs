use crate::persistence;
use crate::snapshots::{
    self, ClaimMaterializationResult, ClaimsImportRequest, ClaimsImportResult, SnapshotImportCounts,
};
use alloy::primitives::{Address, B256, address};
use eyre::{WrapErr, eyre};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const ACCOUNT_ID: i64 = 1;
const ETH_TOKEN_ID: i64 = 0;
const USDC_TOKEN_ID: i64 = 1;
const DAI_TOKEN_ID: i64 = 2;
const USDT_TOKEN_ID: i64 = 3;
const DEV_SNAPSHOT_LABEL: &str = "snapshot-dev-seeded-address";
const SYNC_PUBKEY_HASH: &str = "sync:0000000000000000000000000000000000000000";
const DEV_SEED_USDC_ADDRESS_ENV: &str = "ZKSYNC_LITE_SUNSET__DEV_SEED__USDC_ADDRESS";
const DEV_SEED_DAI_ADDRESS_ENV: &str = "ZKSYNC_LITE_SUNSET__DEV_SEED__DAI_ADDRESS";
const DEV_SEED_USDT_ADDRESS_ENV: &str = "ZKSYNC_LITE_SUNSET__DEV_SEED__USDT_ADDRESS";

const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
const DAI: Address = address!("0x6B175474E89094C44Da98b954EedeAC495271d0F");
const USDT: Address = address!("0xdAC17F958D2ee523a2206206994597C13D831ec7");
const ETH: Address = address!("0x0000000000000000000000000000000000000000");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureTokenAddresses {
    usdc: Address,
    dai: Address,
    usdt: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevFixtureMode {
    Default,
    MigrateOnly,
    AllUnmigrated,
}

impl DevFixtureMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::MigrateOnly => "migrate-only",
            Self::AllUnmigrated => "all-unmigrated",
        }
    }
}

impl FromStr for DevFixtureMode {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "default" => Ok(Self::Default),
            "migrate-only" => Ok(Self::MigrateOnly),
            "all-unmigrated" => Ok(Self::AllUnmigrated),
            _ => Err(eyre::eyre!(
                "unsupported fixture mode `{value}`; expected one of: default, migrate-only, all-unmigrated"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateDevSnapshotRequest {
    pub address: Address,
    pub output_dir: PathBuf,
    pub mode: DevFixtureMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedDevSnapshot {
    pub address: Address,
    pub mode: DevFixtureMode,
    pub output_dir: PathBuf,
    pub token_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevSeedRequest {
    pub database_url: Url,
    pub address: Address,
    pub mode: DevFixtureMode,
    pub snapshot_label: String,
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevSeedResult {
    pub address: Address,
    pub mode: DevFixtureMode,
    pub output_dir: PathBuf,
    pub imported: SnapshotImportCounts,
    pub materialized: ClaimMaterializationResult,
    pub claims: Vec<SeededClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeededClaim {
    pub claim_index: u64,
    pub token_address: Address,
    pub amount: String,
    pub leaf_hash: B256,
    pub proof_hashes: Vec<B256>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureToken {
    token_id: i64,
    token_address: Address,
    symbol: &'static str,
    decimals: u8,
    amount_dec: &'static str,
}

pub async fn generate_dev_snapshot(
    req: &GenerateDevSnapshotRequest,
) -> eyre::Result<GeneratedDevSnapshot> {
    fs::create_dir_all(&req.output_dir).wrap_err_with(|| {
        format!(
            "failed to create dev snapshot output dir {}",
            req.output_dir.display()
        )
    })?;

    let fixture = fixture_tokens(req.mode, fixture_token_addresses_from_env()?);
    write_accounts_csv(&req.output_dir, req.address)?;
    write_tokens_csv(&req.output_dir, &fixture)?;
    write_balances_csv(&req.output_dir, &fixture)?;

    Ok(GeneratedDevSnapshot {
        address: req.address,
        mode: req.mode,
        output_dir: req.output_dir.clone(),
        token_count: fixture.len(),
    })
}

pub async fn seed_dev_snapshot(req: &DevSeedRequest) -> eyre::Result<DevSeedResult> {
    let output_dir = req.output_dir.clone().unwrap_or_else(default_output_dir);
    generate_dev_snapshot(&GenerateDevSnapshotRequest {
        address: req.address,
        output_dir: output_dir.clone(),
        mode: req.mode,
    })
    .await?;

    let pool = persistence::connect_pool(&req.database_url, 5).await?;
    persistence::run_migrations(&pool).await?;
    reset_dev_seed_runtime_tables(&pool).await?;

    let claims_import = ClaimsImportRequest {
        database_url: req.database_url.clone(),
        data_dir: output_dir.clone(),
        chunk_size: std::num::NonZeroUsize::new(5000).expect("5000 is non-zero"),
        truncate: true,
        snapshot_label: req.snapshot_label.clone(),
        reset_runtime_state: false,
    };
    let ClaimsImportResult {
        imported,
        materialized,
    } = snapshots::import_and_materialize_claims(&claims_import).await?;

    let claims = load_seeded_claims(&pool, req.address).await?;

    Ok(DevSeedResult {
        address: req.address,
        mode: req.mode,
        output_dir,
        imported,
        materialized,
        claims,
    })
}

pub fn default_snapshot_label() -> String {
    DEV_SNAPSHOT_LABEL.to_string()
}

fn default_output_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "zksync-lite-sunset-dev-seed-{}-{nanos}",
        std::process::id()
    ))
}

fn fixture_token_addresses_from_env() -> eyre::Result<FixtureTokenAddresses> {
    Ok(FixtureTokenAddresses {
        usdc: env_address_or_default(DEV_SEED_USDC_ADDRESS_ENV, USDC)?,
        dai: env_address_or_default(DEV_SEED_DAI_ADDRESS_ENV, DAI)?,
        usdt: env_address_or_default(DEV_SEED_USDT_ADDRESS_ENV, USDT)?,
    })
}

#[cfg(test)]
fn default_fixture_token_addresses() -> FixtureTokenAddresses {
    FixtureTokenAddresses {
        usdc: USDC,
        dai: DAI,
        usdt: USDT,
    }
}

fn env_address_or_default(key: &str, default: Address) -> eyre::Result<Address> {
    match env::var(key) {
        Ok(raw) if !raw.trim().is_empty() => raw
            .parse()
            .map_err(|_| eyre!("invalid address in {key}: {raw}")),
        Ok(_) | Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(eyre!("failed to read {key}: {err}")),
    }
}

fn fixture_tokens(mode: DevFixtureMode, addresses: FixtureTokenAddresses) -> Vec<FixtureToken> {
    match mode {
        DevFixtureMode::MigrateOnly => vec![FixtureToken {
            token_id: DAI_TOKEN_ID,
            token_address: addresses.dai,
            symbol: "DAI",
            decimals: 18,
            amount_dec: "4000000000000000000",
        }],
        DevFixtureMode::Default => vec![
            FixtureToken {
                token_id: USDC_TOKEN_ID,
                token_address: addresses.usdc,
                symbol: "USDC",
                decimals: 6,
                amount_dec: "1000000",
            },
            FixtureToken {
                token_id: DAI_TOKEN_ID,
                token_address: addresses.dai,
                symbol: "DAI",
                decimals: 18,
                amount_dec: "4000000000000000000",
            },
            FixtureToken {
                token_id: USDT_TOKEN_ID,
                token_address: addresses.usdt,
                symbol: "USDT",
                decimals: 6,
                amount_dec: "1000000",
            },
            FixtureToken {
                token_id: ETH_TOKEN_ID,
                token_address: ETH,
                symbol: "ETH",
                decimals: 18,
                amount_dec: "100000000000000000",
            },
        ],
        DevFixtureMode::AllUnmigrated => vec![
            FixtureToken {
                token_id: USDC_TOKEN_ID,
                token_address: addresses.usdc,
                symbol: "USDC",
                decimals: 6,
                amount_dec: "1000000",
            },
            FixtureToken {
                token_id: DAI_TOKEN_ID,
                token_address: addresses.dai,
                symbol: "DAI",
                decimals: 18,
                amount_dec: "4000000000000000000",
            },
            FixtureToken {
                token_id: USDT_TOKEN_ID,
                token_address: addresses.usdt,
                symbol: "USDT",
                decimals: 6,
                amount_dec: "1000000",
            },
            FixtureToken {
                token_id: ETH_TOKEN_ID,
                token_address: ETH,
                symbol: "ETH",
                decimals: 18,
                amount_dec: "100000000000000000",
            },
        ],
    }
}

fn write_accounts_csv(output_dir: &Path, address: Address) -> eyre::Result<()> {
    let path = output_dir.join("accounts.csv");
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to create {}", path.display()))?;
    writer
        .write_record(["id", "nonce", "address", "pubkey_hash"])
        .wrap_err("failed to write accounts.csv header")?;
    writer
        .write_record([
            ACCOUNT_ID.to_string(),
            "0".to_string(),
            format!("{address:#x}"),
            SYNC_PUBKEY_HASH.to_string(),
        ])
        .wrap_err("failed to write accounts.csv row")?;
    writer
        .flush()
        .wrap_err("failed to flush accounts.csv writer")?;
    Ok(())
}

fn write_tokens_csv(output_dir: &Path, tokens: &[FixtureToken]) -> eyre::Result<()> {
    let path = output_dir.join("tokens.csv");
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to create {}", path.display()))?;
    writer
        .write_record(["id", "address", "symbol", "decimals"])
        .wrap_err("failed to write tokens.csv header")?;
    for token in tokens {
        writer
            .write_record([
                token.token_id.to_string(),
                format!("{:#x}", token.token_address),
                token.symbol.to_string(),
                token.decimals.to_string(),
            ])
            .wrap_err("failed to write tokens.csv row")?;
    }
    writer
        .flush()
        .wrap_err("failed to flush tokens.csv writer")?;
    Ok(())
}

fn write_balances_csv(output_dir: &Path, tokens: &[FixtureToken]) -> eyre::Result<()> {
    let path = output_dir.join("balances.csv");
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to create {}", path.display()))?;
    writer
        .write_record(["account_id", "coin_id", "balance"])
        .wrap_err("failed to write balances.csv header")?;
    for token in tokens {
        writer
            .write_record([
                ACCOUNT_ID.to_string(),
                token.token_id.to_string(),
                token.amount_dec.to_string(),
            ])
            .wrap_err("failed to write balances.csv row")?;
    }
    writer
        .flush()
        .wrap_err("failed to flush balances.csv writer")?;
    Ok(())
}

async fn reset_dev_seed_runtime_tables(pool: &PgPool) -> eyre::Result<()> {
    sqlx::query(
        r#"
        TRUNCATE TABLE
            migration_requests,
            claim_proofs,
            claim_entries,
            claim_snapshot_metadata
        RESTART IDENTITY CASCADE
        "#,
    )
    .execute(pool)
    .await
    .wrap_err("failed to reset runtime tables before dev seeding")?;

    Ok(())
}

async fn load_seeded_claims(pool: &PgPool, user: Address) -> eyre::Result<Vec<SeededClaim>> {
    let rows = sqlx::query(
        r#"
        SELECT
            ce.claim_index,
            ce.token_address,
            ce.amount,
            ce.leaf_hash,
            cp.proof_hashes
        FROM claim_entries ce
        INNER JOIN claim_proofs cp
            ON cp.claim_index = ce.claim_index
        WHERE ce.claimant_address = $1
        ORDER BY ce.claim_index ASC
        "#,
    )
    .bind(format!("{user:#x}"))
    .fetch_all(pool)
    .await
    .wrap_err("failed to load seeded claim proofs for dev snapshot")?;

    let mut claims = Vec::with_capacity(rows.len());
    for row in rows {
        let proof_hashes = row
            .try_get::<Vec<String>, _>("proof_hashes")
            .wrap_err("missing proof_hashes in seeded claim query")?
            .into_iter()
            .map(|hash| {
                hash.parse()
                    .wrap_err_with(|| format!("invalid proof hash in claim_proofs: {hash}"))
            })
            .collect::<eyre::Result<Vec<B256>>>()?;

        claims.push(SeededClaim {
            claim_index: row
                .try_get::<i64, _>("claim_index")
                .wrap_err("missing claim_index in seeded claim query")?
                as u64,
            token_address: row
                .try_get::<String, _>("token_address")
                .wrap_err("missing token_address in seeded claim query")?
                .parse()
                .wrap_err("invalid token_address in seeded claim query")?,
            amount: row
                .try_get::<String, _>("amount")
                .wrap_err("missing amount in seeded claim query")?,
            leaf_hash: row
                .try_get::<String, _>("leaf_hash")
                .wrap_err("missing leaf_hash in seeded claim query")?
                .parse()
                .wrap_err("invalid leaf_hash in seeded claim query")?,
            proof_hashes,
        });
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_modes_match_expected_token_counts() {
        let addresses = default_fixture_token_addresses();
        assert_eq!(fixture_tokens(DevFixtureMode::Default, addresses).len(), 4);
        assert_eq!(
            fixture_tokens(DevFixtureMode::AllUnmigrated, addresses).len(),
            4
        );
        assert_eq!(
            fixture_tokens(DevFixtureMode::MigrateOnly, addresses).len(),
            1
        );
    }
}
