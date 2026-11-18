use alloy::primitives::{Address, B256, U256, hex};
use clap::Args;
use eyre::{Context, OptionExt, bail, ensure};
use k256::ecdsa::SigningKey;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{Instant, sleep};
use tracing::info;
use url::Url;
use zksync_lite_sunset_core::claims::{
    prepare::{ClaimPrepareRequest, ClaimPrepareResponse},
    shared::ClaimStatus,
    status::ClaimResponse,
    submit::ClaimSubmitRequest,
};
use zksync_lite_sunset_core::eligibility::{EligibilityResponse, EligibleToken};
use zksync_lite_sunset_core::migrations::{
    prepare::{MigrationPrepareRequest, MigrationPrepareResponse},
    shared::MigrationStatus,
    status::{MigrationDeliveryPath, MigrationResponse},
    submit::MigrationSubmitRequest,
};
use zksync_lite_sunset_core::signed_batches::compute_claim_batch_signing_hash;

const DEFAULT_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const DEFAULT_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const DEFAULT_ERC20_SYMBOL: &str = "DAI";
const READY_LOG_TAIL_LINES: usize = 80;

#[derive(Debug, Clone, Args)]
pub struct ComposeSeededSmokeArgs {
    #[arg(long, default_value = DEFAULT_ADDRESS)]
    pub address: Address,
    #[arg(long, default_value = DEFAULT_PRIVATE_KEY)]
    pub private_key: String,
    #[arg(long, default_value = DEFAULT_ERC20_SYMBOL)]
    pub token_symbol: String,
    #[arg(long, default_value = "http://127.0.0.1:3000")]
    pub backend_base_url: Url,
    #[arg(long, default_value = "http://127.0.0.1:5010")]
    pub l1_rpc_url: Url,
    #[arg(long)]
    pub compose_file: Option<PathBuf>,
    #[arg(long)]
    pub infra_env_file: Option<PathBuf>,
    #[arg(long)]
    pub log_file: Option<PathBuf>,
    #[arg(long, default_value_t = 240)]
    pub timeout_seconds: u64,
    #[arg(long, default_value_t = false)]
    pub keep_stack_running: bool,
    #[arg(long, default_value_t = false)]
    pub teardown_compose: bool,
}

#[derive(Debug)]
struct ComposeSeededSmokeOptions {
    address: Address,
    private_key: String,
    token_symbol: String,
    backend_base_url: Url,
    l1_rpc_url: Url,
    compose_file: PathBuf,
    infra_env_file: PathBuf,
    log_file: PathBuf,
    timeout: Duration,
    keep_stack_running: bool,
    teardown_compose: bool,
}

#[derive(Debug)]
struct RuntimeContext {
    client: Client,
    claim_frontend_url: Url,
    migrate_frontend_url: Url,
    stack_proc: Option<Child>,
    compose_was_running: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ReadyResponse {
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiErrorResponse {
    code: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SmokeSummary {
    address: Address,
    claim_frontend_url: Url,
    migrate_frontend_url: Url,
    scenarios: Vec<SmokeScenarioSummary>,
    stack_log: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SmokeScenarioKind {
    Erc20,
    Eth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SmokeScenarioSummary {
    kind: SmokeScenarioKind,
    token_symbol: String,
    token_address: Address,
    migration: MigrationResponse,
    claim: ClaimResponse,
}

#[derive(Debug)]
struct HttpJsonError {
    url: String,
    status: StatusCode,
    body: String,
}

impl std::fmt::Display for HttpJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} returned HTTP {}: {}",
            self.url, self.status, self.body
        )
    }
}

impl std::error::Error for HttpJsonError {}

pub async fn run(args: ComposeSeededSmokeArgs) -> eyre::Result<()> {
    let options = ComposeSeededSmokeOptions::new(args)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .wrap_err("failed to build HTTP client for compose seeded smoke")?;
    let claim_port = free_tcp_port().wrap_err("failed to allocate claim frontend port")?;
    let migrate_port = free_tcp_port().wrap_err("failed to allocate migrate frontend port")?;
    let claim_frontend_url = Url::parse(&format!("http://127.0.0.1:{claim_port}"))
        .wrap_err("failed to build claim frontend URL")?;
    let migrate_frontend_url = Url::parse(&format!("http://127.0.0.1:{migrate_port}"))
        .wrap_err("failed to build migrate frontend URL")?;
    let compose_was_running = compose_core_running(&options.compose_file, &options.infra_env_file)
        .wrap_err("failed to inspect compose stack state")?;

    let mut runtime = RuntimeContext {
        client,
        claim_frontend_url,
        migrate_frontend_url,
        stack_proc: None,
        compose_was_running,
    };

    let result = run_inner(&options, &mut runtime).await;
    let cleanup_result = cleanup(&options, &mut runtime);

    match result {
        Ok(summary) => {
            cleanup_result?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        Err(err) => {
            let err = err.wrap_err(tail_log(&options.log_file, READY_LOG_TAIL_LINES));
            if let Err(cleanup_err) = cleanup_result {
                Err(err.wrap_err(cleanup_err))
            } else {
                Err(err)
            }
        }
    }
}

async fn run_inner(
    options: &ComposeSeededSmokeOptions,
    runtime: &mut RuntimeContext,
) -> eyre::Result<SmokeSummary> {
    info!(
        component = "cli",
        command = "compose-seeded-smoke",
        address = %format!("{:#x}", options.address),
        "starting command"
    );
    runtime.stack_proc = Some(
        spawn_stack_process(
            options,
            &runtime.claim_frontend_url,
            &runtime.migrate_frontend_url,
        )
        .wrap_err("failed to start compose seeded stack")?,
    );
    eprintln!(
        "[compose-seeded-smoke] stack log: {}",
        options.log_file.display()
    );
    eprintln!(
        "[compose-seeded-smoke] waiting for backend={} claim={} migrate={}",
        options.backend_base_url, runtime.claim_frontend_url, runtime.migrate_frontend_url,
    );

    wait_for_stack(options, runtime)
        .await
        .wrap_err("compose seeded stack did not become ready")?;
    eprintln!("[compose-seeded-smoke] seeded stack ready");
    verify_healthz_healthy(options, runtime)
        .await
        .wrap_err("supported stack reported unhealthy public health state")?;
    verify_unknown_address_eligibility(options, runtime)
        .await
        .wrap_err("unknown-address eligibility regression")?;

    let initial_tokens = load_user_eligibility(options, runtime)
        .await
        .wrap_err("failed to load user eligibility for smoke seed")?;
    let erc20_token =
        pick_erc20_smoke_token(&initial_tokens.tokens, &options.token_symbol)?.clone();
    let eth_token = pick_eth_smoke_token(&initial_tokens.tokens)?.clone();

    let erc20_summary = run_token_scenario(
        options,
        runtime,
        SmokeScenarioKind::Erc20,
        erc20_token,
        true,
    )
    .await
    .wrap_err("erc20 smoke scenario failed")?;
    let eth_summary =
        run_token_scenario(options, runtime, SmokeScenarioKind::Eth, eth_token, false)
            .await
            .wrap_err("eth smoke scenario failed")?;

    Ok(SmokeSummary {
        address: options.address,
        claim_frontend_url: runtime.claim_frontend_url.clone(),
        migrate_frontend_url: runtime.migrate_frontend_url.clone(),
        scenarios: vec![erc20_summary, eth_summary],
        stack_log: options.log_file.display().to_string(),
    })
}

impl ComposeSeededSmokeOptions {
    fn new(args: ComposeSeededSmokeArgs) -> eyre::Result<Self> {
        let repo_root = repo_root()?;
        Ok(Self {
            address: args.address,
            private_key: args.private_key,
            token_symbol: args.token_symbol,
            backend_base_url: args.backend_base_url,
            l1_rpc_url: args.l1_rpc_url,
            compose_file: args
                .compose_file
                .unwrap_or_else(|| repo_root.join("infra/compose.yml")),
            infra_env_file: args
                .infra_env_file
                .unwrap_or_else(|| repo_root.join("infra/.env.local")),
            log_file: args.log_file.unwrap_or_else(default_log_file),
            timeout: Duration::from_secs(args.timeout_seconds),
            keep_stack_running: args.keep_stack_running,
            teardown_compose: args.teardown_compose,
        })
    }
}

fn repo_root() -> eyre::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .ok_or_eyre("failed to resolve repo root from zksync-lite-sunset-cli manifest dir")
}

fn default_log_file() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "zksync-lite-sunset-compose-seeded-smoke-{}-{now}.log",
        std::process::id()
    ))
}

fn free_tcp_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn compose_base_args(compose_file: &Path, infra_env_file: &Path) -> Vec<String> {
    let mut args = vec!["compose".to_string()];
    if infra_env_file.exists() {
        args.extend([
            "--env-file".to_string(),
            infra_env_file.display().to_string(),
        ]);
    }
    args.extend(["-f".to_string(), compose_file.display().to_string()]);
    args
}

fn compose_core_running(compose_file: &Path, infra_env_file: &Path) -> eyre::Result<bool> {
    let mut args = compose_base_args(compose_file, infra_env_file);
    args.extend([
        "ps".to_string(),
        "--status".to_string(),
        "running".to_string(),
        "--services".to_string(),
    ]);
    let output = Command::new("docker")
        .args(&args)
        .output()
        .wrap_err("failed to execute docker compose ps")?;
    if !output.status.success() {
        return Ok(false);
    }

    let running = String::from_utf8_lossy(&output.stdout);
    let services = ["postgres", "anvil", "zksync-lite-sunset-server"];
    Ok(running
        .split_whitespace()
        .any(|service| services.contains(&service)))
}

fn spawn_stack_process(
    options: &ComposeSeededSmokeOptions,
    claim_frontend_url: &Url,
    migrate_frontend_url: &Url,
) -> eyre::Result<Child> {
    let repo_root = repo_root()?;
    let stack_script = repo_root.join("scripts/dev/run-compose-seeded-stack.sh");
    let log_file = options
        .log_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&log_file).wrap_err("failed to create smoke log directory")?;
    let stdout = File::create(&options.log_file).wrap_err_with(|| {
        format!(
            "failed to create smoke log file {}",
            options.log_file.display()
        )
    })?;
    let stderr = stdout
        .try_clone()
        .wrap_err("failed to clone smoke log handle for stderr")?;

    Command::new("bash")
        .arg(&stack_script)
        .current_dir(&repo_root)
        .env("CLAIMANT", format!("{:#x}", options.address))
        .env("CLAIMANT_PRIVATE_KEY", &options.private_key)
        .env("CLAIM_FRONTEND_HOST", "127.0.0.1")
        .env(
            "CLAIM_FRONTEND_PORT",
            claim_frontend_url.port().unwrap_or_default().to_string(),
        )
        .env("MIGRATE_FRONTEND_HOST", "127.0.0.1")
        .env(
            "MIGRATE_FRONTEND_PORT",
            migrate_frontend_url.port().unwrap_or_default().to_string(),
        )
        .env("COMPOSE_FILE", &options.compose_file)
        .env("INFRA_ENV_FILE", &options.infra_env_file)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .wrap_err_with(|| format!("failed to spawn {}", stack_script.display()))
}

async fn wait_for_stack(
    options: &ComposeSeededSmokeOptions,
    runtime: &mut RuntimeContext,
) -> eyre::Result<()> {
    let deadline = Instant::now() + options.timeout;
    let ready_url = backend_url(&options.backend_base_url, "/healthz")?;
    let mut last_status_report = Instant::now() - Duration::from_secs(10);

    while Instant::now() < deadline {
        if let Some(stack_proc) = runtime.stack_proc.as_mut()
            && let Some(status) = stack_proc
                .try_wait()
                .wrap_err("failed to poll compose seeded stack process")?
        {
            bail!(
                "compose seeded stack exited early with code {:?}\n\n{}",
                status.code(),
                tail_log(&options.log_file, READY_LOG_TAIL_LINES)
            );
        }

        let backend_ready = matches!(
            get_json::<ReadyResponse>(&runtime.client, ready_url.clone()).await,
            Ok(response) if response.status == "ok"
        );
        let claim_ready =
            request_head_ok(&runtime.client, runtime.claim_frontend_url.clone()).await;
        let migrate_ready =
            request_head_ok(&runtime.client, runtime.migrate_frontend_url.clone()).await;

        if backend_ready && claim_ready && migrate_ready {
            return Ok(());
        }

        if last_status_report.elapsed() >= Duration::from_secs(10) {
            let mut pending = Vec::new();
            if !backend_ready {
                pending.push(format!("backend {}", options.backend_base_url));
            }
            if !claim_ready {
                pending.push(format!("claim {}", runtime.claim_frontend_url));
            }
            if !migrate_ready {
                pending.push(format!("migrate {}", runtime.migrate_frontend_url));
            }
            eprintln!(
                "[compose-seeded-smoke] still waiting for {} (log: {})",
                pending.join(", "),
                options.log_file.display()
            );
            last_status_report = Instant::now();
        }

        sleep(Duration::from_secs(1)).await;
    }

    bail!(
        "timed out waiting for the seeded compose stack to become ready\n\n{}",
        tail_log(&options.log_file, READY_LOG_TAIL_LINES)
    )
}

async fn verify_healthz_healthy(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
) -> eyre::Result<()> {
    let body = get_json::<serde_json::Value>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/healthz")?,
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to load /healthz")?;
    ensure!(
        body.get("status").and_then(|value| value.as_str()) == Some("ok"),
        "expected healthz status=ok, got: {}",
        serde_json::to_string_pretty(&body)?
    );
    Ok(())
}

async fn verify_unknown_address_eligibility(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
) -> eyre::Result<()> {
    let response = post_json::<_, serde_json::Value>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/eligibility")?,
        &json!({
            "address": format!("{:#x}", Address::ZERO),
        }),
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to query unknown-address eligibility")?;
    let tokens = response
        .get("tokens")
        .and_then(|value| value.as_array())
        .ok_or_eyre("eligibility response omitted tokens array")?;
    ensure!(
        tokens.is_empty(),
        "expected unknown address eligibility to be empty, got: {}",
        serde_json::to_string_pretty(&response)?
    );
    Ok(())
}

async fn load_user_eligibility(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
) -> eyre::Result<EligibilityResponse> {
    post_json::<_, EligibilityResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/eligibility")?,
        &json!({ "address": format!("{:#x}", options.address) }),
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to load per-user eligibility")
}

async fn run_token_scenario(
    options: &ComposeSeededSmokeOptions,
    runtime: &mut RuntimeContext,
    kind: SmokeScenarioKind,
    smoke_token: EligibleToken,
    restart_backend_after_submit: bool,
) -> eyre::Result<SmokeScenarioSummary> {
    ensure!(
        smoke_token.raw_balance > U256::ZERO,
        "expected positive migration token balance for {}",
        smoke_token.symbol
    );

    let migration_prepare = post_json::<_, MigrationPrepareResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/migrations/prepare")?,
        &MigrationPrepareRequest {
            address: options.address,
            token_address: smoke_token.token_address,
        },
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to prepare migration")?;
    ensure!(
        migration_prepare.item.token_address == smoke_token.token_address,
        "migration prepare token mismatch"
    );

    let migration_tx_hash = rpc_call(
        &runtime.client,
        &options.l1_rpc_url,
        "eth_sendTransaction",
        json!([{
            "from": options.address,
            "to": migration_prepare.contract_address,
            "data": migration_prepare.call_data,
        }]),
    )
    .await
    .wrap_err("failed to submit migration transaction to L1")?;

    let _migration_submit = post_json::<_, MigrationResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/migrations")?,
        &MigrationSubmitRequest {
            migration_id: migration_prepare.migration_id.clone(),
            tx_hash: migration_tx_hash,
        },
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to submit migration record to backend")?;

    let migration_status = poll_migration_status(options, runtime, &migration_prepare).await?;
    ensure!(
        migration_status.status == MigrationStatus::MinedSuccess,
        "migration did not reach mined_success: {}",
        serde_json::to_string_pretty(&migration_status)?
    );
    verify_migration_delivery_path(smoke_token.token_address, &migration_status)?;
    verify_migration_prepare_closed(options, runtime, smoke_token.token_address)
        .await
        .wrap_err("already-migrated rejection regression")?;
    verify_claim_prepare_all_eligible(options, runtime, smoke_token.token_address)
        .await
        .wrap_err("all-eligible claim prepare regression")?;

    let claim_prepare = post_json::<_, ClaimPrepareResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/claims/prepare")?,
        &ClaimPrepareRequest {
            address: options.address,
            token_address: Some(smoke_token.token_address),
            token_addresses: Vec::new(),
            all_eligible: false,
        },
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to prepare claim")?;
    verify_invalid_claim_signature_rejected(options, runtime, &claim_prepare)
        .await
        .wrap_err("invalid-signature rejection regression")?;
    let signature = sign_claim_batch(&options.private_key, &claim_prepare)
        .wrap_err("failed to sign claim batch")?;
    let claim_submit = post_json::<_, ClaimResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/claims")?,
        &ClaimSubmitRequest {
            address: options.address,
            items: claim_prepare.items.clone(),
            claims_hash: claim_prepare.claims_hash,
            nonce: claim_prepare.nonce,
            expiry: claim_prepare.expiry,
            signature,
        },
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to submit claim to backend")?;

    if restart_backend_after_submit {
        restart_backend_service(options)
            .wrap_err("failed to restart backend container during smoke persistence check")?;
        wait_for_stack(options, runtime)
            .await
            .wrap_err("stack did not recover after backend restart")?;
    }

    let claim_status = poll_claim_status(options, runtime, &claim_submit).await?;
    ensure!(
        claim_status.status == ClaimStatus::MinedSuccess,
        "claim did not reach mined_success: {}",
        serde_json::to_string_pretty(&claim_status)?
    );

    verify_migration_tokens_metadata(options, runtime, smoke_token.token_address)
        .await
        .wrap_err("migration tokens metadata regression")?;
    verify_claim_closed(options, runtime, smoke_token.token_address).await?;

    Ok(SmokeScenarioSummary {
        kind,
        token_symbol: smoke_token.symbol,
        token_address: smoke_token.token_address,
        migration: migration_status,
        claim: claim_status,
    })
}

async fn request_head_ok(client: &Client, url: Url) -> bool {
    match client.head(url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn tail_log(path: &Path, lines: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return format!("log file not found: {}", path.display());
    };
    let tail = content
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "last {lines} stack log lines from {}:\n{tail}",
        path.display()
    )
}

fn pick_erc20_smoke_token<'a>(
    tokens: &'a [EligibleToken],
    preferred_symbol: &str,
) -> eyre::Result<&'a EligibleToken> {
    let candidates = tokens
        .iter()
        .filter(|token| token.token_address != Address::ZERO)
        .collect::<Vec<_>>();
    ensure!(
        !candidates.is_empty(),
        "no ERC20 token found in payload: {}",
        serde_json::to_string_pretty(tokens)?
    );

    Ok(candidates
        .iter()
        .copied()
        .find(|token| token.symbol == preferred_symbol)
        .unwrap_or(candidates[0]))
}

fn pick_eth_smoke_token(tokens: &[EligibleToken]) -> eyre::Result<&EligibleToken> {
    tokens
        .iter()
        .find(|token| token.token_address == Address::ZERO)
        .ok_or_else(|| {
            eyre::eyre!(
                "no ETH token found in payload: {}",
                serde_json::to_string_pretty(tokens)
                    .unwrap_or_else(|_| "<invalid json>".to_string())
            )
        })
}

async fn poll_migration_status(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    migration_prepare: &MigrationPrepareResponse,
) -> eyre::Result<MigrationResponse> {
    let deadline = Instant::now() + options.timeout;
    let status_url = backend_url(
        &options.backend_base_url,
        &format!(
            "/api/migrations/{}",
            migration_prepare.migration_id.as_str()
        ),
    )?;
    let mut last = None;

    while Instant::now() < deadline {
        let current = get_json::<MigrationResponse>(&runtime.client, status_url.clone())
            .await
            .map_err(eyre::Report::new)
            .wrap_err("failed to poll migration status")?;
        if matches!(
            current.status,
            MigrationStatus::MinedSuccess | MigrationStatus::MinedReverted
        ) {
            return Ok(current);
        }
        last = Some(current);
        sleep(Duration::from_secs(1)).await;
    }

    bail!(
        "timed out polling migration status: {}",
        serde_json::to_string_pretty(&last)?
    )
}

async fn poll_claim_status(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    claim_submit: &ClaimResponse,
) -> eyre::Result<ClaimResponse> {
    let deadline = Instant::now() + options.timeout;
    let status_url = backend_url(
        &options.backend_base_url,
        &format!("/api/claims/{}", claim_submit.claim_id.as_str()),
    )?;
    let mut last = None;

    while Instant::now() < deadline {
        let current = get_json::<ClaimResponse>(&runtime.client, status_url.clone())
            .await
            .map_err(eyre::Report::new)
            .wrap_err("failed to poll claim status")?;
        if matches!(
            current.status,
            ClaimStatus::MinedSuccess | ClaimStatus::MinedReverted
        ) {
            return Ok(current);
        }
        last = Some(current);
        sleep(Duration::from_secs(1)).await;
    }

    bail!(
        "timed out polling claim status: {}",
        serde_json::to_string_pretty(&last)?
    )
}

fn sign_claim_batch(
    private_key: &str,
    claim_prepare: &ClaimPrepareResponse,
) -> eyre::Result<String> {
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

async fn verify_migration_tokens_metadata(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    token_address: Address,
) -> eyre::Result<()> {
    let response = get_json::<serde_json::Value>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/migrations/tokens")?,
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to load migration tokens metadata")?;
    let tokens = response
        .get("tokens")
        .and_then(|value| value.as_array())
        .ok_or_eyre("migrations tokens response omitted tokens array")?;
    let token = tokens
        .iter()
        .find(|token| {
            token
                .get("tokenAddress")
                .and_then(|value| value.as_str())
                .is_some_and(|addr| addr.eq_ignore_ascii_case(&format!("{token_address:#x}")))
        })
        .ok_or_eyre(format!(
            "token {token_address:#x} missing from migration tokens metadata"
        ))?;
    for forbidden in ["rawBalance", "displayBalance", "migrated", "claimed"] {
        ensure!(
            token.get(forbidden).is_none(),
            "migrations tokens metadata leaked field {forbidden}: {}",
            serde_json::to_string_pretty(token)?
        );
    }
    ensure!(
        token.get("symbol").and_then(|v| v.as_str()).is_some(),
        "migrations tokens metadata missing symbol: {}",
        serde_json::to_string_pretty(token)?
    );
    ensure!(
        token.get("decimals").and_then(|v| v.as_u64()).is_some(),
        "migrations tokens metadata missing decimals: {}",
        serde_json::to_string_pretty(token)?
    );
    Ok(())
}

async fn verify_migration_prepare_closed(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    token_address: Address,
) -> eyre::Result<()> {
    let response = post_json::<_, MigrationPrepareResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/migrations/prepare")?,
        &MigrationPrepareRequest {
            address: options.address,
            token_address,
        },
    )
    .await;

    match response {
        Ok(_) => bail!("migration prepare unexpectedly succeeded after migration completion"),
        Err(err) if err.status == StatusCode::CONFLICT => {
            let body: ApiErrorResponse = serde_json::from_str(&err.body)
                .wrap_err("failed to parse already-migrated rejection body")?;
            ensure!(
                body.code == "ALREADY_MIGRATED",
                "unexpected post-migration prepare response: {}",
                serde_json::to_string_pretty(&body)?
            );
            Ok(())
        }
        Err(err) => Err(eyre::Report::new(err)),
    }
}

async fn verify_claim_prepare_all_eligible(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    token_address: Address,
) -> eyre::Result<()> {
    let response = post_json::<_, ClaimPrepareResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/claims/prepare")?,
        &ClaimPrepareRequest {
            address: options.address,
            token_address: None,
            token_addresses: Vec::new(),
            all_eligible: true,
        },
    )
    .await
    .map_err(eyre::Report::new)
    .wrap_err("failed to prepare all eligible claims")?;
    ensure!(
        !response.items.is_empty(),
        "expected all-eligible claim prepare to return at least one item"
    );
    ensure!(
        response
            .items
            .iter()
            .any(|item| item.token_address == token_address),
        "all-eligible claim prepare did not include selected token"
    );
    Ok(())
}

async fn verify_invalid_claim_signature_rejected(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    claim_prepare: &ClaimPrepareResponse,
) -> eyre::Result<()> {
    let response = post_json::<_, ClaimResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/claims")?,
        &ClaimSubmitRequest {
            address: options.address,
            items: claim_prepare.items.clone(),
            claims_hash: claim_prepare.claims_hash,
            nonce: claim_prepare.nonce,
            expiry: claim_prepare.expiry,
            signature: "0x1234".to_string(),
        },
    )
    .await;

    match response {
        Ok(_) => bail!("claim submission unexpectedly succeeded with an invalid signature"),
        Err(err) if err.status == StatusCode::BAD_REQUEST => {
            let body: ApiErrorResponse = serde_json::from_str(&err.body)
                .wrap_err("failed to parse invalid-signature response body")?;
            ensure!(
                body.code == "INVALID_SIGNATURE",
                "unexpected invalid-signature response: {}",
                serde_json::to_string_pretty(&body)?
            );
            Ok(())
        }
        Err(err) => Err(eyre::Report::new(err)),
    }
}

fn verify_migration_delivery_path(
    token_address: Address,
    migration_status: &MigrationResponse,
) -> eyre::Result<()> {
    let delivery = migration_status
        .delivery
        .as_ref()
        .ok_or_eyre("migration status omitted delivery metadata")?;
    let expected_path = MigrationDeliveryPath::DirectL1ClaimContract;
    ensure!(
        delivery.path == expected_path,
        "unexpected delivery path: expected {:?}, got {:?}",
        expected_path,
        delivery.path
    );
    ensure!(
        delivery.token_address == token_address,
        "delivery token address mismatch"
    );
    Ok(())
}

fn restart_backend_service(options: &ComposeSeededSmokeOptions) -> eyre::Result<()> {
    let mut args = compose_base_args(&options.compose_file, &options.infra_env_file);
    args.extend([
        "restart".to_string(),
        "zksync-lite-sunset-server".to_string(),
    ]);
    let status = Command::new("docker")
        .args(&args)
        .status()
        .wrap_err("failed to execute docker compose restart for backend service")?;
    ensure!(
        status.success(),
        "docker compose restart zksync-lite-sunset-server exited with status {status}"
    );
    Ok(())
}

async fn verify_claim_closed(
    options: &ComposeSeededSmokeOptions,
    runtime: &RuntimeContext,
    token_address: Address,
) -> eyre::Result<()> {
    let response = post_json::<_, ClaimPrepareResponse>(
        &runtime.client,
        backend_url(&options.backend_base_url, "/api/claims/prepare")?,
        &ClaimPrepareRequest {
            address: options.address,
            token_address: Some(token_address),
            token_addresses: Vec::new(),
            all_eligible: false,
        },
    )
    .await;

    match response {
        Ok(_) => bail!("claim prepare unexpectedly succeeded after claim completion"),
        Err(err) if err.status == StatusCode::NOT_FOUND => {
            let body: ApiErrorResponse = serde_json::from_str(&err.body)
                .wrap_err("failed to parse post-claim prepare rejection body")?;
            ensure!(
                body.code == "TOKEN_NOT_ELIGIBLE",
                "unexpected post-claim response: {}",
                serde_json::to_string_pretty(&body)?
            );
            Ok(())
        }
        Err(err) => Err(eyre::Report::new(err)),
    }
}

fn cleanup(options: &ComposeSeededSmokeOptions, runtime: &mut RuntimeContext) -> eyre::Result<()> {
    if !options.keep_stack_running
        && let Some(stack_proc) = runtime.stack_proc.as_mut()
    {
        terminate_child(stack_proc)?;
    }

    if options.teardown_compose && !runtime.compose_was_running && !options.keep_stack_running {
        let mut args = compose_base_args(&options.compose_file, &options.infra_env_file);
        args.extend([
            "down".to_string(),
            "--volumes".to_string(),
            "--remove-orphans".to_string(),
        ]);
        let status = Command::new("docker")
            .args(&args)
            .status()
            .wrap_err("failed to run docker compose down during smoke cleanup")?;
        ensure!(
            status.success(),
            "docker compose down exited with status {status}"
        );
    }

    Ok(())
}

fn terminate_child(child: &mut Child) -> eyre::Result<()> {
    if child
        .try_wait()
        .wrap_err("failed to inspect compose seeded stack process")?
        .is_some()
    {
        return Ok(());
    }

    let status = Command::new("kill")
        .arg(child.id().to_string())
        .status()
        .wrap_err("failed to send SIGTERM to compose seeded stack process")?;
    ensure!(status.success(), "kill returned non-zero status {status}");

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if child
            .try_wait()
            .wrap_err("failed to wait for compose seeded stack process")?
            .is_some()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    child
        .kill()
        .wrap_err("failed to SIGKILL compose seeded stack process after SIGTERM timeout")?;
    child
        .wait()
        .wrap_err("failed to reap compose seeded stack process")?;
    Ok(())
}

async fn get_json<T>(client: &Client, url: Url) -> Result<T, HttpJsonError>
where
    T: DeserializeOwned,
{
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|err| HttpJsonError {
            url: url.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: err.to_string(),
        })?;
    parse_json_response(response, url).await
}

async fn post_json<Req, Resp>(
    client: &Client,
    url: Url,
    payload: &Req,
) -> Result<Resp, HttpJsonError>
where
    Req: Serialize + ?Sized,
    Resp: DeserializeOwned,
{
    let response = client
        .post(url.clone())
        .json(payload)
        .send()
        .await
        .map_err(|err| HttpJsonError {
            url: url.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: err.to_string(),
        })?;
    parse_json_response(response, url).await
}

async fn parse_json_response<T>(response: reqwest::Response, url: Url) -> Result<T, HttpJsonError>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let body = response.text().await.map_err(|err| HttpJsonError {
        url: url.to_string(),
        status,
        body: err.to_string(),
    })?;
    if !status.is_success() {
        return Err(HttpJsonError {
            url: url.to_string(),
            status,
            body,
        });
    }
    serde_json::from_str(&body).map_err(|err| HttpJsonError {
        url: url.to_string(),
        status,
        body: format!("failed to decode JSON: {err}; body={body}"),
    })
}

async fn rpc_call(
    client: &Client,
    rpc_url: &Url,
    method: &str,
    params: serde_json::Value,
) -> eyre::Result<B256> {
    let response = client
        .post(rpc_url.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .wrap_err_with(|| format!("failed to send JSON-RPC request {method} to {rpc_url}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .wrap_err("failed to read JSON-RPC response body")?;
    ensure!(
        status.is_success(),
        "rpc {method} returned HTTP {status}: {body}"
    );

    let parsed: JsonRpcResponse<String> =
        serde_json::from_str(&body).wrap_err("failed to parse JSON-RPC response")?;
    if let Some(error) = parsed.error {
        bail!("rpc {method} failed: {error}");
    }
    let tx_hash = parsed
        .result
        .ok_or_eyre(format!("rpc {method} returned no result"))?;
    B256::from_str(&tx_hash).wrap_err_with(|| format!("invalid tx hash returned from {method}"))
}

fn backend_url(base: &Url, path: &str) -> eyre::Result<Url> {
    let mut url = base.clone();
    url.set_path(path);
    url.set_query(None);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{pick_erc20_smoke_token, pick_eth_smoke_token};
    use alloy::primitives::{Address, U256};
    use eyre::Result;
    use zksync_lite_sunset_core::eligibility::EligibleToken;

    #[test]
    fn pick_erc20_smoke_token_prefers_requested_symbol() -> Result<()> {
        let usdc = token("USDC", 1);
        let dai = token("DAI", 2);
        let tokens = [usdc, dai.clone()];
        let picked = pick_erc20_smoke_token(&tokens, "DAI")?;
        assert_eq!(picked.symbol, "DAI");
        assert_eq!(picked.token_address, dai.token_address);
        Ok(())
    }

    #[test]
    fn pick_erc20_smoke_token_skips_eth() -> Result<()> {
        let eth = token("ETH", 0);
        let dai = token("DAI", 2);
        let tokens = [eth, dai.clone()];
        let picked = pick_erc20_smoke_token(&tokens, "ETH")?;
        assert_eq!(picked.symbol, "DAI");
        assert_eq!(picked.token_address, dai.token_address);
        Ok(())
    }

    #[test]
    fn pick_eth_smoke_token_selects_native_asset() -> Result<()> {
        let dai = token("DAI", 2);
        let eth = token("ETH", 0);
        let tokens = [dai, eth.clone()];
        let picked = pick_eth_smoke_token(&tokens)?;
        assert_eq!(picked.symbol, "ETH");
        assert_eq!(picked.token_address, Address::ZERO);
        Ok(())
    }

    fn token(symbol: &str, suffix: u8) -> EligibleToken {
        EligibleToken {
            claim_index: u64::from(suffix),
            token_address: if suffix == 0 {
                Address::ZERO
            } else {
                Address::from([suffix; 20])
            },
            symbol: symbol.to_string(),
            display_balance: "1.00".to_string(),
            raw_balance: U256::from(1),
            decimals: 18,
        }
    }
}
