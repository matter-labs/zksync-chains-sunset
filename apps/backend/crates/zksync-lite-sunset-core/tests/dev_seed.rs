use alloy::primitives::{Address, address};
use eyre::{Result, WrapErr};
use sqlx::PgPool;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use zksync_lite_sunset_core::dev_seed::{
    DevFixtureMode, DevSeedRequest, generate_dev_snapshot, seed_dev_snapshot,
};

const DEFAULT_TEST_DATABASE_URL: &str =
    "postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test";
const TEST_ADDRESS: Address = address!("0xBa98deEFBca8E585208B252f39f997733aB65072");

#[tokio::test]
async fn generate_dev_snapshot_writes_expected_csvs() -> Result<()> {
    let output_dir = unique_temp_dir("generate-dev-snapshot");
    let result = generate_dev_snapshot(
        &zksync_lite_sunset_core::dev_seed::GenerateDevSnapshotRequest {
            address: TEST_ADDRESS,
            output_dir: output_dir.clone(),
            mode: DevFixtureMode::Default,
        },
    )
    .await?;

    assert_eq!(result.token_count, 4);
    assert!(output_dir.join("accounts.csv").exists());
    assert!(output_dir.join("tokens.csv").exists());
    assert!(output_dir.join("balances.csv").exists());

    let accounts = std::fs::read_to_string(output_dir.join("accounts.csv"))
        .wrap_err("failed to read generated accounts.csv")?;
    assert!(accounts.contains("0xba98deefbca8e585208b252f39f997733ab65072"));

    let tokens = std::fs::read_to_string(output_dir.join("tokens.csv"))
        .wrap_err("failed to read generated tokens.csv")?;
    assert!(tokens.contains("id,address,symbol,decimals"));
    assert!(tokens.contains("0xdac17f958d2ee523a2206206994597c13d831ec7,USDT,6"));
    assert!(tokens.contains("0x0000000000000000000000000000000000000000,ETH,18"));

    Ok(())
}

#[tokio::test]
async fn seed_dev_snapshot_populates_expected_tables() -> Result<()> {
    let database_url = provision_test_database().await?;
    zksync_lite_sunset_core::snapshots::initialize_snapshot_storage(&database_url).await?;

    let output_dir = unique_temp_dir("seed-dev-snapshot");
    let _ = seed_dev_snapshot(&DevSeedRequest {
        database_url: database_url.clone(),
        address: TEST_ADDRESS,
        mode: DevFixtureMode::Default,
        snapshot_label: "test-dev-seed".to_string(),
        output_dir: Some(output_dir),
    })
    .await?;

    let pool = PgPool::connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres for validation")?;

    assert_eq!(count_rows(&pool, "snapshot_accounts").await?, 1);
    assert_eq!(count_rows(&pool, "snapshot_tokens").await?, 4);
    assert_eq!(count_rows(&pool, "snapshot_balances").await?, 4);
    assert_eq!(count_rows(&pool, "claim_entries").await?, 4);
    assert_eq!(count_rows(&pool, "claim_proofs").await?, 4);

    Ok(())
}

#[tokio::test]
async fn reseeding_replaces_snapshot_tables() -> Result<()> {
    let database_url = provision_test_database().await?;
    zksync_lite_sunset_core::snapshots::initialize_snapshot_storage(&database_url).await?;

    seed_dev_snapshot(&DevSeedRequest {
        database_url: database_url.clone(),
        address: TEST_ADDRESS,
        mode: DevFixtureMode::Default,
        snapshot_label: "test-dev-seed-default".to_string(),
        output_dir: Some(unique_temp_dir("seed-default")),
    })
    .await?;

    let second = seed_dev_snapshot(&DevSeedRequest {
        database_url: database_url.clone(),
        address: TEST_ADDRESS,
        mode: DevFixtureMode::MigrateOnly,
        snapshot_label: "test-dev-seed-migrate-only".to_string(),
        output_dir: Some(unique_temp_dir("seed-migrate-only")),
    })
    .await?;

    let pool = PgPool::connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres for reseed validation")?;

    assert_eq!(count_rows(&pool, "claim_entries").await?, 1);
    assert_eq!(count_rows(&pool, "claim_proofs").await?, 1);
    assert_eq!(count_rows(&pool, "snapshot_tokens").await?, 1);
    assert_eq!(count_rows(&pool, "snapshot_balances").await?, 1);
    assert_eq!(count_rows(&pool, "snapshot_accounts").await?, 1);

    assert_eq!(second.claims.len(), 1);

    Ok(())
}

#[tokio::test]
async fn claims_import_can_reuse_existing_materialized_snapshots() -> Result<()> {
    let database_url = provision_test_database().await?;
    zksync_lite_sunset_core::snapshots::initialize_snapshot_storage(&database_url).await?;

    let fixture_dir = unique_temp_dir("claims-import-reuse-existing");
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

    let first = zksync_lite_sunset_core::snapshots::import_and_materialize_claims(
        &zksync_lite_sunset_core::snapshots::ClaimsImportRequest {
            database_url: database_url.clone(),
            data_dir: fixture_dir.clone(),
            chunk_size: std::num::NonZeroUsize::new(100).expect("100 is non-zero"),
            truncate: true,
            snapshot_label: "claims-import-first".to_string(),
            reset_runtime_state: false,
        },
    )
    .await?;

    let second = zksync_lite_sunset_core::snapshots::import_and_materialize_claims(
        &zksync_lite_sunset_core::snapshots::ClaimsImportRequest {
            database_url: database_url.clone(),
            data_dir: fixture_dir,
            chunk_size: std::num::NonZeroUsize::new(100).expect("100 is non-zero"),
            truncate: true,
            snapshot_label: "claims-import-second".to_string(),
            reset_runtime_state: false,
        },
    )
    .await?;

    let pool = PgPool::connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres for reuse validation")?;

    assert_eq!(
        first.materialized.merkle_root,
        second.materialized.merkle_root
    );
    assert_eq!(count_rows(&pool, "claim_entries").await?, 2);

    Ok(())
}

#[tokio::test]
async fn claims_import_materializes_large_snapshot_without_exceeding_bind_limit() -> Result<()> {
    let database_url = provision_test_database().await?;
    zksync_lite_sunset_core::snapshots::initialize_snapshot_storage(&database_url).await?;

    let fixture_dir = unique_temp_dir("claims-import-large-snapshot");
    const ENTRY_COUNT: usize = 10_000;

    let mut accounts = String::from("id,nonce,address,pubkey_hash\n");
    let mut balances = String::from("account_id,coin_id,balance\n");

    for account_id in 1..=ENTRY_COUNT {
        accounts.push_str(&format!(
            "{account_id},0,0x{account_id:040x},sync:0000000000000000000000000000000000000000\n",
        ));
        balances.push_str(&format!("{account_id},0,100000000000000000\n"));
    }

    write_fixture_csv(&fixture_dir, "accounts.csv", &accounts)?;
    write_fixture_csv(
        &fixture_dir,
        "tokens.csv",
        "id,address,symbol,decimals\n0,0x0000000000000000000000000000000000000000,ETH,18\n",
    )?;
    write_fixture_csv(&fixture_dir, "balances.csv", &balances)?;

    let result = zksync_lite_sunset_core::snapshots::import_and_materialize_claims(
        &zksync_lite_sunset_core::snapshots::ClaimsImportRequest {
            database_url: database_url.clone(),
            data_dir: fixture_dir,
            chunk_size: std::num::NonZeroUsize::new(1_000).expect("1_000 is non-zero"),
            truncate: true,
            snapshot_label: "claims-import-large".to_string(),
            reset_runtime_state: false,
        },
    )
    .await?;

    assert_eq!(result.materialized.entry_count, ENTRY_COUNT);

    let pool = PgPool::connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres for large snapshot validation")?;

    assert_eq!(
        count_rows(&pool, "claim_entries").await?,
        ENTRY_COUNT as i64
    );
    assert_eq!(count_rows(&pool, "claim_proofs").await?, ENTRY_COUNT as i64);

    Ok(())
}

fn test_database_url() -> Url {
    let value = std::env::var("ZKSYNC_LITE_SUNSET_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL"))
        .unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    Url::parse(&value).expect("test database URL should parse")
}

async fn provision_test_database() -> Result<Url> {
    let base_url = test_database_url();
    let db_name = format!(
        "zksync_lite_sunset_test_{}_{}",
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

async fn count_rows(pool: &PgPool, table: &str) -> Result<i64> {
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

fn write_fixture_csv(dir: &PathBuf, name: &str, contents: &str) -> Result<()> {
    std::fs::create_dir_all(dir)
        .wrap_err_with(|| format!("failed to create fixture dir {}", dir.display()))?;
    std::fs::write(dir.join(name), contents)
        .wrap_err_with(|| format!("failed to write fixture file {name}"))?;
    Ok(())
}
