use crate::config::Config;
use crate::observability::Observability;
use crate::persistence;
use alloy::primitives::Address;
use alloy::providers::RootProvider;
use alloy::signers::local::PrivateKeySigner;
use eyre::WrapErr;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct World {
    inner: Arc<WorldInner>,
}

impl World {
    pub async fn new(config: Config) -> eyre::Result<Self> {
        let db_pool = persistence::connect(&config).await?;
        Self::try_with_db_pool(config, db_pool)
    }

    pub fn with_db_pool(config: Config, db_pool: PgPool) -> Self {
        Self::try_with_db_pool(config, db_pool).expect("invalid world configuration")
    }

    pub fn try_with_db_pool(config: Config, db_pool: PgPool) -> eyre::Result<Self> {
        let provider = build_provider(config.execution.rpc_url().clone());
        let (sponsor_signer, sponsor_address) = load_sponsor_signer(&config)?;
        Ok(Self::with_db_pool_and_provider(
            config,
            db_pool,
            provider,
            sponsor_signer,
            sponsor_address,
        ))
    }

    pub fn with_db_pool_and_provider(
        config: Config,
        db_pool: PgPool,
        provider: RootProvider,
        sponsor_signer: PrivateKeySigner,
        sponsor_address: Address,
    ) -> Self {
        Self {
            inner: Arc::new(WorldInner {
                config,
                db_pool,
                provider,
                observability: Observability::new(),
                sponsor_signer,
                sponsor_address,
                sponsor_nonce: Mutex::new(None),
            }),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn db_pool(&self) -> &PgPool {
        &self.inner.db_pool
    }

    pub fn provider(&self) -> &RootProvider {
        &self.inner.provider
    }

    pub fn l1_provider(&self) -> &RootProvider {
        self.provider()
    }

    pub fn sponsor_signer(&self) -> PrivateKeySigner {
        self.inner.sponsor_signer.clone()
    }

    pub fn observability(&self) -> &Observability {
        &self.inner.observability
    }

    pub fn sponsor_address(&self) -> Address {
        self.inner.sponsor_address
    }

    /// Serializes sponsor-account transaction submissions so concurrent
    /// requests can't hand alloy's nonce auto-fill the same pending-block
    /// nonce and race each other into the mempool. Holds the locally-tracked
    /// next nonce — `None` means "re-seed from chain on next use".
    pub(crate) fn sponsor_nonce_lock(&self) -> &Mutex<Option<u64>> {
        &self.inner.sponsor_nonce
    }
}

#[derive(Debug)]
struct WorldInner {
    config: Config,
    db_pool: PgPool,
    provider: RootProvider,
    observability: Observability,
    sponsor_signer: PrivateKeySigner,
    sponsor_address: Address,
    sponsor_nonce: Mutex<Option<u64>>,
}

fn build_provider(rpc_url: url::Url) -> RootProvider {
    RootProvider::new_http(rpc_url)
}

fn load_sponsor_signer(config: &Config) -> eyre::Result<(PrivateKeySigner, Address)> {
    let private_key = config
        .execution
        .sponsor_private_key
        .as_ref()
        .map(crate::config::SecretString::expose_secret)
        .ok_or_else(|| eyre::eyre!("missing execution.sponsor_private_key"))?
        .trim_start_matches("0x");
    let signer: PrivateKeySigner = private_key
        .parse()
        .wrap_err("failed to parse execution.sponsor_private_key")?;
    let signer_address = signer.address();

    if let Some(from_address) = config.execution.from_address
        && from_address != signer_address
    {
        return Err(eyre::eyre!(
            "execution.from_address ({from_address:#x}) does not match execution.sponsor_private_key signer ({signer_address:#x})"
        ));
    }

    Ok((signer, signer_address))
}
