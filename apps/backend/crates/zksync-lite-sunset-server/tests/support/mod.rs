use alloy::signers::local::PrivateKeySigner;
use eyre::{Result, WrapErr as _};
use sqlx::postgres::PgPoolOptions;
use zksync_lite_sunset_core::{config::Config as CoreConfig, world::World};

const SPONSOR_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

pub fn dummy_world() -> Result<World> {
    dummy_world_with(|_| {})
}

pub fn dummy_world_with(mut configure: impl FnMut(&mut CoreConfig)) -> Result<World> {
    let mut config = CoreConfig::default();
    configure(&mut config);
    config.database.url = "postgres://postgres:postgres@127.0.0.1:1/zksync_lite_sunset_dummy"
        .parse()
        .unwrap();
    config.execution.rpc_url = "http://127.0.0.1:1".parse().unwrap();

    let signer: PrivateKeySigner = SPONSOR_PRIVATE_KEY.trim_start_matches("0x").parse()?;
    config.execution.from_address = Some(signer.address());
    config.execution.sponsor_private_key = Some(SPONSOR_PRIVATE_KEY.to_string().into());

    let pool = PgPoolOptions::new()
        .connect_lazy(config.database.url.as_str())
        .wrap_err("failed to construct lazy postgres pool")?;

    Ok(World::with_db_pool(config, pool))
}
