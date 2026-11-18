#[path = "claim_env.rs"]
mod claim_env;

use std::process::Command;

use alloy::primitives::{Address, B256};
use eyre::{Result, WrapErr as _, ensure};
use zksync_lite_sunset_core::claims::prepare::ClaimPrepareResponse;

pub fn sign_claim_batch(claim_prepare: &ClaimPrepareResponse) -> Result<String> {
    claim_env::sign_claim_batch(claim_prepare)
}

pub fn sign_claim_batch_as_sponsor(claim_prepare: &ClaimPrepareResponse) -> Result<String> {
    claim_env::sign_claim_batch_with_private_key(claim_prepare, claim_env::SPONSOR_PRIVATE_KEY)
}

pub async fn setup_smart_account() -> Result<Address> {
    let config = claim_env::init_chain_with_claimant(|contracts_dir, rpc_url| {
        claim_env::deploy_contract(
            contracts_dir,
            rpc_url,
            "src/mocks/MockERC1271Wallet.sol:MockERC1271Wallet",
            &[format!("{:#x}", claim_env::test_signer_address())],
        )
    })?;
    let claimant = config.claimant;
    claim_env::complete_setup(&config).await?;
    Ok(claimant)
}

pub async fn mark_all_claim_tokens_migrated(user: Address) -> Result<()> {
    claim_env::mark_all_claim_tokens_migrated(user).await
}

pub fn execute_smart_wallet(wallet: Address, target: Address, data: &str) -> Result<B256> {
    let rpc_url = std::env::var("ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", claim_env::TEST_RPC_PORT));
    send_contract_call_with_private_key_and_hash(
        &rpc_url,
        claim_env::TEST_SIGNER_PRIVATE_KEY,
        wallet,
        "execute(address,bytes)",
        &[format!("{target:#x}"), data.to_string()],
    )
}

fn send_contract_call_with_private_key_and_hash(
    rpc_url: &str,
    private_key: &str,
    to: Address,
    signature: &str,
    args: &[String],
) -> Result<B256> {
    let output = Command::new("cast")
        .arg("send")
        .arg("--json")
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

    parse_cast_send_tx_hash(&output.stdout, &output.stderr)
}

fn parse_cast_send_tx_hash(stdout: &[u8], stderr: &[u8]) -> Result<B256> {
    let stdout_text = String::from_utf8_lossy(stdout);
    let combined = format!("{}{}", stdout_text, String::from_utf8_lossy(stderr));
    let json_value: serde_json::Value = serde_json::from_str(stdout_text.trim())
        .wrap_err_with(|| format!("failed to parse cast send json output: {combined}"))?;

    for key in ["transactionHash", "transaction_hash", "hash"] {
        if let Some(value) = json_value.get(key).and_then(|value| value.as_str()) {
            return value
                .parse()
                .wrap_err_with(|| format!("failed to parse transaction hash `{value}`"));
        }
    }

    Err(eyre::eyre!(
        "cast send json output missing transaction hash: {combined}"
    ))
}
