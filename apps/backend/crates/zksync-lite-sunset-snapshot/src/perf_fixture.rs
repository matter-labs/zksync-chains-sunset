use crate::model::SourceClaimRow;
use alloy::primitives::{Address, U256};
use eyre::{WrapErr, ensure};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticSnapshotSpec {
    pub account_count: usize,
    pub token_count: usize,
    pub balances_per_account: usize,
    pub duplicate_span: usize,
}

impl SyntheticSnapshotSpec {
    pub fn benchmark_small() -> Self {
        Self {
            account_count: 4_000,
            token_count: 32,
            balances_per_account: 8,
            duplicate_span: 2,
        }
    }

    pub fn benchmark_medium() -> Self {
        Self {
            account_count: 20_000,
            token_count: 64,
            balances_per_account: 8,
            duplicate_span: 2,
        }
    }

    pub fn performance_smoke() -> Self {
        Self {
            account_count: 50_000,
            token_count: 128,
            balances_per_account: 8,
            duplicate_span: 2,
        }
    }

    pub fn normalized_source_rows(self) -> usize {
        self.account_count * self.balances_per_account
    }
}

impl Default for SyntheticSnapshotSpec {
    fn default() -> Self {
        Self::benchmark_small()
    }
}

pub fn generate_source_rows(spec: SyntheticSnapshotSpec) -> eyre::Result<Vec<SourceClaimRow>> {
    validate_spec(spec)?;

    let mut rows = Vec::with_capacity(spec.normalized_source_rows());
    for account_index in 0..spec.account_count {
        let claimant_address = synthetic_address(0x11, account_index + 1);

        for balance_index in 0..spec.balances_per_account {
            let token_bucket = balance_index / spec.duplicate_span.max(1);
            let token_index = (account_index + token_bucket) % spec.token_count;
            let token_address = synthetic_token_address(token_index);
            let amount = synthetic_amount(account_index, balance_index);

            rows.push(SourceClaimRow {
                claimant_address,
                token_address,
                amount,
            });
        }
    }

    Ok(rows)
}

pub fn write_snapshot_csv_fixture(
    data_dir: &Path,
    spec: SyntheticSnapshotSpec,
) -> eyre::Result<()> {
    validate_spec(spec)?;
    std::fs::create_dir_all(data_dir)
        .wrap_err_with(|| format!("failed to create synthetic data dir {}", data_dir.display()))?;

    let mut accounts_writer = csv::Writer::from_path(data_dir.join("accounts.csv"))
        .wrap_err("failed to create synthetic accounts.csv")?;
    accounts_writer
        .write_record(["id", "address"])
        .wrap_err("failed to write synthetic accounts.csv header")?;

    for account_index in 0..spec.account_count {
        let account_id = (account_index + 1).to_string();
        let account_address = format!("{:#x}", synthetic_address(0x11, account_index + 1));

        accounts_writer
            .write_record([account_id.as_str(), account_address.as_str()])
            .wrap_err("failed to write synthetic account row")?;
    }
    accounts_writer
        .flush()
        .wrap_err("failed to flush synthetic accounts.csv")?;

    let mut tokens_writer = csv::Writer::from_path(data_dir.join("tokens.csv"))
        .wrap_err("failed to create synthetic tokens.csv")?;
    tokens_writer
        .write_record(["id", "address", "symbol", "decimals"])
        .wrap_err("failed to write synthetic tokens.csv header")?;

    for token_index in 0..spec.token_count {
        let token_id = token_index.to_string();
        let token_address = format!("{:#x}", synthetic_token_address(token_index));
        let symbol = format!("T{:04}", token_index);
        let decimals = if token_index % 2 == 0 { "18" } else { "6" };

        tokens_writer
            .write_record([
                token_id.as_str(),
                token_address.as_str(),
                symbol.as_str(),
                decimals,
            ])
            .wrap_err("failed to write synthetic token row")?;
    }
    tokens_writer
        .flush()
        .wrap_err("failed to flush synthetic tokens.csv")?;

    let mut balances_writer = csv::Writer::from_path(data_dir.join("balances.csv"))
        .wrap_err("failed to create synthetic balances.csv")?;
    balances_writer
        .write_record(["account_id", "coin_id", "balance"])
        .wrap_err("failed to write synthetic balances.csv header")?;

    for account_index in 0..spec.account_count {
        let account_id = (account_index + 1).to_string();
        for balance_index in 0..spec.balances_per_account {
            let token_bucket = balance_index / spec.duplicate_span.max(1);
            let token_index = (account_index + token_bucket) % spec.token_count;
            let coin_id = token_index.to_string();
            let amount = synthetic_amount(account_index, balance_index).to_string();

            balances_writer
                .write_record([account_id.as_str(), coin_id.as_str(), amount.as_str()])
                .wrap_err("failed to write synthetic balance row")?;
        }
    }
    balances_writer
        .flush()
        .wrap_err("failed to flush synthetic balances.csv")?;

    Ok(())
}

fn validate_spec(spec: SyntheticSnapshotSpec) -> eyre::Result<()> {
    ensure!(
        spec.account_count > 0,
        "synthetic snapshot requires at least one account"
    );
    ensure!(
        spec.token_count > 0,
        "synthetic snapshot requires at least one token"
    );
    ensure!(
        spec.balances_per_account > 0,
        "synthetic snapshot requires at least one balance per account"
    );
    ensure!(
        spec.duplicate_span > 0,
        "synthetic duplicate_span must be positive"
    );
    Ok(())
}

fn synthetic_token_address(token_index: usize) -> Address {
    synthetic_address(0x22, token_index)
}

fn synthetic_address(namespace: u8, value: usize) -> Address {
    let mut bytes = [0_u8; 20];
    bytes[0] = namespace;
    bytes[12..].copy_from_slice(&(value as u64).to_be_bytes());
    Address::from_slice(&bytes)
}

fn synthetic_amount(account_index: usize, balance_index: usize) -> U256 {
    U256::from(((account_index + 1) * (balance_index + 2)) as u64)
}
