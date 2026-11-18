use crate::config::Config;
use alloy::primitives::{B256, U256};
use eyre::WrapErr;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, QueryBuilder};
use std::num::NonZeroUsize;
use std::path::Path;
use tracing::info;
use url::Url;
use zksync_lite_sunset_snapshot::compiler::{compute_source_checksum, materialize_claim_entries};
use zksync_lite_sunset_snapshot::model::{MaterializedClaimEntry, SourceClaimRow};

const POSTGRES_BIND_LIMIT: usize = 65_535;
const CLAIM_ENTRY_BIND_COUNT: usize = 5;
const CLAIM_PROOF_BIND_COUNT: usize = 2;
const CLAIM_ENTRY_BATCH_SIZE_CAP: usize = 2_000;
const CLAIM_PROOF_BATCH_SIZE_CAP: usize = 1_000;
const IMPORT_PROGRESS_INTERVAL: usize = 500_000;
const MATERIALIZED_PROGRESS_INTERVAL: usize = 250_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaimMaterializationRequest {
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaimMaterializationResult {
    pub label: String,
    pub merkle_root: B256,
    pub source_checksum: B256,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SnapshotImportCounts {
    pub accounts: usize,
    pub tokens: usize,
    pub balances: usize,
}

pub(crate) async fn connect(config: &Config) -> eyre::Result<PgPool> {
    let pool = connect_pool(&config.database.url, 10).await?;

    if config.database.auto_migrate {
        run_migrations(&pool).await?;
    }

    Ok(pool)
}

fn rows_per_insert(bind_count_per_row: usize) -> usize {
    (POSTGRES_BIND_LIMIT / bind_count_per_row).max(1)
}

fn capped_rows_per_insert(bind_count_per_row: usize, batch_size_cap: usize) -> usize {
    rows_per_insert(bind_count_per_row)
        .min(batch_size_cap)
        .max(1)
}

fn log_import_progress(kind: &str, imported: usize) {
    if imported != 0 && imported.is_multiple_of(IMPORT_PROGRESS_INTERVAL) {
        info!(
            component = "snapshot-import",
            dataset = kind,
            imported_rows = imported,
            "import progress"
        );
    }
}

fn log_materialized_progress(kind: &str, inserted: usize, total: usize, batch_rows: usize) {
    if inserted == total || inserted.is_multiple_of(MATERIALIZED_PROGRESS_INTERVAL) {
        info!(
            component = "snapshot-persist",
            phase = kind,
            inserted_rows = inserted,
            total_rows = total,
            batch_rows,
            "persistence progress"
        );
    }
}

pub(crate) async fn connect_pool(database_url: &Url, max_connections: u32) -> eyre::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres")
}

pub(crate) async fn run_migrations(pool: &PgPool) -> eyre::Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .wrap_err("failed to run postgres migrations")
}

pub(crate) async fn init_database(database_url: &Url) -> eyre::Result<()> {
    let pool = connect_pool(database_url, 5).await?;
    run_migrations(&pool).await
}

pub(crate) async fn sum_claim_entries_amount_for_token(
    database_url: &Url,
    token_address: &str,
) -> eyre::Result<String> {
    let pool = connect_pool(database_url, 1).await?;
    let row: (String,) = sqlx::query_as(
        r#"
        SELECT COALESCE(SUM(amount::numeric), 0)::text
        FROM claim_entries
        WHERE LOWER(token_address) = LOWER($1)
        "#,
    )
    .bind(token_address)
    .fetch_one(&pool)
    .await
    .wrap_err("failed to sum claim_entries.amount")?;
    Ok(row.0)
}

pub(crate) async fn reset_runtime_state(pool: &PgPool) -> eyre::Result<()> {
    info!(
        component = "snapshot-import",
        "truncating runtime state tables"
    );
    sqlx::query(
        r#"
        TRUNCATE TABLE
            migration_requests,
            claim_requests
        CASCADE
        "#,
    )
    .execute(pool)
    .await
    .wrap_err("failed to truncate runtime state tables")?;
    Ok(())
}

pub(crate) async fn import_snapshots(
    database_url: &Url,
    data_dir: &Path,
    chunk_size: NonZeroUsize,
    truncate: bool,
) -> eyre::Result<SnapshotImportCounts> {
    let pool = connect_pool(database_url, 5).await?;
    run_migrations(&pool).await?;
    import_snapshots_with_pool(&pool, data_dir, chunk_size, truncate).await
}

pub(crate) async fn import_snapshots_with_pool(
    pool: &PgPool,
    data_dir: &Path,
    chunk_size: NonZeroUsize,
    truncate: bool,
) -> eyre::Result<SnapshotImportCounts> {
    let chunk_size = chunk_size.get();
    info!(
        component = "snapshot-import",
        data_dir = %data_dir.display(),
        chunk_size,
        truncate,
        "starting snapshot import"
    );
    if truncate {
        info!(
            component = "snapshot-import",
            "truncating raw snapshot tables"
        );
        sqlx::query(
            r#"
            TRUNCATE TABLE
                snapshot_balances,
                snapshot_tokens,
                snapshot_accounts
            "#,
        )
        .execute(pool)
        .await
        .wrap_err("failed to truncate snapshot tables")?;
    }

    info!(
        component = "snapshot-import",
        dataset = "accounts",
        "importing dataset"
    );
    let accounts = import_accounts(pool, &data_dir.join("accounts.csv"), chunk_size).await?;
    info!(
        component = "snapshot-import",
        dataset = "tokens",
        "importing dataset"
    );
    let tokens = import_tokens(pool, &data_dir.join("tokens.csv"), chunk_size).await?;
    info!(
        component = "snapshot-import",
        dataset = "balances",
        "importing dataset"
    );
    let balances = import_balances(pool, &data_dir.join("balances.csv"), chunk_size).await?;
    info!(
        component = "snapshot-import",
        accounts, tokens, balances, "completed snapshot import"
    );

    Ok(SnapshotImportCounts {
        accounts,
        tokens,
        balances,
    })
}

pub(crate) async fn materialize_claim_snapshot_with_pool(
    pool: &PgPool,
    req: &ClaimMaterializationRequest,
) -> eyre::Result<ClaimMaterializationResult> {
    info!(
        component = "claim-materialize",
        snapshot_label = %req.label,
        "loading source rows"
    );
    let source_rows = load_source_claim_rows(pool).await?;
    info!(
        component = "claim-materialize",
        snapshot_label = %req.label,
        source_rows = source_rows.len(),
        "loaded source rows"
    );
    let source_checksum = compute_source_checksum(&source_rows);
    info!(
        component = "claim-materialize",
        snapshot_label = %req.label,
        source_checksum = %format!("{source_checksum:#x}"),
        "computed source checksum"
    );
    info!(
        component = "claim-materialize",
        "building claim entries and proofs"
    );
    let (entries, merkle_root) = materialize_claim_entries(source_rows)?;
    info!(
        component = "claim-materialize",
        entry_count = entries.len(),
        merkle_root = %format!("{merkle_root:#x}"),
        "built claim entries and proofs"
    );

    persist_materialized_snapshot(pool, &req.label, source_checksum, merkle_root, entries).await
}

#[derive(Debug, Deserialize)]
struct AccountRow {
    id: i64,
    nonce: i64,
    address: String,
    pubkey_hash: String,
}

#[derive(Debug, Deserialize)]
struct TokenRow {
    id: i64,
    address: String,
    symbol: String,
    decimals: i16,
}

#[derive(Debug, Deserialize)]
struct BalanceRow {
    account_id: i64,
    coin_id: i64,
    balance: String,
}

#[derive(Debug, sqlx::FromRow)]
struct SourceClaimRowDb {
    claimant_address: String,
    token_address: String,
    amount: String,
}

async fn load_source_claim_rows(pool: &PgPool) -> eyre::Result<Vec<SourceClaimRow>> {
    let rows = sqlx::query_as::<_, SourceClaimRowDb>(
        r#"
        SELECT
            LOWER(sa.address) AS claimant_address,
            LOWER(st.address) AS token_address,
            sb.balance::text AS amount
        FROM snapshot_balances sb
        INNER JOIN snapshot_accounts sa ON sa.id = sb.account_id
        INNER JOIN snapshot_tokens st ON st.id = sb.coin_id
        "#,
    )
    .fetch_all(pool)
    .await
    .wrap_err("failed to load source claim rows from snapshot tables")?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(SourceClaimRow {
            claimant_address: row.claimant_address.parse().wrap_err_with(|| {
                format!(
                    "invalid claimant address in snapshot_accounts: {}",
                    row.claimant_address
                )
            })?,
            token_address: row.token_address.parse().wrap_err_with(|| {
                format!(
                    "invalid token address in snapshot_tokens: {}",
                    row.token_address
                )
            })?,
            amount: U256::from_str_radix(&row.amount, 10)
                .wrap_err_with(|| format!("invalid amount in snapshot_balances: {}", row.amount))?,
        });
    }

    Ok(out)
}

async fn persist_materialized_snapshot(
    pool: &PgPool,
    label: &str,
    source_checksum: B256,
    merkle_root: B256,
    entries: Vec<MaterializedClaimEntry>,
) -> eyre::Result<ClaimMaterializationResult> {
    let mut tx = pool
        .begin()
        .await
        .wrap_err("failed to begin claim materialization transaction")?;
    let total_entries = entries.len();

    sqlx::query("TRUNCATE TABLE claim_proofs, claim_entries CASCADE")
        .execute(tx.as_mut())
        .await
        .wrap_err("failed to truncate existing claim tables before materialization")?;

    sqlx::query(
        r#"
        INSERT INTO claim_snapshot_metadata (singleton, label, merkle_root, source_checksum, updated_at)
        VALUES (TRUE, $1, $2, $3, NOW())
        ON CONFLICT (singleton) DO UPDATE SET
            label = EXCLUDED.label,
            merkle_root = EXCLUDED.merkle_root,
            source_checksum = EXCLUDED.source_checksum,
            updated_at = NOW()
        "#,
    )
    .bind(label)
    .bind(format!("{merkle_root:#x}"))
    .bind(format!("{source_checksum:#x}"))
    .execute(tx.as_mut())
    .await
    .wrap_err("failed to upsert claim snapshot metadata")?;

    info!(
        component = "snapshot-persist",
        phase = "claim-snapshot",
        total_entries,
        "cleared existing claim rows and upserted snapshot metadata"
    );

    if !entries.is_empty() {
        let claim_entry_batch_size =
            capped_rows_per_insert(CLAIM_ENTRY_BIND_COUNT, CLAIM_ENTRY_BATCH_SIZE_CAP);
        info!(
            component = "snapshot-persist",
            phase = "claim-entries",
            total_rows = total_entries,
            batch_size = claim_entry_batch_size,
            "persisting claim entries"
        );
        let mut inserted_claim_entries = 0usize;
        for batch in entries.chunks(claim_entry_batch_size) {
            let mut entries_builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO claim_entries (claim_index, claimant_address, token_address, amount, leaf_hash) ",
            );

            entries_builder.push_values(batch, |mut b, entry| {
                b.push_bind(entry.claim_index as i64)
                    .push_bind(format!("{:#x}", entry.claimant_address))
                    .push_bind(format!("{:#x}", entry.token_address))
                    .push_bind(entry.amount.to_string())
                    .push_bind(format!("{:#x}", entry.leaf_hash));
            });
            entries_builder
                .build()
                .execute(tx.as_mut())
                .await
                .wrap_err("failed to insert claim entries")?;
            inserted_claim_entries += batch.len();
            log_materialized_progress(
                "claim-entries",
                inserted_claim_entries,
                total_entries,
                batch.len(),
            );
        }

        let claim_proof_batch_size =
            capped_rows_per_insert(CLAIM_PROOF_BIND_COUNT, CLAIM_PROOF_BATCH_SIZE_CAP);
        info!(
            component = "snapshot-persist",
            phase = "claim-proofs",
            total_rows = total_entries,
            batch_size = claim_proof_batch_size,
            "persisting claim proofs"
        );
        let mut inserted_claim_proofs = 0usize;
        for batch in entries.chunks(claim_proof_batch_size) {
            let mut proofs_builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO claim_proofs (claim_index, proof_hashes) ",
            );
            proofs_builder.push_values(batch, |mut b, entry| {
                let proof_hashes = entry
                    .proof_hashes
                    .iter()
                    .map(|hash| format!("{hash:#x}"))
                    .collect::<Vec<_>>();
                b.push_bind(entry.claim_index as i64)
                    .push_bind(proof_hashes);
            });
            proofs_builder
                .build()
                .execute(tx.as_mut())
                .await
                .wrap_err("failed to insert claim proofs")?;
            inserted_claim_proofs += batch.len();
            log_materialized_progress(
                "claim-proofs",
                inserted_claim_proofs,
                total_entries,
                batch.len(),
            );
        }
    }

    tx.commit()
        .await
        .wrap_err("failed to commit claim materialization transaction")?;
    info!(
        component = "snapshot-persist",
        phase = "claim-snapshot",
        total_entries,
        "committed claim snapshot transaction"
    );

    Ok(ClaimMaterializationResult {
        label: label.to_string(),
        merkle_root,
        source_checksum,
        entry_count: total_entries,
    })
}

async fn import_accounts(pool: &PgPool, path: &Path, chunk_size: usize) -> eyre::Result<usize> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open {}", path.display()))?;
    let mut rows = Vec::with_capacity(chunk_size);
    let mut imported = 0usize;

    for row in reader.deserialize::<AccountRow>() {
        rows.push(row.wrap_err("failed to parse accounts.csv row")?);
        if rows.len() >= chunk_size {
            upsert_accounts(pool, &rows).await?;
            imported += rows.len();
            log_import_progress("accounts", imported);
            rows.clear();
        }
    }

    if !rows.is_empty() {
        upsert_accounts(pool, &rows).await?;
        imported += rows.len();
        log_import_progress("accounts", imported);
    }

    Ok(imported)
}

async fn import_tokens(pool: &PgPool, path: &Path, chunk_size: usize) -> eyre::Result<usize> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open {}", path.display()))?;
    let mut rows = Vec::with_capacity(chunk_size);
    let mut imported = 0usize;

    for row in reader.deserialize::<TokenRow>() {
        rows.push(row.wrap_err("failed to parse tokens.csv row")?);
        if rows.len() >= chunk_size {
            upsert_tokens(pool, &rows).await?;
            imported += rows.len();
            log_import_progress("tokens", imported);
            rows.clear();
        }
    }

    if !rows.is_empty() {
        upsert_tokens(pool, &rows).await?;
        imported += rows.len();
        log_import_progress("tokens", imported);
    }

    Ok(imported)
}

async fn import_balances(pool: &PgPool, path: &Path, chunk_size: usize) -> eyre::Result<usize> {
    let mut reader = csv::Reader::from_path(path)
        .wrap_err_with(|| format!("failed to open {}", path.display()))?;
    let mut rows = Vec::with_capacity(chunk_size);
    let mut imported = 0usize;

    for row in reader.deserialize::<BalanceRow>() {
        rows.push(row.wrap_err("failed to parse balances.csv row")?);
        if rows.len() >= chunk_size {
            upsert_balances(pool, &rows).await?;
            imported += rows.len();
            log_import_progress("balances", imported);
            rows.clear();
        }
    }

    if !rows.is_empty() {
        upsert_balances(pool, &rows).await?;
        imported += rows.len();
        log_import_progress("balances", imported);
    }

    Ok(imported)
}

async fn upsert_accounts(pool: &PgPool, rows: &[AccountRow]) -> eyre::Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO snapshot_accounts (id, nonce, address, pubkey_hash) ",
    );
    builder.push_values(rows, |mut b, row| {
        b.push_bind(row.id)
            .push_bind(row.nonce)
            .push_bind(&row.address)
            .push_bind(&row.pubkey_hash);
    });
    builder.push(
        " ON CONFLICT (id) DO UPDATE SET nonce = EXCLUDED.nonce, address = EXCLUDED.address, pubkey_hash = EXCLUDED.pubkey_hash",
    );
    builder
        .build()
        .execute(pool)
        .await
        .wrap_err("failed to insert accounts")?;
    Ok(())
}

async fn upsert_tokens(pool: &PgPool, rows: &[TokenRow]) -> eyre::Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO snapshot_tokens (id, address, symbol, decimals) ",
    );
    builder.push_values(rows, |mut b, row| {
        b.push_bind(row.id)
            .push_bind(&row.address)
            .push_bind(&row.symbol)
            .push_bind(row.decimals);
    });
    builder.push(
        " ON CONFLICT (id) DO UPDATE SET address = EXCLUDED.address, symbol = EXCLUDED.symbol, decimals = EXCLUDED.decimals",
    );
    builder
        .build()
        .execute(pool)
        .await
        .wrap_err("failed to insert tokens")?;
    Ok(())
}

async fn upsert_balances(pool: &PgPool, rows: &[BalanceRow]) -> eyre::Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO snapshot_balances (account_id, coin_id, balance) ",
    );
    builder.push_values(rows, |mut b, row| {
        b.push_bind(row.account_id)
            .push_bind(row.coin_id)
            .push_bind(&row.balance)
            .push_unseparated("::numeric");
    });
    builder.push(" ON CONFLICT (account_id, coin_id) DO UPDATE SET balance = EXCLUDED.balance");
    builder
        .build()
        .execute(pool)
        .await
        .wrap_err("failed to insert balances")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{capped_rows_per_insert, rows_per_insert};

    #[test]
    fn rows_per_insert_stays_under_postgres_bind_limit() {
        assert_eq!(rows_per_insert(6), 10_922);
        assert_eq!(rows_per_insert(3), 21_845);
        assert_eq!(rows_per_insert(100_000), 1);
    }

    #[test]
    fn capped_rows_per_insert_prefers_safer_batch_caps() {
        assert_eq!(capped_rows_per_insert(6, 2_000), 2_000);
        assert_eq!(capped_rows_per_insert(3, 1_000), 1_000);
        assert_eq!(capped_rows_per_insert(100_000, 1_000), 1);
    }
}
