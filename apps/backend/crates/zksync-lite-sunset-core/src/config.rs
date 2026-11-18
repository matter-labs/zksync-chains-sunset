use alloy::primitives::Address;
use eyre::{WrapErr as _, eyre};
use serde::{Deserialize, Serialize, Serializer, de};
use std::{fmt, str::FromStr};
use url::Url;

const ENV_DATABASE_URL: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL";
const ENV_DATABASE_HOST: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__HOST";
const ENV_DATABASE_PORT: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__PORT";
const ENV_DATABASE_NAME: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__NAME";
const ENV_DATABASE_USER: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__USER";
const ENV_DATABASE_PASSWORD: &str = "ZKSYNC_LITE_SUNSET__CORE__DATABASE__PASSWORD";
const ENV_DATABASE_INSTANCE_UNIX_SOCKET: &str =
    "ZKSYNC_LITE_SUNSET__CORE__DATABASE__INSTANCE_UNIX_SOCKET";
const ENV_EXECUTION_RPC_URL: &str = "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub claims: ClaimsConfig,
    #[serde(default)]
    pub materialization: MaterializationConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AuthConfig {
    #[serde(default = "default_claim_chain_id")]
    pub claim_chain_id: u64,
    #[serde(default = "default_migrate_chain_id")]
    pub migrate_chain_id: u64,
}

const fn default_claim_chain_id() -> u64 {
    1
}

const fn default_migrate_chain_id() -> u64 {
    1
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            claim_chain_id: default_claim_chain_id(),
            migrate_chain_id: default_migrate_chain_id(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: Url,
    #[serde(default = "default_database_auto_migrate")]
    pub auto_migrate: bool,
}

fn default_database_url() -> Url {
    Url::parse("postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset")
        .expect("hardcoded postgres URL should parse")
}

const fn default_database_auto_migrate() -> bool {
    true
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
            auto_migrate: default_database_auto_migrate(),
        }
    }
}

impl Config {
    pub fn apply_runtime_env_overrides(mut self) -> eyre::Result<Self> {
        self.database.apply_runtime_env_overrides()?;
        self.execution.apply_runtime_env_overrides()?;
        Ok(self)
    }
}

impl DatabaseConfig {
    fn apply_runtime_env_overrides(&mut self) -> eyre::Result<()> {
        if std::env::var_os(ENV_DATABASE_URL).is_some() {
            return Ok(());
        }

        let Some(parts) = DatabaseUrlParts::from_env()? else {
            return Ok(());
        };

        self.url = parts.into_url()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ClaimsConfig {
    #[serde(default = "default_claim_signature_ttl_seconds")]
    pub signature_ttl_seconds: u64,
    #[serde(default = "default_claim_frontend_mode")]
    pub frontend_mode: ClaimFrontendMode,
    #[serde(default)]
    pub worker: ClaimWorkerConfig,
}

const fn default_claim_signature_ttl_seconds() -> u64 {
    15 * 60
}

const fn default_claim_frontend_mode() -> ClaimFrontendMode {
    ClaimFrontendMode::Paid
}

impl Default for ClaimsConfig {
    fn default() -> Self {
        Self {
            signature_ttl_seconds: default_claim_signature_ttl_seconds(),
            frontend_mode: default_claim_frontend_mode(),
            worker: ClaimWorkerConfig::default(),
        }
    }
}

/// Tuning for the background claim broadcast worker. The worker drains rows
/// in `pending` state, broadcasts the sponsor multicall, and transitions them
/// to `submitted`. With the current single-replica deployment posture the
/// worker may run in-process; if you ever lift to multi-replica, ensure only
/// one instance has `enabled = true`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ClaimWorkerConfig {
    #[serde(default = "default_claim_worker_enabled")]
    pub enabled: bool,
    #[serde(default = "default_claim_worker_tick_interval_ms")]
    pub tick_interval_ms: u64,
    #[serde(default = "default_claim_worker_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_claim_worker_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_claim_worker_backoff_cap_seconds")]
    pub backoff_cap_seconds: u64,
    #[serde(default = "default_claim_worker_finalize_poll_interval_ms")]
    pub finalize_poll_interval_ms: u64,
}

const fn default_claim_worker_enabled() -> bool {
    true
}

const fn default_claim_worker_tick_interval_ms() -> u64 {
    500
}

const fn default_claim_worker_max_attempts() -> u32 {
    5
}

const fn default_claim_worker_batch_size() -> u32 {
    16
}

const fn default_claim_worker_backoff_cap_seconds() -> u64 {
    60
}

const fn default_claim_worker_finalize_poll_interval_ms() -> u64 {
    // ~one L1 block. Sweeping faster wastes RPC: `min_confirmations`
    // is 4, so a freshly broadcast tx can't reach a terminal state for
    // ~48s of wall time regardless of how often we poll.
    15_000
}

impl Default for ClaimWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: default_claim_worker_enabled(),
            tick_interval_ms: default_claim_worker_tick_interval_ms(),
            max_attempts: default_claim_worker_max_attempts(),
            batch_size: default_claim_worker_batch_size(),
            backoff_cap_seconds: default_claim_worker_backoff_cap_seconds(),
            finalize_poll_interval_ms: default_claim_worker_finalize_poll_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimFrontendMode {
    Unavailable,
    Sponsored,
    Paid,
}

impl ClaimFrontendMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Sponsored => "sponsored",
            Self::Paid => "paid",
        }
    }

    pub const fn is_sponsored(self) -> bool {
        matches!(self, Self::Sponsored)
    }
}

impl FromStr for ClaimFrontendMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unavailable" => Ok(Self::Unavailable),
            "sponsored" => Ok(Self::Sponsored),
            "paid" => Ok(Self::Paid),
            _ => Err(format!(
                "invalid claim frontend mode {value:?}; expected unavailable, sponsored, or paid"
            )),
        }
    }
}

impl Serialize for ClaimFrontendMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ClaimFrontendMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct MaterializationConfig {
    #[serde(default = "default_snapshot_label_prefix")]
    pub snapshot_label_prefix: String,
}

fn default_snapshot_label_prefix() -> String {
    "snapshot".to_string()
}

impl Default for MaterializationConfig {
    fn default() -> Self {
        Self {
            snapshot_label_prefix: default_snapshot_label_prefix(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionConfig {
    #[serde(default = "default_execution_rpc_url")]
    pub rpc_url: Url,
    #[serde(default)]
    pub from_address: Option<Address>,
    #[serde(default)]
    pub sponsor_private_key: Option<SecretString>,
    #[serde(default)]
    pub migration_contract_address: Option<Address>,
    #[serde(default)]
    pub claim_contract_address: Option<Address>,
    #[serde(default)]
    pub multicall3_contract_address: Option<Address>,
    #[serde(default = "default_execution_receipt_poll_interval_ms")]
    pub receipt_poll_interval_ms: u64,
    #[serde(default = "default_execution_receipt_timeout_seconds")]
    pub receipt_timeout_seconds: u64,
    #[serde(default = "default_execution_min_confirmations")]
    pub min_confirmations: u64,
    #[serde(default = "default_execution_max_retries")]
    pub max_retries: u32,
}

fn default_execution_rpc_url() -> Url {
    Url::parse("http://127.0.0.1:8545").expect("hardcoded execution L1 RPC URL should parse")
}

const fn default_execution_receipt_poll_interval_ms() -> u64 {
    1_000
}

const fn default_execution_receipt_timeout_seconds() -> u64 {
    180
}

const fn default_execution_min_confirmations() -> u64 {
    4
}

const fn default_execution_max_retries() -> u32 {
    2
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            rpc_url: default_execution_rpc_url(),
            from_address: None,
            sponsor_private_key: None,
            migration_contract_address: None,
            claim_contract_address: None,
            multicall3_contract_address: None,
            receipt_poll_interval_ms: default_execution_receipt_poll_interval_ms(),
            receipt_timeout_seconds: default_execution_receipt_timeout_seconds(),
            min_confirmations: default_execution_min_confirmations(),
            max_retries: default_execution_max_retries(),
        }
    }
}

impl ExecutionConfig {
    pub fn rpc_url(&self) -> &Url {
        &self.rpc_url
    }

    fn apply_runtime_env_overrides(&mut self) -> eyre::Result<()> {
        if let Some(rpc_url) = env_var(ENV_EXECUTION_RPC_URL) {
            self.rpc_url = Url::parse(&rpc_url)
                .wrap_err_with(|| format!("failed to parse {ENV_EXECUTION_RPC_URL}"))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseUrlParts {
    host: Option<String>,
    port: Option<u16>,
    name: String,
    user: String,
    password: String,
    instance_unix_socket: Option<String>,
}

impl DatabaseUrlParts {
    fn from_env() -> eyre::Result<Option<Self>> {
        let host = env_var(ENV_DATABASE_HOST);
        let port = env_var(ENV_DATABASE_PORT)
            .map(|value| {
                value.parse::<u16>().wrap_err_with(|| {
                    format!("failed to parse {ENV_DATABASE_PORT} as a valid TCP port")
                })
            })
            .transpose()?;
        let name = env_var(ENV_DATABASE_NAME);
        let user = env_var(ENV_DATABASE_USER);
        let password = env_var(ENV_DATABASE_PASSWORD);
        let instance_unix_socket = env_var(ENV_DATABASE_INSTANCE_UNIX_SOCKET);

        if host.is_none()
            && port.is_none()
            && name.is_none()
            && user.is_none()
            && password.is_none()
            && instance_unix_socket.is_none()
        {
            return Ok(None);
        }

        let name = name
            .ok_or_else(|| eyre!("missing {ENV_DATABASE_NAME} when database URL parts are used"))?;
        let user = user
            .ok_or_else(|| eyre!("missing {ENV_DATABASE_USER} when database URL parts are used"))?;
        let password = password.ok_or_else(|| {
            eyre!("missing {ENV_DATABASE_PASSWORD} when database URL parts are used")
        })?;

        if host.is_some() && instance_unix_socket.is_some() {
            return Err(eyre!(
                "set either {ENV_DATABASE_HOST} or {ENV_DATABASE_INSTANCE_UNIX_SOCKET}, not both"
            ));
        }

        if host.is_none() && instance_unix_socket.is_none() {
            return Err(eyre!(
                "missing {ENV_DATABASE_HOST} or {ENV_DATABASE_INSTANCE_UNIX_SOCKET} when database URL parts are used"
            ));
        }

        Ok(Some(Self {
            host,
            port,
            name,
            user,
            password,
            instance_unix_socket,
        }))
    }

    fn into_url(self) -> eyre::Result<Url> {
        let mut url =
            Url::parse("postgres://localhost").expect("hardcoded postgres base URL should parse");

        url.set_username(&self.user)
            .map_err(|_| eyre!("failed to set database username in runtime database URL"))?;
        url.set_password(Some(&self.password))
            .map_err(|_| eyre!("failed to set database password in runtime database URL"))?;
        url.set_path(&format!("/{}", self.name));

        match (self.host, self.instance_unix_socket) {
            (Some(host), None) => {
                url.set_host(Some(&host))
                    .map_err(|_| eyre!("invalid {ENV_DATABASE_HOST}: {host}"))?;
            }
            (None, Some(socket_path)) => {
                url.set_host(Some("localhost"))
                    .expect("localhost should always be a valid database host");
                url.query_pairs_mut().append_pair("host", &socket_path);
            }
            _ => unreachable!("database URL parts are validated before URL assembly"),
        }

        if let Some(port) = self.port {
            url.set_port(Some(port))
                .map_err(|_| eyre!("failed to set database port in runtime database URL"))?;
        }

        Ok(url)
    }
}

fn env_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimFrontendMode, ClaimsConfig, DatabaseUrlParts, ENV_DATABASE_HOST,
        ENV_DATABASE_INSTANCE_UNIX_SOCKET, ENV_DATABASE_NAME, ENV_DATABASE_PASSWORD,
        ENV_DATABASE_PORT, ENV_DATABASE_USER, ENV_EXECUTION_RPC_URL, ExecutionConfig, SecretString,
    };
    use serde_json::json;

    #[test]
    fn database_url_parts_build_tcp_url() {
        let url = DatabaseUrlParts {
            host: Some("postgres.internal".to_string()),
            port: Some(5433),
            name: "zksync_lite_sunset".to_string(),
            user: "app".to_string(),
            password: "secret".to_string(),
            instance_unix_socket: None,
        }
        .into_url()
        .unwrap();

        assert_eq!(url.scheme(), "postgres");
        assert_eq!(url.host_str(), Some("postgres.internal"));
        assert_eq!(url.port(), Some(5433));
        assert_eq!(url.username(), "app");
        assert_eq!(url.password(), Some("secret"));
        assert_eq!(url.path(), "/zksync_lite_sunset");
        assert_eq!(url.query(), None);
    }

    #[test]
    fn database_url_parts_build_unix_socket_url() {
        let url = DatabaseUrlParts {
            host: None,
            port: Some(5432),
            name: "zksync_lite_sunset".to_string(),
            user: "app".to_string(),
            password: "secret".to_string(),
            instance_unix_socket: Some("/cloudsql/project:us-central1:db".to_string()),
        }
        .into_url()
        .unwrap();

        assert_eq!(url.host_str(), Some("localhost"));
        assert_eq!(url.port(), Some(5432));
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "host")
                .map(|(_, value)| value.into_owned()),
            Some("/cloudsql/project:us-central1:db".to_string())
        );
    }

    #[test]
    fn database_url_env_keys_match_expected_runtime_shape() {
        assert_eq!(
            ENV_DATABASE_NAME,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__NAME"
        );
        assert_eq!(
            ENV_DATABASE_USER,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__USER"
        );
        assert_eq!(
            ENV_DATABASE_PASSWORD,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__PASSWORD"
        );
        assert_eq!(
            ENV_DATABASE_HOST,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__HOST"
        );
        assert_eq!(
            ENV_DATABASE_PORT,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__PORT"
        );
        assert_eq!(
            ENV_DATABASE_INSTANCE_UNIX_SOCKET,
            "ZKSYNC_LITE_SUNSET__CORE__DATABASE__INSTANCE_UNIX_SOCKET"
        );
    }

    #[test]
    fn execution_config_accepts_single_rpc_field() {
        let config: ExecutionConfig = serde_json::from_value(json!({
            "rpc_url": "http://127.0.0.1:5010"
        }))
        .unwrap();

        assert_eq!(config.rpc_url().as_str(), "http://127.0.0.1:5010/");
    }

    #[test]
    fn execution_config_runtime_env_overrides_rpc_url() {
        let mut config = ExecutionConfig::default();

        // SAFETY: tests update process env in isolation.
        unsafe {
            std::env::set_var(ENV_EXECUTION_RPC_URL, "https://rpc.example");
        }
        config.apply_runtime_env_overrides().unwrap();
        // SAFETY: tests update process env in isolation.
        unsafe {
            std::env::remove_var(ENV_EXECUTION_RPC_URL);
        }

        assert_eq!(config.rpc_url().as_str(), "https://rpc.example/");
    }

    #[test]
    fn execution_runtime_env_keys_match_expected_shape() {
        assert_eq!(
            ENV_EXECUTION_RPC_URL,
            "ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL"
        );
    }

    #[test]
    fn execution_config_debug_redacts_sponsor_private_key() {
        let config = ExecutionConfig {
            sponsor_private_key: Some(SecretString::from(
                "0xfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeed",
            )),
            ..ExecutionConfig::default()
        };

        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("feedfeed"));
    }

    #[test]
    fn claims_config_defaults_to_paid_frontend_mode() {
        let config = ClaimsConfig::default();

        assert_eq!(config.frontend_mode, ClaimFrontendMode::Paid);
        assert_eq!(config.signature_ttl_seconds, 15 * 60);
    }

    #[test]
    fn claims_config_deserializes_frontend_mode() {
        let config: ClaimsConfig = serde_json::from_value(json!({
            "frontend_mode": "sponsored",
            "signature_ttl_seconds": 60
        }))
        .unwrap();

        assert_eq!(config.frontend_mode, ClaimFrontendMode::Sponsored);
        assert_eq!(config.signature_ttl_seconds, 60);
    }
}
