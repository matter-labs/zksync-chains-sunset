use axum::http::HeaderValue;
use eyre::{WrapErr, eyre};
use serde::Deserialize;
use std::net::SocketAddr;

use zksync_lite_sunset_core::config::Config as CoreConfig;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub metrics: MetricsConfig,

    #[serde(default)]
    pub core: CoreConfig,

    #[serde(default)]
    pub cors: CorsConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_metrics_host")]
    pub host: String,
    #[serde(default = "default_metrics_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub enabled: bool,

    // Narrow allowlist for local dev/e2e. This is only used when `enabled=true`.
    #[serde(
        default = "default_allowed_origins",
        deserialize_with = "deserialize_allowed_origins"
    )]
    pub allowed_origins: Vec<String>,
}

fn default_allowed_origins() -> Vec<String> {
    vec![
        "http://localhost:4321".to_string(),
        "http://127.0.0.1:4321".to_string(),
        "http://localhost:4322".to_string(),
        "http://127.0.0.1:4322".to_string(),
        "http://localhost:4421".to_string(),
        "http://127.0.0.1:4421".to_string(),
        "http://localhost:4422".to_string(),
        "http://127.0.0.1:4422".to_string(),
    ]
}

const fn default_metrics_enabled() -> bool {
    true
}

fn default_metrics_host() -> String {
    "127.0.0.1".to_string()
}

const fn default_metrics_port() -> u16 {
    3001
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            host: default_metrics_host(),
            port: default_metrics_port(),
        }
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_origins: default_allowed_origins(),
        }
    }
}

impl CorsConfig {
    pub fn enabled_for_dev() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    fn validate(&self) -> eyre::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        for origin in &self.allowed_origins {
            origin
                .parse::<HeaderValue>()
                .map_err(|_| eyre!("invalid CORS origin: {origin}"))?;
        }

        Ok(())
    }
}

impl Config {
    pub fn load() -> eyre::Result<Self> {
        let config = config::Config::builder()
            // Bind to localhost by default for safer local dev.
            // Containers/compose can override to `0.0.0.0` via env vars.
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 3000)?
            .set_default("metrics.enabled", default_metrics_enabled())?
            .set_default("metrics.host", default_metrics_host())?
            .set_default("metrics.port", default_metrics_port())?
            .add_source(config::Environment::with_prefix("ZKSYNC_LITE_SUNSET").separator("__"))
            .build()?;

        let mut cfg: Self = config
            .try_deserialize()
            .wrap_err("failed to deserialize server settings")?;

        cfg.core = cfg.core.apply_runtime_env_overrides()?;
        cfg.cors.validate()?;
        Ok(cfg)
    }
}

impl ServerConfig {
    pub fn addr(&self) -> eyre::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .wrap_err_with(|| format!("invalid server address: {addr}"))
    }
}

impl MetricsConfig {
    pub fn addr(&self) -> eyre::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .wrap_err_with(|| format!("invalid metrics address: {addr}"))
    }
}

fn deserialize_allowed_origins<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AllowedOriginsValue {
        List(Vec<String>),
        String(String),
    }

    let value = Option::<AllowedOriginsValue>::deserialize(deserializer)?;
    match value {
        None => Ok(default_allowed_origins()),
        Some(AllowedOriginsValue::List(origins)) => Ok(origins),
        Some(AllowedOriginsValue::String(raw)) => {
            parse_allowed_origins(&raw).map_err(serde::de::Error::custom)
        }
    }
}

fn parse_allowed_origins(raw: &str) -> eyre::Result<Vec<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<String>>(trimmed)
            .wrap_err("failed to parse CORS allowed origins as JSON");
    }

    Ok(trimmed
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(ToString::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Config, parse_allowed_origins};
    use serial_test::serial;
    use std::ffi::OsString;

    const ENV_CLAIMS_FRONTEND_MODE: &str = "ZKSYNC_LITE_SUNSET__CORE__CLAIMS__FRONTEND_MODE";

    #[test]
    fn parses_csv_cors_allowed_origins() {
        let origins =
            parse_allowed_origins("https://claim.example.com, https://migrate.example.com")
                .unwrap();

        assert_eq!(
            origins,
            vec![
                "https://claim.example.com".to_string(),
                "https://migrate.example.com".to_string(),
            ]
        );
    }

    #[test]
    fn parses_json_cors_allowed_origins() {
        let origins =
            parse_allowed_origins(r#"["https://claim.example.com","https://migrate.example.com"]"#)
                .unwrap();

        assert_eq!(
            origins,
            vec![
                "https://claim.example.com".to_string(),
                "https://migrate.example.com".to_string(),
            ]
        );
    }

    #[test]
    #[serial]
    fn load_reads_claim_frontend_mode_from_env() {
        let previous = std::env::var_os(ENV_CLAIMS_FRONTEND_MODE);
        // SAFETY: this test is serialized because it mutates process env.
        unsafe {
            std::env::set_var(ENV_CLAIMS_FRONTEND_MODE, "sponsored");
        }

        let config = Config::load();
        restore_env(ENV_CLAIMS_FRONTEND_MODE, previous);
        let config = config.unwrap();

        assert!(config.core.claims.frontend_mode.is_sponsored());
    }

    fn restore_env(key: &str, previous: Option<OsString>) {
        // SAFETY: callers serialize tests that mutate process env.
        unsafe {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
