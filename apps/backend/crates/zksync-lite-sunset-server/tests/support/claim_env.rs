use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use alloy::primitives::{Address, B256, U256, hex, keccak256};
use eyre::{Context, Result, ensure};
use k256::ecdsa::SigningKey;
use sqlx::PgPool;
use zksync_lite_sunset_core::{
    claims::prepare::ClaimPrepareResponse, signed_batches::compute_claim_batch_signing_hash,
};

pub(super) const TEST_RPC_PORT: u16 = 18545;
pub(super) const SPONSOR_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
pub(super) const TEST_SIGNER_PRIVATE_KEY: &str =
    "0x0707070707070707070707070707070707070707070707070707070707070707";
const DEFAULT_TEST_DATABASE_URL: &str =
    "postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test";
const CLAIM_PROXY_ADMIN: &str = "0x000000000000000000000000000000000000beef";

#[derive(Debug, Clone)]
pub(super) struct TestChainConfig {
    pub(super) rpc_url: String,
    pub(super) migration_contract: Address,
    pub(super) claim_contract: Address,
    pub(super) multicall_contract: Address,
    pub(super) claim_root: B256,
    pub(super) claimant: Address,
    pub(super) token_a: Address,
    pub(super) token_b: Address,
}

#[derive(Debug, Clone)]
struct ClaimFixture {
    claimant: Address,
    root: B256,
    items: [ClaimFixtureItem; 2],
}

#[derive(Debug, Clone, Copy)]
struct ClaimFixtureItem {
    index: u64,
    token: Address,
    amount: U256,
    leaf: B256,
    proof: [B256; 1],
}

#[derive(Debug, Clone, Copy)]
struct MigrationFixtureItem {
    total: U256,
}

pub(super) fn test_signer_address() -> Address {
    let private_key = hex::decode(TEST_SIGNER_PRIVATE_KEY.trim_start_matches("0x"))
        .expect("valid private key hex");
    let signing_key = SigningKey::from_slice(&private_key).expect("valid private key bytes");
    Address::from_public_key(signing_key.verifying_key())
}

pub(super) fn sign_claim_batch(claim_prepare: &ClaimPrepareResponse) -> Result<String> {
    sign_claim_batch_with_private_key(claim_prepare, TEST_SIGNER_PRIVATE_KEY)
}

pub(super) fn sign_claim_batch_with_private_key(
    claim_prepare: &ClaimPrepareResponse,
    private_key: &str,
) -> Result<String> {
    let digest = compute_claim_batch_signing_hash(
        claim_prepare.address,
        claim_prepare.claims_hash,
        claim_prepare.expiry,
        claim_prepare.nonce,
        claim_prepare.chain_id,
        claim_prepare.verifying_contract,
    );
    let private_key = B256::from_str(private_key).wrap_err("invalid private key")?;
    let signing_key = SigningKey::from_slice(private_key.as_slice())
        .wrap_err("failed to build signing key from private key")?;
    let (signature, recid) = signing_key
        .sign_prehash_recoverable(digest.as_slice())
        .wrap_err("failed to sign claim digest")?;

    let mut bytes = [0u8; 65];
    bytes[..64].copy_from_slice(signature.to_bytes().as_slice());
    bytes[64] = 27 + recid.to_byte();
    Ok(format!("0x{}", hex::encode(bytes)))
}

pub(super) async fn complete_setup(config: &TestChainConfig) -> Result<()> {
    apply_env(config);
    let database_url = apply_database_url_env();

    reset_database_schema(&database_url).await?;
    // Ensure migrations are applied before seed operations.
    let _ = zksync_lite_sunset_server::app().await?;
    seed_database(config).await?;

    Ok(())
}

pub(super) async fn mark_all_claim_tokens_migrated(user: Address) -> Result<()> {
    let database_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL")
        .unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    let pool = PgPool::connect(&database_url)
        .await
        .wrap_err("failed to connect to postgres for migration state seed")?;

    let tokens: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT token_address FROM claim_entries WHERE claimant_address = $1",
    )
    .bind(format!("{user:#x}"))
    .fetch_all(&pool)
    .await
    .wrap_err("failed to load claim entry tokens for migration state seed")?;

    let rpc_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{TEST_RPC_PORT}"));
    let migration_contract: Address =
        std::env::var("ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS")
            .wrap_err("migration contract address not configured in test environment")?
            .parse()
            .wrap_err("failed to parse configured migration contract address")?;

    for token in tokens {
        send_contract_call(
            &rpc_url,
            migration_contract,
            "setTokenMigrated(address,bool)",
            &[token, "true".to_string()],
        )?;
    }

    Ok(())
}

pub(super) fn init_chain_with_claimant(
    claimant: impl FnOnce(&PathBuf, &str) -> Result<Address>,
) -> Result<TestChainConfig> {
    let rpc_url = format!("http://127.0.0.1:{TEST_RPC_PORT}");

    if !rpc_is_ready(TEST_RPC_PORT) {
        let anvil_log = std::env::temp_dir().join("zksync-lite-sunset-server-tests-anvil.log");
        let log_file = std::fs::File::create(&anvil_log)
            .wrap_err_with(|| format!("failed to create anvil log at {}", anvil_log.display()))?;
        let err_file = log_file
            .try_clone()
            .wrap_err("failed to clone anvil log file handle")?;

        let _child = Command::new("anvil")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(TEST_RPC_PORT.to_string())
            .arg("--chain-id")
            .arg("1")
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(err_file))
            .spawn()
            .wrap_err("failed to spawn anvil for server integration tests")?;

        wait_for_rpc(TEST_RPC_PORT)?;
    }

    let contracts_dir = repo_root().join("contracts/zksync-lite-migration-claims");

    let token_a = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "src/mocks/TestERC20.sol:TestERC20",
        &[],
    )?;
    let token_b = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "src/mocks/TestERC20.sol:TestERC20",
        &[],
    )?;

    let claimant = claimant(&contracts_dir, &rpc_url)?;
    let fixture = claim_fixture(claimant, token_a, token_b);
    let migration_fixture = migration_fixture(token_a);
    let migration_contract = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "src/mocks/MockZkSyncLiteMigrationState.sol:MockZkSyncLiteMigrationState",
        &[],
    )?;

    let claim_implementation = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "src/ZkSyncLiteSunsetClaimDistributorL1.sol:ZkSyncLiteSunsetClaimDistributorL1",
        &[],
    )?;
    let claim_init_data = contract_calldata(
        &contracts_dir,
        "initialize(bytes32,uint256,address)",
        &[
            format!("{:#x}", fixture.root),
            "0".to_string(),
            test_signer_address().to_string(),
        ],
    )?;
    let claim_contract = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "lib/openzeppelin-contracts/contracts/proxy/transparent/TransparentUpgradeableProxy.sol:TransparentUpgradeableProxy",
        &[
            format!("{claim_implementation:#x}"),
            CLAIM_PROXY_ADMIN.to_string(),
            claim_init_data,
        ],
    )?;
    // Anvil does not reliably pre-deploy Multicall3 on this fixture, so we
    // deploy a minimal Multicall3-compatible mock and point the backend at it
    // via ExecutionConfig. Canonical aggregate semantics (atomic revert on any
    // sub-call failure) are preserved.
    let multicall_contract = deploy_contract(
        &contracts_dir,
        &rpc_url,
        "src/mocks/Multicall3.sol:Multicall3",
        &[],
    )?;

    mint_test_token(&rpc_url, token_a, claim_contract, fixture.items[0].amount)?;
    mint_test_token(&rpc_url, token_b, claim_contract, fixture.items[1].amount)?;
    mint_test_token(
        &rpc_url,
        token_a,
        migration_contract,
        migration_fixture.total,
    )?;
    transfer_eth(&rpc_url, test_signer_address(), U256::from(10u64.pow(18)))?;

    send_contract_call(
        &rpc_url,
        migration_contract,
        "setL1ClaimDistributor(address)",
        &[format!("{claim_contract:#x}")],
    )?;

    Ok(TestChainConfig {
        rpc_url,
        migration_contract,
        claim_contract,
        multicall_contract,
        claim_root: fixture.root,
        claimant,
        token_a,
        token_b,
    })
}

fn apply_env(config: &TestChainConfig) {
    // SAFETY: integration tests configure process env before creating app state.
    unsafe {
        std::env::set_var("ZKSYNC_LITE_SUNSET__CORE__AUTH__MIGRATE_CHAIN_ID", "1");
        std::env::set_var("ZKSYNC_LITE_SUNSET__CORE__AUTH__CLAIM_CHAIN_ID", "1");
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__CLAIMS__FRONTEND_MODE",
            "sponsored",
        );
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL",
            &config.rpc_url,
        );
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY",
            SPONSOR_PRIVATE_KEY,
        );
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS",
            format!("{:#x}", config.migration_contract),
        );
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS",
            format!("{:#x}", config.claim_contract),
        );
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MULTICALL3_CONTRACT_ADDRESS",
            format!("{:#x}", config.multicall_contract),
        );
        // Anvil automines one block per tx, so leave finalization at a single
        // confirmation here — production defaults to 4 (config.rs) and is the
        // policy that matters for users.
        std::env::set_var(
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIN_CONFIRMATIONS",
            "1",
        );
        std::env::set_var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__AUTO_MIGRATE", "true");
    }
}

async fn seed_database(config: &TestChainConfig) -> Result<()> {
    let database_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL")
        .unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .wrap_err("failed to connect to postgres for server test seed")?;

    let fixture = claim_fixture(config.claimant, config.token_a, config.token_b);
    ensure!(
        fixture.root == config.claim_root,
        "fixture root mismatch with deployed migration contract"
    );

    sqlx::query(
        r#"
        TRUNCATE TABLE
            migration_requests,
            claim_requests,
            snapshot_tokens,
            claim_proofs,
            claim_entries,
            claim_snapshot_metadata
        RESTART IDENTITY CASCADE
        "#,
    )
    .execute(&pool)
    .await
    .wrap_err("failed to truncate runtime tables before server test seed")?;

    sqlx::query(
        r#"
        INSERT INTO claim_snapshot_metadata (singleton, label, merkle_root, source_checksum, updated_at)
        VALUES (TRUE, $1, $2, $3, NOW())
        "#,
    )
    .bind("server-test-fixture")
    .bind(format!("{:#x}", fixture.root))
    .bind(format!("{:#x}", keccak256(b"server-test-fixture")))
    .execute(&pool)
    .await
    .wrap_err("failed to seed claim snapshot metadata for server test fixture")?;

    sqlx::query(
        r#"
        INSERT INTO snapshot_tokens (id, address, symbol, decimals)
        VALUES
            (0, $1, 'TKA', 18),
            (1, $2, 'TKB', 18)
        ON CONFLICT (id) DO UPDATE SET
            address = EXCLUDED.address,
            symbol = EXCLUDED.symbol,
            decimals = EXCLUDED.decimals
        "#,
    )
    .bind(format!("{:#x}", config.token_a))
    .bind(format!("{:#x}", config.token_b))
    .execute(&pool)
    .await
    .wrap_err("failed to seed token metadata for server test fixture")?;

    for item in fixture.items {
        sqlx::query(
            r#"
            INSERT INTO claim_entries (
                claim_index,
                claimant_address,
                token_address,
                amount,
                leaf_hash,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            "#,
        )
        .bind(item.index as i64)
        .bind(format!("{:#x}", fixture.claimant))
        .bind(format!("{:#x}", item.token))
        .bind(item.amount.to_string())
        .bind(format!("{:#x}", item.leaf))
        .execute(&pool)
        .await
        .wrap_err("failed to insert claim entry fixture")?;

        sqlx::query(
            r#"
            INSERT INTO claim_proofs (claim_index, proof_hashes, created_at, updated_at)
            VALUES ($1, $2, NOW(), NOW())
            "#,
        )
        .bind(item.index as i64)
        .bind(vec![format!("{:#x}", item.proof[0])])
        .execute(&pool)
        .await
        .wrap_err("failed to insert claim proof fixture")?;
    }

    Ok(())
}

fn apply_database_url_env() -> String {
    let database_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL")
        .unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    // SAFETY: integration tests configure process env before app state initialization.
    unsafe {
        std::env::set_var("ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL", &database_url);
    }
    database_url
}

async fn reset_database_schema(database_url: &str) -> Result<()> {
    let pool = PgPool::connect(database_url)
        .await
        .wrap_err("failed to connect to postgres for schema reset")?;

    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(&pool)
        .await
        .wrap_err("failed to drop test database schema")?;
    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .wrap_err("failed to recreate test database schema")?;

    Ok(())
}

pub(super) fn deploy_contract(
    contracts_dir: &PathBuf,
    rpc_url: &str,
    fqcn: &str,
    constructor_args: &[String],
) -> Result<Address> {
    let mut cmd = Command::new("forge");
    cmd.arg("create")
        .arg("--broadcast")
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--private-key")
        .arg(SPONSOR_PRIVATE_KEY)
        .arg(fqcn)
        .current_dir(contracts_dir);

    if !constructor_args.is_empty() {
        cmd.arg("--constructor-args");
        for arg in constructor_args {
            cmd.arg(arg);
        }
    }

    let output = cmd
        .output()
        .wrap_err_with(|| format!("failed to deploy contract {fqcn} with forge"))?;

    ensure!(
        output.status.success(),
        "forge create failed for {fqcn}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for line in combined.lines() {
        if let Some((_, addr)) = line.split_once("Deployed to:") {
            let parsed: Address = addr
                .trim()
                .parse()
                .wrap_err_with(|| format!("failed to parse deployed address from line: {line}"))?;
            return Ok(parsed);
        }
    }

    Err(eyre::eyre!(
        "forge create output missing deployed address for {fqcn}: {combined}"
    ))
}

fn contract_calldata(contracts_dir: &PathBuf, signature: &str, args: &[String]) -> Result<String> {
    let output = Command::new("cast")
        .arg("calldata")
        .arg(signature)
        .args(args)
        .current_dir(contracts_dir)
        .output()
        .wrap_err_with(|| format!("failed to compute calldata for {signature}"))?;

    ensure!(
        output.status.success(),
        "cast calldata failed for {signature}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn mint_test_token(rpc_url: &str, token: Address, recipient: Address, amount: U256) -> Result<()> {
    let output = Command::new("cast")
        .arg("send")
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--private-key")
        .arg(SPONSOR_PRIVATE_KEY)
        .arg(format!("{token:#x}"))
        .arg("mint(address,uint256)")
        .arg(format!("{recipient:#x}"))
        .arg(amount.to_string())
        .output()
        .wrap_err_with(|| format!("failed to mint test token {token:#x}"))?;

    ensure!(
        output.status.success(),
        "cast send mint failed for {token:#x}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

fn send_contract_call(rpc_url: &str, to: Address, signature: &str, args: &[String]) -> Result<()> {
    send_contract_call_with_private_key(rpc_url, SPONSOR_PRIVATE_KEY, to, signature, args)
}

pub(super) fn send_contract_call_with_private_key(
    rpc_url: &str,
    private_key: &str,
    to: Address,
    signature: &str,
    args: &[String],
) -> Result<()> {
    let output = Command::new("cast")
        .arg("send")
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--private-key")
        .arg(private_key)
        .arg(format!("{to:#x}"))
        .arg(signature)
        .args(args)
        .output()
        .wrap_err_with(|| format!("failed to call {signature} on {to:#x}"))?;

    ensure!(
        output.status.success(),
        "cast send failed for {signature} on {to:#x}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

fn transfer_eth(rpc_url: &str, recipient: Address, value: U256) -> Result<()> {
    let output = Command::new("cast")
        .arg("send")
        .arg("--async")
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--private-key")
        .arg(SPONSOR_PRIVATE_KEY)
        .arg(format!("{recipient:#x}"))
        .arg("--value")
        .arg(value.to_string())
        .output()
        .wrap_err_with(|| format!("failed to fund signer {recipient:#x}"))?;

    ensure!(
        output.status.success(),
        "cast send funding failed for {recipient:#x}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../")
        .canonicalize()
        .expect("repo root path should resolve")
}

fn rpc_is_ready(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn wait_for_rpc(port: u16) -> Result<()> {
    for _ in 0..80 {
        if rpc_is_ready(port) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    Err(eyre::eyre!("anvil RPC did not become ready on port {port}"))
}

fn claim_fixture(claimant: Address, token_a: Address, token_b: Address) -> ClaimFixture {
    let amount_a = U256::from(1_000_000u64);
    let amount_b =
        U256::from_str_radix("4000000000000000000", 10).expect("fixture token amount should parse");

    let leaf_a = claim_leaf(0, claimant, token_a, amount_a);
    let leaf_b = claim_leaf(1, claimant, token_b, amount_b);

    let root = hash_pair(leaf_a, leaf_b);

    ClaimFixture {
        claimant,
        root,
        items: [
            ClaimFixtureItem {
                index: 0,
                token: token_a,
                amount: amount_a,
                leaf: leaf_a,
                proof: [leaf_b],
            },
            ClaimFixtureItem {
                index: 1,
                token: token_b,
                amount: amount_b,
                leaf: leaf_b,
                proof: [leaf_a],
            },
        ],
    }
}

fn migration_fixture(_token: Address) -> MigrationFixtureItem {
    let total =
        U256::from_str_radix("10000000000000000000", 10).expect("migration amount should parse");
    MigrationFixtureItem { total }
}

fn claim_leaf(index: u64, claimant: Address, token: Address, amount: U256) -> B256 {
    let mut packed = Vec::with_capacity(32 + 20 + 20 + 32);

    let mut index_bytes = [0u8; 32];
    index_bytes[24..].copy_from_slice(&index.to_be_bytes());
    packed.extend_from_slice(&index_bytes);

    packed.extend_from_slice(claimant.as_slice());
    packed.extend_from_slice(token.as_slice());
    packed.extend_from_slice(&amount.to_be_bytes::<32>());

    keccak256(packed)
}

fn hash_pair(a: B256, b: B256) -> B256 {
    let (left, right) = if a < b { (a, b) } else { (b, a) };
    let mut joined = Vec::with_capacity(64);
    joined.extend_from_slice(left.as_slice());
    joined.extend_from_slice(right.as_slice());
    keccak256(joined)
}
