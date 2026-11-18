use eyre::{Result, WrapErr};
use sqlx::PgPool;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const DEFAULT_TEST_DATABASE_URL: &str =
    "postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test";
const TEST_ADDRESS: &str = "0xBa98deEFBca8E585208B252f39f997733aB65072";

#[test]
fn generate_dev_snapshot_cli_emits_json_and_csvs() -> Result<()> {
    let output_dir = unique_temp_dir("cli-generate-dev-snapshot");
    let output = Command::new(env!("CARGO_BIN_EXE_zksync-lite-sunset-cli"))
        .args([
            "generate-dev-snapshot",
            "--address",
            TEST_ADDRESS,
            "--out-dir",
            output_dir
                .to_str()
                .expect("temp dir path should be valid utf-8"),
            "--mode",
            "default",
            "--json",
        ])
        .output()
        .wrap_err("failed to execute generate-dev-snapshot CLI")?;

    if !output.status.success() {
        return Err(eyre::eyre!(
            "generate-dev-snapshot CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).wrap_err("failed to parse CLI JSON output")?;
    assert_eq!(value["mode"], "default");
    assert_eq!(value["tokenCount"], 4);
    assert!(output_dir.join("accounts.csv").exists());
    assert!(output_dir.join("tokens.csv").exists());
    assert!(output_dir.join("balances.csv").exists());

    Ok(())
}

#[tokio::test]
async fn dev_seed_snapshot_cli_sets_up_database() -> Result<()> {
    let database_url = provision_test_database().await?;

    let output_dir = unique_temp_dir("cli-dev-seed-snapshot");
    let init = Command::new(env!("CARGO_BIN_EXE_zksync-lite-sunset-cli"))
        .args(["db-init", "--database-url", database_url.as_str()])
        .output()
        .wrap_err("failed to execute db-init CLI")?;
    if !init.status.success() {
        return Err(eyre::eyre!(
            "db-init CLI failed: {}",
            String::from_utf8_lossy(&init.stderr)
        ));
    }

    let output = Command::new(env!("CARGO_BIN_EXE_zksync-lite-sunset-cli"))
        .args([
            "dev-seed-snapshot",
            "--database-url",
            database_url.as_str(),
            "--address",
            TEST_ADDRESS,
            "--out-dir",
            output_dir
                .to_str()
                .expect("temp dir path should be valid utf-8"),
            "--mode",
            "default",
            "--snapshot-label",
            "cli-seeded-snapshot",
            "--json",
        ])
        .output()
        .wrap_err("failed to execute dev-seed-snapshot CLI")?;

    if !output.status.success() {
        return Err(eyre::eyre!(
            "dev-seed-snapshot CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).wrap_err("failed to parse dev seed JSON output")?;
    assert_eq!(value["address"], TEST_ADDRESS.to_lowercase());
    assert_eq!(value["mode"], "default");
    assert_eq!(value["imported"]["accounts"], 1);
    assert_eq!(value["imported"]["tokens"], 4);
    assert_eq!(value["materialized"]["entry_count"], 4);

    let pool = PgPool::connect(database_url.as_str())
        .await
        .wrap_err("failed to connect to postgres for CLI validation")?;
    assert_eq!(count_rows(&pool, "snapshot_accounts").await?, 1);
    assert_eq!(count_rows(&pool, "snapshot_tokens").await?, 4);
    assert_eq!(count_rows(&pool, "snapshot_balances").await?, 4);
    assert_eq!(count_rows(&pool, "claim_entries").await?, 4);
    assert_eq!(count_rows(&pool, "claim_proofs").await?, 4);

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
        .wrap_err("failed to connect to postgres admin database for CLI test")?;
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin_pool)
        .await
        .wrap_err_with(|| format!("failed to create CLI test database {db_name}"))?;

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
