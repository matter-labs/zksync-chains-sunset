#[path = "claim_env.rs"]
mod claim_env;

use alloy::primitives::Address;
use eyre::{Result, WrapErr as _};
use zksync_lite_sunset_core::claims::prepare::ClaimPrepareResponse;

pub fn test_signer_address() -> Address {
    claim_env::test_signer_address()
}

pub fn sign_claim_batch(claim_prepare: &ClaimPrepareResponse) -> Result<String> {
    claim_env::sign_claim_batch(claim_prepare)
}

pub async fn setup() -> Result<()> {
    let config = claim_env::init_chain_with_claimant(|_, _| Ok(claim_env::test_signer_address()))?;
    claim_env::complete_setup(&config).await
}

pub async fn mark_all_claim_tokens_migrated(user: Address) -> Result<()> {
    claim_env::mark_all_claim_tokens_migrated(user).await
}

pub fn pause_claims() -> Result<()> {
    claim_owner_contract_call("pause()", &[])
}

pub fn unpause_claims() -> Result<()> {
    claim_owner_contract_call("unpause()", &[])
}

fn claim_owner_contract_call(signature: &str, args: &[String]) -> Result<()> {
    let rpc_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", claim_env::TEST_RPC_PORT));
    let claim_contract: Address =
        std::env::var("ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS")
            .wrap_err("claim contract address not configured in test environment")?
            .parse()
            .wrap_err("failed to parse configured claim contract address")?;

    claim_env::send_contract_call_with_private_key(
        &rpc_url,
        claim_env::TEST_SIGNER_PRIVATE_KEY,
        claim_contract,
        signature,
        args,
    )
}
