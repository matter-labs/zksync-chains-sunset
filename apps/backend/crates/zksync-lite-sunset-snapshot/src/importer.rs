use crate::artifacts;
use crate::manifest::SnapshotManifest;
use eyre::{WrapErr, ensure};
use postgres::{Client, GenericClient, NoTls, Transaction};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read, copy};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::task;
use tracing::info;
use url::Url;

const RESET_RUNTIME_STATE_SQL: &str = r#"
TRUNCATE TABLE
    migration_requests,
    claim_requests
CASCADE
"#;

const COPY_PROGRESS_BYTES_INTERVAL: u64 = 128 * 1024 * 1024;
const COPY_PROGRESS_TIME_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSnapshotArtifactsRequest {
    pub database_url: Url,
    pub artifact_dir: PathBuf,
    pub reset_runtime_state: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSnapshotArtifactsResult {
    pub artifact_dir: PathBuf,
    pub snapshot_label: String,
    pub source_checksum: alloy::primitives::B256,
    pub claim_entry_count: usize,
}

struct ProofImportSpec<'a> {
    phase: &'a str,
    temp_table_name: &'a str,
    table_name: &'a str,
    index_column_name: &'a str,
}

struct ProgressReader<R> {
    inner: R,
    phase: String,
    path: PathBuf,
    total_bytes: u64,
    bytes_read: u64,
    next_byte_checkpoint: u64,
    started_at: Instant,
    last_progress_at: Instant,
}

impl<R> ProgressReader<R> {
    fn new(inner: R, phase: impl Into<String>, path: &Path, total_bytes: u64) -> Self {
        let started_at = Instant::now();
        Self {
            inner,
            phase: phase.into(),
            path: path.to_path_buf(),
            total_bytes,
            bytes_read: 0,
            next_byte_checkpoint: COPY_PROGRESS_BYTES_INTERVAL,
            started_at,
            last_progress_at: started_at,
        }
    }

    fn log_progress_if_needed(&mut self) {
        let byte_threshold_reached = self.bytes_read >= self.next_byte_checkpoint;
        let time_threshold_reached = self.last_progress_at.elapsed() >= COPY_PROGRESS_TIME_INTERVAL;

        if !(byte_threshold_reached || time_threshold_reached) {
            return;
        }

        let progress_pct = if self.total_bytes == 0 {
            100.0
        } else {
            (self.bytes_read as f64 / self.total_bytes as f64) * 100.0
        };

        info!(
            component = "snapshot-import-artifacts",
            phase = %self.phase,
            path = %self.path.display(),
            bytes_read = self.bytes_read,
            total_bytes = self.total_bytes,
            progress_pct = progress_pct,
            elapsed_ms = self.started_at.elapsed().as_millis() as u64,
            "copy stream progress"
        );

        while self.bytes_read >= self.next_byte_checkpoint {
            self.next_byte_checkpoint += COPY_PROGRESS_BYTES_INTERVAL;
        }
        self.last_progress_at = Instant::now();
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            self.bytes_read += read as u64;
            self.log_progress_if_needed();
        }
        Ok(read)
    }
}

pub async fn import_snapshot_artifacts(
    req: &ImportSnapshotArtifactsRequest,
) -> eyre::Result<ImportSnapshotArtifactsResult> {
    let req = req.clone();
    task::spawn_blocking(move || import_snapshot_artifacts_blocking(&req))
        .await
        .wrap_err("artifact import task panicked")?
}

fn import_snapshot_artifacts_blocking(
    req: &ImportSnapshotArtifactsRequest,
) -> eyre::Result<ImportSnapshotArtifactsResult> {
    let import_started_at = Instant::now();
    let manifest = load_manifest(&req.artifact_dir)?;
    let mut client = Client::connect(req.database_url.as_str(), NoTls)
        .wrap_err("failed to connect to postgres for artifact import")?;

    info!(
        component = "snapshot-import-artifacts",
        artifact_dir = %req.artifact_dir.display(),
        snapshot_label = %manifest.snapshot_label,
        reset_runtime_state = req.reset_runtime_state,
        source_checksum = %format!("{:#x}", manifest.source_checksum),
        claim_entries = manifest.row_counts.claim_entries,
        "starting artifact import"
    );

    let mut tx = client
        .transaction()
        .wrap_err("failed to begin artifact import transaction")?;

    if req.reset_runtime_state {
        let started_at = Instant::now();
        reset_runtime_state(&mut tx)?;
        log_phase_complete(
            "reset-runtime-state",
            started_at,
            Some(0),
            Some(&req.artifact_dir),
        );
    }

    upsert_snapshot_tokens(
        &mut tx,
        &req.artifact_dir.join(artifacts::TOKENS_CSV),
        manifest.row_counts.tokens,
    )?;

    clear_existing_claims(&mut tx)?;
    upsert_claim_snapshot_metadata(&mut tx, &manifest)?;

    import_claim_entries(
        &mut tx,
        &req.artifact_dir.join(artifacts::CLAIM_ENTRIES_CSV),
        manifest.row_counts.claim_entries,
    )?;
    import_claim_proofs(
        &mut tx,
        &req.artifact_dir.join(artifacts::CLAIM_PROOFS_CSV),
        manifest.row_counts.claim_entries,
    )?;

    let started_at = Instant::now();
    tx.commit()
        .wrap_err("failed to commit artifact import transaction")?;
    info!(
        component = "snapshot-import-artifacts",
        phase = "commit",
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        "committed artifact import transaction"
    );

    info!(
        component = "snapshot-import-artifacts",
        phase = "complete",
        artifact_dir = %req.artifact_dir.display(),
        elapsed_ms = import_started_at.elapsed().as_millis() as u64,
        "completed artifact import"
    );

    Ok(build_import_result(&manifest, req.artifact_dir.clone()))
}

fn load_manifest(artifact_dir: &Path) -> eyre::Result<SnapshotManifest> {
    let manifest_path = artifact_dir.join(artifacts::MANIFEST_JSON);
    let file = std::fs::File::open(&manifest_path)
        .wrap_err_with(|| format!("failed to open manifest {}", manifest_path.display()))?;
    serde_json::from_reader(file)
        .wrap_err_with(|| format!("failed to deserialize manifest {}", manifest_path.display()))
}

fn build_import_result(
    manifest: &SnapshotManifest,
    artifact_dir: PathBuf,
) -> ImportSnapshotArtifactsResult {
    ImportSnapshotArtifactsResult {
        artifact_dir,
        snapshot_label: manifest.snapshot_label.clone(),
        source_checksum: manifest.source_checksum,
        claim_entry_count: manifest.row_counts.claim_entries,
    }
}

fn reset_runtime_state(client: &mut impl GenericClient) -> eyre::Result<()> {
    client
        .batch_execute(RESET_RUNTIME_STATE_SQL)
        .wrap_err("failed to reset runtime state tables before artifact import")
}

fn clear_existing_claims(tx: &mut Transaction<'_>) -> eyre::Result<()> {
    tx.batch_execute("TRUNCATE TABLE claim_proofs, claim_entries CASCADE")
        .wrap_err("failed to clear existing claim tables before artifact import")
}

fn upsert_claim_snapshot_metadata(
    tx: &mut Transaction<'_>,
    manifest: &SnapshotManifest,
) -> eyre::Result<()> {
    let merkle_root = format!("{:#x}", manifest.roots.claim_root);
    let source_checksum = format!("{:#x}", manifest.source_checksum);
    tx.execute(
        r#"
        INSERT INTO claim_snapshot_metadata (singleton, label, merkle_root, source_checksum, updated_at)
        VALUES (TRUE, $1, $2, $3, NOW())
        ON CONFLICT (singleton) DO UPDATE SET
            label = EXCLUDED.label,
            merkle_root = EXCLUDED.merkle_root,
            source_checksum = EXCLUDED.source_checksum,
            updated_at = NOW()
        "#,
        &[&manifest.snapshot_label, &merkle_root, &source_checksum],
    )
    .wrap_err("failed to upsert claim snapshot metadata from artifact manifest")?;
    Ok(())
}

fn upsert_snapshot_tokens(
    tx: &mut Transaction<'_>,
    path: &Path,
    expected_rows: usize,
) -> eyre::Result<()> {
    let phase = "copy-tokens";
    let started_at = Instant::now();
    tx.batch_execute(
        r#"
        CREATE TEMP TABLE snapshot_tokens_import (
            id BIGINT,
            address TEXT,
            symbol TEXT,
            decimals SMALLINT
        ) ON COMMIT DROP
        "#,
    )
    .wrap_err("failed to create snapshot tokens import table")?;
    let copied_rows = copy_csv_from_path(
        tx,
        phase,
        "COPY snapshot_tokens_import (id, address, symbol, decimals) FROM STDIN WITH (FORMAT csv, HEADER true)",
        path,
    )?;
    ensure_expected_rows(phase, expected_rows, copied_rows)?;
    let upserted_rows = tx
        .execute(
            r#"
            INSERT INTO snapshot_tokens (id, address, symbol, decimals)
            SELECT id, address, symbol, decimals
            FROM snapshot_tokens_import
            ON CONFLICT (id)
            DO UPDATE SET
                address = EXCLUDED.address,
                symbol = EXCLUDED.symbol,
                decimals = EXCLUDED.decimals
            "#,
            &[],
        )
        .wrap_err("failed to upsert snapshot token artifact rows")?;
    info!(
        component = "snapshot-import-artifacts",
        phase,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %path.display(),
        copied_rows,
        upserted_rows,
        "imported snapshot tokens artifact"
    );
    Ok(())
}

fn import_claim_entries(
    tx: &mut Transaction<'_>,
    path: &Path,
    expected_rows: usize,
) -> eyre::Result<()> {
    let phase = "copy-claim-entries";
    let started_at = Instant::now();
    tx.batch_execute(
        r#"
        CREATE TEMP TABLE claim_entries_import (
            claim_index BIGINT,
            claimant_address TEXT,
            token_address TEXT,
            amount TEXT,
            leaf_hash TEXT
        ) ON COMMIT DROP
        "#,
    )
    .wrap_err("failed to create claim entries import table")?;
    let copied_rows = copy_csv_from_path(
        tx,
        phase,
        "COPY claim_entries_import (claim_index, claimant_address, token_address, amount, leaf_hash) FROM STDIN WITH (FORMAT csv, HEADER true)",
        path,
    )?;
    ensure_expected_rows(phase, expected_rows, copied_rows)?;
    info!(
        component = "snapshot-import-artifacts",
        phase,
        path = %path.display(),
        expected_rows,
        "starting insert from temporary claim entries table"
    );
    let inserted_rows = tx
        .execute(
            r#"
            INSERT INTO claim_entries (
                claim_index,
                claimant_address,
                token_address,
                amount,
                leaf_hash
            )
            SELECT
                claim_index,
                claimant_address,
                token_address,
                amount,
                leaf_hash
            FROM claim_entries_import
            "#,
            &[],
        )
        .wrap_err("failed to insert claim entry artifact rows")?;
    ensure_expected_rows(phase, expected_rows, inserted_rows)?;
    info!(
        component = "snapshot-import-artifacts",
        phase,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %path.display(),
        copied_rows,
        inserted_rows,
        "imported claim entries artifact"
    );
    Ok(())
}

fn import_claim_proofs(
    tx: &mut Transaction<'_>,
    path: &Path,
    expected_rows: usize,
) -> eyre::Result<()> {
    import_proof_table(
        tx,
        &ProofImportSpec {
            phase: "copy-claim-proofs",
            temp_table_name: "claim_proofs_import",
            table_name: "claim_proofs",
            index_column_name: "claim_index",
        },
        path,
        expected_rows,
    )
}

fn import_proof_table(
    tx: &mut Transaction<'_>,
    spec: &ProofImportSpec<'_>,
    path: &Path,
    expected_rows: usize,
) -> eyre::Result<()> {
    let started_at = Instant::now();
    tx.batch_execute(&format!(
        r#"
        CREATE TEMP TABLE {temp_table_name} (
            {index_column_name} BIGINT,
            proof_hashes TEXT[]
        ) ON COMMIT DROP
        "#,
        temp_table_name = spec.temp_table_name,
        index_column_name = spec.index_column_name,
    ))
    .wrap_err_with(|| format!("failed to create {} import table", spec.table_name))?;
    let copied_rows = copy_csv_from_path(
        tx,
        spec.phase,
        &format!(
            "COPY {temp_table_name} ({index_column_name}, proof_hashes) FROM STDIN WITH (FORMAT csv, HEADER true)",
            temp_table_name = spec.temp_table_name,
            index_column_name = spec.index_column_name,
        ),
        path,
    )?;
    ensure_expected_rows(spec.phase, expected_rows, copied_rows)?;
    info!(
        component = "snapshot-import-artifacts",
        phase = spec.phase,
        path = %path.display(),
        expected_rows,
        "starting insert from temporary proof table"
    );
    let inserted_rows = tx
        .execute(
            &format!(
                r#"
                INSERT INTO {table_name} ({index_column_name}, proof_hashes)
                SELECT
                    {index_column_name},
                    proof_hashes
                FROM {temp_table_name}
                "#,
                table_name = spec.table_name,
                index_column_name = spec.index_column_name,
                temp_table_name = spec.temp_table_name,
            ),
            &[],
        )
        .wrap_err_with(|| format!("failed to insert {} artifact rows", spec.table_name))?;
    ensure_expected_rows(spec.phase, expected_rows, inserted_rows)?;
    info!(
        component = "snapshot-import-artifacts",
        phase = spec.phase,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        path = %path.display(),
        copied_rows,
        inserted_rows,
        "imported proof artifact"
    );
    Ok(())
}

fn copy_csv_from_path(
    tx: &mut Transaction<'_>,
    phase: &str,
    copy_sql: &str,
    path: &Path,
) -> eyre::Result<u64> {
    let file = File::open(path)
        .wrap_err_with(|| format!("failed to open artifact csv {}", path.display()))?;
    let total_bytes = file
        .metadata()
        .wrap_err_with(|| format!("failed to read metadata for {}", path.display()))?
        .len();
    info!(
        component = "snapshot-import-artifacts",
        phase,
        path = %path.display(),
        total_bytes,
        "starting copy from artifact csv"
    );
    let mut reader = ProgressReader::new(BufReader::new(file), phase, path, total_bytes);
    let mut writer = tx
        .copy_in(copy_sql)
        .wrap_err_with(|| format!("failed to start COPY for {}", path.display()))?;
    copy(&mut reader, &mut writer)
        .wrap_err_with(|| format!("failed to stream artifact csv {}", path.display()))?;
    info!(
        component = "snapshot-import-artifacts",
        phase,
        path = %path.display(),
        total_bytes,
        streamed_bytes = reader.bytes_read,
        elapsed_ms = reader.started_at.elapsed().as_millis() as u64,
        "finished streaming artifact csv into COPY"
    );
    writer
        .finish()
        .wrap_err_with(|| format!("failed to finish COPY for {}", path.display()))
}

fn ensure_expected_rows(phase: &str, expected_rows: usize, actual_rows: u64) -> eyre::Result<()> {
    ensure!(
        actual_rows == expected_rows as u64,
        "{phase} row count mismatch: expected {expected_rows}, got {actual_rows}"
    );
    Ok(())
}

fn log_phase_complete(phase: &str, started_at: Instant, rows: Option<u64>, path: Option<&Path>) {
    info!(
        component = "snapshot-import-artifacts",
        phase,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        rows,
        path = path.map(|value| value.display().to_string()),
        "completed phase"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompileSnapshotRequest, compile_snapshot};
    use sqlx::PgPool;
    use std::time::{SystemTime, UNIX_EPOCH};

    const DEFAULT_TEST_DATABASE_URL: &str =
        "postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test";

    static MIGRATOR: sqlx::migrate::Migrator =
        sqlx::migrate!("../zksync-lite-sunset-core/migrations");

    #[tokio::test]
    async fn import_snapshot_artifacts_replaces_previous_rows() -> eyre::Result<()> {
        let database_url = provision_test_database().await?;
        initialize_test_database(&database_url).await?;

        let fixture_dir = unique_temp_dir("snapshot-import-artifacts-fixture");
        write_fixture_csv(
            &fixture_dir,
            "accounts.csv",
            "id,nonce,address,pubkey_hash\n1,0,0xba98deefbca8e585208b252f39f997733ab65072,sync:0000000000000000000000000000000000000000\n",
        )?;
        write_fixture_csv(
            &fixture_dir,
            "tokens.csv",
            "id,address,symbol,decimals\n0,0x0000000000000000000000000000000000000000,ETH,18\n1,0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48,USDC,6\n",
        )?;
        write_fixture_csv(
            &fixture_dir,
            "balances.csv",
            "account_id,coin_id,balance\n1,0,100000000000000000\n1,1,1000000\n",
        )?;

        let artifact_dir = unique_temp_dir("snapshot-import-artifacts-output");
        compile_snapshot(&CompileSnapshotRequest {
            data_dir: fixture_dir,
            out_dir: artifact_dir.clone(),
            snapshot_label: "snapshot-artifact-test".to_string(),
        })?;

        let first = import_snapshot_artifacts(&ImportSnapshotArtifactsRequest {
            database_url: database_url.clone(),
            artifact_dir: artifact_dir.clone(),
            reset_runtime_state: true,
        })
        .await?;

        let pool = PgPool::connect(database_url.as_str())
            .await
            .wrap_err("failed to connect to postgres for importer validation")?;
        assert_eq!(count_rows(&pool, "claim_entries").await?, 2);
        assert_eq!(count_rows(&pool, "claim_proofs").await?, 2);

        let second = import_snapshot_artifacts(&ImportSnapshotArtifactsRequest {
            database_url: database_url.clone(),
            artifact_dir,
            reset_runtime_state: true,
        })
        .await?;

        assert_eq!(first.claim_entry_count, second.claim_entry_count);
        assert_eq!(count_rows(&pool, "claim_entries").await?, 2);
        assert_eq!(count_rows(&pool, "claim_proofs").await?, 2);

        Ok(())
    }

    async fn initialize_test_database(database_url: &Url) -> eyre::Result<()> {
        let pool = PgPool::connect(database_url.as_str())
            .await
            .wrap_err("failed to connect to postgres for test migrations")?;
        MIGRATOR
            .run(&pool)
            .await
            .wrap_err("failed to run snapshot importer test migrations")?;
        Ok(())
    }

    async fn provision_test_database() -> eyre::Result<Url> {
        let base_url = test_database_url();
        let db_name = format!(
            "zksync_lite_sunset_snapshot_test_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        );

        let mut admin_url = base_url.clone();
        admin_url.set_path("/postgres");
        let admin_pool = PgPool::connect(admin_url.as_str())
            .await
            .wrap_err("failed to connect to postgres admin database")?;
        sqlx::query(&format!("CREATE DATABASE {db_name}"))
            .execute(&admin_pool)
            .await
            .wrap_err_with(|| format!("failed to create test database {db_name}"))?;

        let mut database_url = base_url;
        database_url.set_path(&format!("/{db_name}"));
        Ok(database_url)
    }

    fn test_database_url() -> Url {
        let value = std::env::var("ZKSYNC_LITE_SUNSET_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL"))
            .unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
        Url::parse(&value).expect("test database URL should parse")
    }

    async fn count_rows(pool: &PgPool, table: &str) -> eyre::Result<i64> {
        let query = format!("SELECT COUNT(*) AS count FROM {table}");
        sqlx::query_scalar(&query)
            .fetch_one(pool)
            .await
            .wrap_err_with(|| format!("failed to count rows in {table}"))
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "zksync-lite-sunset-{prefix}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_fixture_csv(dir: &Path, name: &str, contents: &str) -> eyre::Result<()> {
        std::fs::create_dir_all(dir)
            .wrap_err_with(|| format!("failed to create fixture dir {}", dir.display()))?;
        std::fs::write(dir.join(name), contents)
            .wrap_err_with(|| format!("failed to write fixture file {name}"))?;
        Ok(())
    }
}
