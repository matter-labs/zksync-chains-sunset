use crate::artifacts;
use crate::manifest::{
    SnapshotArtifactFiles, SnapshotCompilerInfo, SnapshotManifest, SnapshotRoots,
    SnapshotRowCounts, SnapshotSourceFile,
};
use crate::model::{MaterializedClaimEntry, SnapshotTokenRow, SourceClaimRow};
use alloy::primitives::{Address, B256, U256};
use eyre::{WrapErr, ensure, eyre};
use merkle_tree_generator::{ClaimTree, Leaf};
use rayon::prelude::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileSnapshotRequest {
    pub data_dir: PathBuf,
    pub out_dir: PathBuf,
    pub snapshot_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileSnapshotResult {
    pub manifest: SnapshotManifest,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct InputRowCounts {
    accounts: usize,
    tokens: usize,
    balances: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedSnapshotInput {
    aggregation: SnapshotAggregation,
    tokens: Vec<SnapshotTokenRow>,
    counts: InputRowCounts,
    source_files: Vec<SnapshotSourceFile>,
}

type ClaimAggregateMap = BTreeMap<(Address, Address), U256>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SnapshotAggregation {
    normalized_source_rows: usize,
    checksum_rows: BTreeMap<ChecksumRowKey, usize>,
    claim_aggregates: ClaimAggregateMap,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ChecksumRowKey {
    claimant_address: String,
    token_address: String,
    amount: String,
}

impl SnapshotAggregation {
    fn push_row(&mut self, row: &SourceClaimRow) -> eyre::Result<()> {
        self.normalized_source_rows += 1;

        let checksum_row = ChecksumRowKey {
            claimant_address: format!("{:#x}", row.claimant_address),
            token_address: format!("{:#x}", row.token_address),
            amount: row.amount.to_string(),
        };
        *self.checksum_rows.entry(checksum_row).or_default() += 1;

        let claim_key = (row.claimant_address, row.token_address);
        let claim_amount = self.claim_aggregates.entry(claim_key).or_default();
        checked_add_amount(claim_amount, row.amount, "claim aggregate accumulation")?;

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct AccountCsvRow {
    id: i64,
    address: String,
}

#[derive(Debug, Deserialize)]
struct TokenCsvRow {
    id: i64,
    address: String,
    symbol: String,
    decimals: u8,
}

#[derive(Debug, Deserialize)]
struct BalanceCsvRow {
    account_id: i64,
    coin_id: i64,
    balance: String,
}

#[derive(Debug, serde::Serialize)]
struct ClaimEntryArtifactRow {
    claim_index: u64,
    claimant_address: String,
    token_address: String,
    amount: String,
    leaf_hash: String,
}

#[derive(Debug, serde::Serialize)]
struct ClaimProofArtifactRow {
    claim_index: u64,
    proof_hashes: String,
}

#[derive(Debug, serde::Serialize)]
struct TokenArtifactRow {
    id: i64,
    address: String,
    symbol: String,
    decimals: u8,
}

pub fn compile_snapshot(req: &CompileSnapshotRequest) -> eyre::Result<CompileSnapshotResult> {
    let compile_started_at = Instant::now();
    info!(
        component = "snapshot-compile",
        phase = "start",
        data_dir = %req.data_dir.display(),
        out_dir = %req.out_dir.display(),
        snapshot_label = %req.snapshot_label,
        "starting compile"
    );
    let load_started_at = Instant::now();
    let input = load_normalized_snapshot_input(&req.data_dir)?;
    info!(
        component = "snapshot-compile",
        phase = "load-input",
        elapsed_ms = load_started_at.elapsed().as_millis() as u64,
        normalized_source_rows = input.aggregation.normalized_source_rows,
        accounts = input.counts.accounts,
        tokens = input.counts.tokens,
        balances = input.counts.balances,
        "loaded normalized compiler input"
    );
    let checksum_started_at = Instant::now();
    let source_checksum = compute_source_checksum_from_aggregation(&input.aggregation);
    info!(
        component = "snapshot-compile",
        phase = "compute-checksum",
        elapsed_ms = checksum_started_at.elapsed().as_millis() as u64,
        source_checksum = %format!("{source_checksum:#x}"),
        "computed source checksum"
    );

    let claim_started_at = Instant::now();
    let (claim_entries, claim_root) =
        materialize_claim_entries_from_aggregation(&input.aggregation)?;
    info!(
        component = "snapshot-compile",
        phase = "materialize-claim",
        elapsed_ms = claim_started_at.elapsed().as_millis() as u64,
        claim_entries = claim_entries.len(),
        claim_root = %format!("{claim_root:#x}"),
        "materialized claim snapshot"
    );
    fs::create_dir_all(&req.out_dir).wrap_err_with(|| {
        format!(
            "failed to create artifact output dir {}",
            req.out_dir.display()
        )
    })?;
    let write_artifacts_started_at = Instant::now();
    write_artifacts(&req.out_dir, &input.tokens, &claim_entries)?;
    info!(
        component = "snapshot-compile",
        phase = "write-artifacts",
        elapsed_ms = write_artifacts_started_at.elapsed().as_millis() as u64,
        out_dir = %req.out_dir.display(),
        "wrote artifact csv files"
    );

    let manifest_started_at = Instant::now();
    let manifest = build_manifest(req, &input, source_checksum, claim_root, &claim_entries)?;
    write_manifest(&req.out_dir.join(artifacts::MANIFEST_JSON), &manifest)?;
    info!(
        component = "snapshot-compile",
        phase = "write-manifest",
        elapsed_ms = manifest_started_at.elapsed().as_millis() as u64,
        out_dir = %req.out_dir.display(),
        claim_entries = claim_entries.len(),
        "wrote manifest"
    );
    info!(
        component = "snapshot-compile",
        phase = "complete",
        elapsed_ms = compile_started_at.elapsed().as_millis() as u64,
        out_dir = %req.out_dir.display(),
        source_checksum = %format!("{source_checksum:#x}"),
        "completed compile"
    );

    Ok(CompileSnapshotResult {
        manifest,
        out_dir: req.out_dir.clone(),
    })
}

pub fn compute_source_checksum(source_rows: &[SourceClaimRow]) -> B256 {
    let checksum_rows = aggregate_checksum_rows(source_rows);
    compute_source_checksum_from_aggregation_rows(&checksum_rows)
}

pub fn materialize_claim_entries(
    source_rows: Vec<SourceClaimRow>,
) -> eyre::Result<(Vec<MaterializedClaimEntry>, B256)> {
    let claim_aggregates = aggregate_claim_rows(&source_rows)?;
    materialize_claim_entries_from_claim_aggregates(&claim_aggregates)
}

fn compute_source_checksum_from_aggregation(aggregation: &SnapshotAggregation) -> B256 {
    compute_source_checksum_from_aggregation_rows(&aggregation.checksum_rows)
}

fn compute_source_checksum_from_aggregation_rows(
    checksum_rows: &BTreeMap<ChecksumRowKey, usize>,
) -> B256 {
    let mut hasher = Keccak256::new();
    hasher.update(b"rows=");
    let mut is_first_row = true;

    for (row, count) in checksum_rows {
        for _ in 0..*count {
            if !is_first_row {
                hasher.update(b";");
            }
            hasher.update(row.claimant_address.as_bytes());
            hasher.update(b"|");
            hasher.update(row.token_address.as_bytes());
            hasher.update(b"|");
            hasher.update(row.amount.as_bytes());
            is_first_row = false;
        }
    }

    let digest: [u8; 32] = hasher.finalize().into();
    B256::from(digest)
}

fn build_snapshot_aggregation(source_rows: &[SourceClaimRow]) -> eyre::Result<SnapshotAggregation> {
    let mut aggregation = SnapshotAggregation::default();
    for row in source_rows {
        aggregation.push_row(row)?;
    }
    Ok(aggregation)
}

fn aggregate_checksum_rows(source_rows: &[SourceClaimRow]) -> BTreeMap<ChecksumRowKey, usize> {
    let mut checksum_rows = BTreeMap::new();
    for row in source_rows {
        let checksum_row = ChecksumRowKey {
            claimant_address: format!("{:#x}", row.claimant_address),
            token_address: format!("{:#x}", row.token_address),
            amount: row.amount.to_string(),
        };
        *checksum_rows.entry(checksum_row).or_default() += 1;
    }
    checksum_rows
}

fn aggregate_claim_rows(source_rows: &[SourceClaimRow]) -> eyre::Result<ClaimAggregateMap> {
    let aggregation = build_snapshot_aggregation(source_rows)?;
    Ok(aggregation.claim_aggregates)
}

fn materialize_claim_entries_from_aggregation(
    aggregation: &SnapshotAggregation,
) -> eyre::Result<(Vec<MaterializedClaimEntry>, B256)> {
    materialize_claim_entries_from_claim_aggregates(&aggregation.claim_aggregates)
}

fn materialize_claim_entries_from_claim_aggregates(
    claim_aggregates: &ClaimAggregateMap,
) -> eyre::Result<(Vec<MaterializedClaimEntry>, B256)> {
    ensure!(
        !claim_aggregates.is_empty(),
        "no claim rows materialized from normalized source rows"
    );

    let ordered_aggregates: Vec<(Address, Address, U256)> = claim_aggregates
        .iter()
        .map(|(&(claimant, token), &amount)| (claimant, token, amount))
        .collect();
    let tree = ClaimTree::new(build_claim_leaves(&ordered_aggregates))
        .map_err(|err| eyre!("failed to build claim merkle tree: {err:#}"))?;

    let entries = ordered_aggregates
        .par_iter()
        .map(|&(claimant_address, token_address, amount)| {
            let proof = tree
                .proof(claimant_address, token_address)
                .map_err(|err| eyre!("failed to compute claim merkle proof: {err:#}"))?;
            Ok(MaterializedClaimEntry {
                claim_index: proof.claim_index,
                claimant_address,
                token_address,
                amount,
                leaf_hash: proof.leaf,
                proof_hashes: proof.merkle_path,
            })
        })
        .collect::<eyre::Result<Vec<_>>>()?;

    Ok((entries, tree.root()))
}

fn load_normalized_snapshot_input(data_dir: &Path) -> eyre::Result<NormalizedSnapshotInput> {
    let accounts_path = data_dir.join("accounts.csv");
    let tokens_path = data_dir.join("tokens.csv");
    let balances_path = data_dir.join("balances.csv");

    let started_at = Instant::now();
    let accounts = load_accounts(&accounts_path)?;
    info!(
        component = "snapshot-compile",
        phase = "load-accounts",
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %accounts_path.display(),
        rows = accounts.len(),
        "loaded accounts.csv"
    );
    let started_at = Instant::now();
    let tokens = load_tokens(&tokens_path)?;
    info!(
        component = "snapshot-compile",
        phase = "load-tokens",
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %tokens_path.display(),
        rows = tokens.len(),
        "loaded tokens.csv"
    );
    let tokens_by_id: HashMap<i64, Address> = tokens
        .iter()
        .map(|token| (token.id, token.address))
        .collect();
    let started_at = Instant::now();
    let source_rows = load_source_rows(&balances_path, &accounts, &tokens_by_id)?;
    info!(
        component = "snapshot-compile",
        phase = "normalize-balances",
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %balances_path.display(),
        balance_rows = source_rows.balance_rows,
        normalized_source_rows = source_rows.aggregation.normalized_source_rows,
        skipped_missing_account_rows = source_rows.skipped_missing_account_rows,
        skipped_missing_token_rows = source_rows.skipped_missing_token_rows,
        "loaded balances.csv and normalized source rows"
    );

    Ok(NormalizedSnapshotInput {
        aggregation: source_rows.aggregation,
        counts: InputRowCounts {
            accounts: accounts.len(),
            tokens: tokens.len(),
            balances: source_rows.balance_rows,
        },
        tokens,
        source_files: vec![
            build_source_file_descriptor(&accounts_path)?,
            build_source_file_descriptor(&tokens_path)?,
            build_source_file_descriptor(&balances_path)?,
        ],
    })
}

fn load_accounts(path: &Path) -> eyre::Result<HashMap<i64, Address>> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open accounts csv {}", path.display()))?;
    let mut accounts = HashMap::new();
    for row in reader.deserialize::<AccountCsvRow>() {
        let row = row.wrap_err("failed to deserialize accounts.csv row")?;
        let address = row
            .address
            .parse()
            .wrap_err_with(|| format!("invalid address in accounts.csv: {}", row.address))?;
        ensure!(
            accounts.insert(row.id, address).is_none(),
            "duplicate account id in accounts.csv: {}",
            row.id
        );
    }
    Ok(accounts)
}

fn load_tokens(path: &Path) -> eyre::Result<Vec<SnapshotTokenRow>> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open tokens csv {}", path.display()))?;
    let mut tokens = Vec::new();
    let mut seen_ids = BTreeMap::new();
    for row in reader.deserialize::<TokenCsvRow>() {
        let row = row.wrap_err("failed to deserialize tokens.csv row")?;
        let address: Address = row
            .address
            .parse()
            .wrap_err_with(|| format!("invalid address in tokens.csv: {}", row.address))?;
        ensure!(
            seen_ids.insert(row.id, address).is_none(),
            "duplicate token id in tokens.csv: {}",
            row.id
        );
        tokens.push(SnapshotTokenRow {
            id: row.id,
            address,
            symbol: row.symbol,
            decimals: row.decimals,
        });
    }
    Ok(tokens)
}

struct LoadedSourceRows {
    aggregation: SnapshotAggregation,
    balance_rows: usize,
    skipped_missing_account_rows: usize,
    skipped_missing_token_rows: usize,
}

fn load_source_rows(
    path: &Path,
    accounts: &HashMap<i64, Address>,
    tokens: &HashMap<i64, Address>,
) -> eyre::Result<LoadedSourceRows> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open balances csv {}", path.display()))?;
    let mut aggregation = SnapshotAggregation::default();
    let mut balance_rows = 0usize;
    let mut skipped_missing_account_rows = 0usize;
    let mut skipped_missing_token_rows = 0usize;
    let mut next_progress_log = 1_000_000usize;
    let progress_started_at = Instant::now();

    for row in reader.deserialize::<BalanceCsvRow>() {
        let row = row.wrap_err("failed to deserialize balances.csv row")?;
        balance_rows += 1;
        if balance_rows >= next_progress_log {
            info!(
                component = "snapshot-compile",
                phase = "normalize-balances-progress",
                balance_rows_processed = balance_rows,
                normalized_source_rows = aggregation.normalized_source_rows,
                skipped_missing_account_rows,
                skipped_missing_token_rows,
                elapsed_ms = progress_started_at.elapsed().as_millis() as u64,
                path = %path.display(),
                "still normalizing balances.csv"
            );
            next_progress_log += 1_000_000;
        }
        let amount = U256::from_str_radix(&row.balance, 10)
            .wrap_err_with(|| format!("invalid balance in balances.csv: {}", row.balance))?;

        let Some(claimant_address) = accounts.get(&row.account_id).copied() else {
            skipped_missing_account_rows += 1;
            continue;
        };
        let Some(token_address) = tokens.get(&row.coin_id).copied() else {
            skipped_missing_token_rows += 1;
            continue;
        };

        aggregation.push_row(&SourceClaimRow {
            claimant_address,
            token_address,
            amount,
        })?;
    }

    Ok(LoadedSourceRows {
        aggregation,
        balance_rows,
        skipped_missing_account_rows,
        skipped_missing_token_rows,
    })
}

fn build_source_file_descriptor(path: &Path) -> eyre::Result<SnapshotSourceFile> {
    Ok(SnapshotSourceFile {
        path: path
            .file_name()
            .ok_or_else(|| eyre!("invalid source file path {}", path.display()))?
            .to_string_lossy()
            .to_string(),
        digest: sha256_file(path)?,
    })
}

fn sha256_file(path: &Path) -> eyre::Result<String> {
    let file =
        File::open(path).wrap_err_with(|| format!("failed to open file {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 16 * 1024];

    loop {
        let read = reader
            .read(&mut buf)
            .wrap_err_with(|| format!("failed to read file {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn build_manifest(
    req: &CompileSnapshotRequest,
    input: &NormalizedSnapshotInput,
    source_checksum: B256,
    claim_root: B256,
    claim_entries: &[MaterializedClaimEntry],
) -> eyre::Result<SnapshotManifest> {
    Ok(SnapshotManifest {
        schema_version: 1,
        snapshot_label: req.snapshot_label.clone(),
        created_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .wrap_err("failed to format manifest created_at timestamp")?,
        source_checksum,
        source_files: input.source_files.clone(),
        row_counts: SnapshotRowCounts {
            accounts: input.counts.accounts,
            tokens: input.counts.tokens,
            balances: input.counts.balances,
            normalized_source_rows: input.aggregation.normalized_source_rows,
            claim_entries: claim_entries.len(),
        },
        roots: SnapshotRoots { claim_root },
        artifacts: SnapshotArtifactFiles {
            manifest: artifacts::MANIFEST_JSON.to_string(),
            tokens: artifacts::TOKENS_CSV.to_string(),
            claim_entries: artifacts::CLAIM_ENTRIES_CSV.to_string(),
            claim_proofs: artifacts::CLAIM_PROOFS_CSV.to_string(),
        },
        compiler: SnapshotCompilerInfo {
            package_name: env!("CARGO_PKG_NAME").to_string(),
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            git_commit: std::env::var("GIT_COMMIT").ok(),
        },
    })
}

fn write_manifest(path: &Path, manifest: &SnapshotManifest) -> eyre::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(manifest)?)
        .wrap_err_with(|| format!("failed to write manifest {}", path.display()))
}

fn write_artifacts(
    out_dir: &Path,
    tokens: &[SnapshotTokenRow],
    claim_entries: &[MaterializedClaimEntry],
) -> eyre::Result<()> {
    write_tokens(out_dir, tokens)?;
    write_claim_entries(out_dir, claim_entries)?;
    write_claim_proofs(out_dir, claim_entries)?;
    Ok(())
}

fn write_tokens(out_dir: &Path, tokens: &[SnapshotTokenRow]) -> eyre::Result<()> {
    let path = out_dir.join(artifacts::TOKENS_CSV);
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to open artifact {}", path.display()))?;
    for row in tokens {
        writer
            .serialize(TokenArtifactRow {
                id: row.id,
                address: format!("{:#x}", row.address),
                symbol: row.symbol.clone(),
                decimals: row.decimals,
            })
            .wrap_err("failed to serialize token artifact row")?;
    }
    writer
        .flush()
        .wrap_err_with(|| format!("failed to flush artifact {}", path.display()))
}

fn write_claim_entries(out_dir: &Path, entries: &[MaterializedClaimEntry]) -> eyre::Result<()> {
    let path = out_dir.join(artifacts::CLAIM_ENTRIES_CSV);
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to open artifact {}", path.display()))?;
    for entry in entries {
        writer
            .serialize(ClaimEntryArtifactRow {
                claim_index: entry.claim_index,
                claimant_address: format!("{:#x}", entry.claimant_address),
                token_address: format!("{:#x}", entry.token_address),
                amount: entry.amount.to_string(),
                leaf_hash: format!("{:#x}", entry.leaf_hash),
            })
            .wrap_err("failed to serialize claim entry artifact row")?;
    }
    writer
        .flush()
        .wrap_err_with(|| format!("failed to flush artifact {}", path.display()))
}

fn write_claim_proofs(out_dir: &Path, entries: &[MaterializedClaimEntry]) -> eyre::Result<()> {
    let path = out_dir.join(artifacts::CLAIM_PROOFS_CSV);
    let mut writer = csv::Writer::from_path(&path)
        .wrap_err_with(|| format!("failed to open artifact {}", path.display()))?;
    for entry in entries {
        writer
            .serialize(ClaimProofArtifactRow {
                claim_index: entry.claim_index,
                proof_hashes: pg_text_array_from_hashes(&entry.proof_hashes),
            })
            .wrap_err("failed to serialize claim proof artifact row")?;
    }
    writer
        .flush()
        .wrap_err_with(|| format!("failed to flush artifact {}", path.display()))
}

fn pg_text_array_from_hashes(hashes: &[B256]) -> String {
    let inner = hashes
        .iter()
        .map(|hash| format!("{hash:#x}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{inner}}}")
}

fn checked_add_amount(total: &mut U256, amount: U256, context: &str) -> eyre::Result<()> {
    let (sum, overflow) = total.overflowing_add(amount);
    ensure!(!overflow, "uint256 overflow while {context}");
    *total = sum;
    Ok(())
}

fn build_claim_leaves(ordered_aggregates: &[(Address, Address, U256)]) -> Vec<Leaf> {
    ordered_aggregates
        .par_iter()
        .enumerate()
        .map(
            |(claim_index, &(account_address, token_address, balance))| Leaf {
                claim_index: claim_index as u64,
                account_address,
                token_address,
                balance,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{CompileSnapshotRequest, compile_snapshot, materialize_claim_entries};
    use crate::model::SourceClaimRow;
    use alloy::primitives::{Address, U256};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    fn write(path: &Path, body: &str) {
        fs::write(path, body).expect("failed to write fixture file");
    }

    #[test]
    fn compile_snapshot_emits_manifest_and_artifacts() {
        let data_dir = unique_temp_dir("snapshot-compile-data");
        let out_dir = unique_temp_dir("snapshot-compile-out");
        fs::create_dir_all(&data_dir).expect("failed to create data dir");

        write(
            &data_dir.join("accounts.csv"),
            "id,nonce,address,pubkey_hash\n1,0,0x1111111111111111111111111111111111111111,0x0\n2,0,0x2222222222222222222222222222222222222222,0x0\n",
        );
        write(
            &data_dir.join("tokens.csv"),
            "id,address,symbol,decimals\n0,0x0000000000000000000000000000000000000000,ETH,18\n1,0x6b175474e89094c44da98b954eedeac495271d0f,DAI,18\n",
        );
        write(
            &data_dir.join("balances.csv"),
            "account_id,coin_id,balance\n1,0,100\n1,1,200\n2,0,300\n",
        );

        let result = compile_snapshot(&CompileSnapshotRequest {
            data_dir: data_dir.clone(),
            out_dir: out_dir.clone(),
            snapshot_label: "snapshot-test".to_string(),
        })
        .expect("snapshot compile should succeed");

        assert_eq!(result.manifest.snapshot_label, "snapshot-test");
        assert_eq!(result.manifest.row_counts.normalized_source_rows, 3);
        assert!(out_dir.join("manifest.json").exists());
        assert!(out_dir.join("tokens.csv").exists());
        assert!(out_dir.join("claim_entries.csv").exists());

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(out_dir);
    }

    #[test]
    fn one_pass_aggregation_preserves_claim_semantics() {
        let claimant_a: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .expect("valid address");
        let claimant_b: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .expect("valid address");
        let token_eth: Address = "0x0000000000000000000000000000000000000000"
            .parse()
            .expect("valid address");
        let token_dai: Address = "0x6b175474e89094c44da98b954eedeac495271d0f"
            .parse()
            .expect("valid address");

        let rows = vec![
            SourceClaimRow {
                claimant_address: claimant_a,
                token_address: token_eth,
                amount: U256::from(10_u64),
            },
            SourceClaimRow {
                claimant_address: claimant_a,
                token_address: token_eth,
                amount: U256::from(5_u64),
            },
            SourceClaimRow {
                claimant_address: claimant_b,
                token_address: token_eth,
                amount: U256::from(7_u64),
            },
            SourceClaimRow {
                claimant_address: claimant_b,
                token_address: token_dai,
                amount: U256::from(8_u64),
            },
        ];

        let (claim_entries, _claim_root) =
            materialize_claim_entries(rows).expect("claim materialization should succeed");

        assert_eq!(claim_entries.len(), 3);
        let merged_claim = claim_entries
            .iter()
            .find(|entry| entry.claimant_address == claimant_a && entry.token_address == token_eth)
            .expect("merged claim entry should exist");
        assert_eq!(merged_claim.amount, U256::from(15_u64));
    }
}
