#[path = "claim_env.rs"]
mod claim_env;

use alloy::primitives::Address;
use eyre::Result;
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
