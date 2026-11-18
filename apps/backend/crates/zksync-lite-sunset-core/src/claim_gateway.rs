use crate::contracts::{MockZkSyncLiteMigrationState, ZkSyncLiteSunsetClaimDistributorL1};
use crate::world::World;
use alloy::{
    network::EthereumWallet,
    primitives::{Address, B256, Bytes, U256, address},
    providers::{Provider, ProviderBuilder},
    rpc::types::{TransactionInput, TransactionRequest},
    sol,
    sol_types::SolCall,
};
use eyre::{Result, eyre};
use std::collections::HashMap;
use tracing::warn;

/// Canonical Multicall3 deployment, deterministic across all EVM chains.
/// https://www.multicall3.com/
/// Used as a fallback when `execution.multicall3_contract_address` is not set.
pub const MULTICALL3_DEFAULT_ADDRESS: Address =
    address!("cA11bde05977b3631167028862bE2a173976CA11");

sol! {
    function claim(
        uint256 index,
        address token,
        uint256 amount,
        bytes32[] merkleProof
    );

    function claimTo(
        uint256 index,
        address claimant,
        address token,
        uint256 amount,
        bytes32[] merkleProof
    );

    struct Multicall3Call {
        address target;
        bytes callData;
    }

    struct Multicall3Call3 {
        address target;
        bool allowFailure;
        bytes callData;
    }

    struct Multicall3Result {
        bool success;
        bytes returnData;
    }

    function aggregate(Multicall3Call[] calls)
        returns (uint256 blockNumber, bytes[] returnData);

    function aggregate3(Multicall3Call3[] calls)
        returns (Multicall3Result[] returnData);
}

/// Returns the configured Multicall3 address, falling back to the canonical
/// deployment when `execution.multicall3_contract_address` is unset.
pub(crate) fn multicall3_address(world: &World) -> Address {
    world
        .config()
        .execution
        .multicall3_contract_address
        .unwrap_or(MULTICALL3_DEFAULT_ADDRESS)
}

fn encode_multicall_claim_to(
    claim_contract: Address,
    claimant: Address,
    items: &[ClaimGatewayItem],
) -> Bytes {
    let calls = items
        .iter()
        .map(|item| {
            let inner = claimToCall {
                index: U256::from(item.index),
                claimant,
                token: item.token,
                amount: item.amount,
                merkleProof: item.merkle_proof.clone(),
            };
            Multicall3Call {
                target: claim_contract,
                callData: Bytes::copy_from_slice(&inner.abi_encode()),
            }
        })
        .collect::<Vec<_>>();

    let aggregate_call = aggregateCall { calls };
    Bytes::copy_from_slice(&aggregate_call.abi_encode())
}

/// Encodes the calldata a user-signed direct claim must submit. A single-item
/// batch targets the distributor's `claim(...)` entrypoint directly (msg.sender
/// must equal the leaf-bound claimant). Batches with more than one item are
/// routed through Multicall3's `aggregate`, wrapping each leaf in a
/// permissionless `claimTo(claimant, ...)` call so all proceeds are delivered
/// to the claimant regardless of `msg.sender`.
pub(crate) fn encode_direct_claim_transaction(
    world: &World,
    claim_contract: Address,
    claimant: Address,
    items: &[ClaimGatewayItem],
) -> (Address, Bytes) {
    if let [item] = items {
        let call = claimCall {
            index: U256::from(item.index),
            token: item.token,
            amount: item.amount,
            merkleProof: item.merkle_proof.clone(),
        };
        return (claim_contract, Bytes::copy_from_slice(&call.abi_encode()));
    }

    (
        multicall3_address(world),
        encode_multicall_claim_to(claim_contract, claimant, items),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaimGatewayItem {
    pub index: u64,
    pub token: Address,
    pub amount: U256,
    pub merkle_proof: Vec<B256>,
}

impl ClaimGatewayItem {
    pub(crate) fn from_batch_items(items: &[crate::claims::shared::ClaimBatchItem]) -> Vec<Self> {
        items
            .iter()
            .map(|item| Self {
                index: item.index,
                token: item.token_address,
                amount: item.amount,
                merkle_proof: item.merkle_proof.clone(),
            })
            .collect()
    }
}

pub(crate) async fn query_is_claimed(
    world: &World,
    claim_contract: Address,
    index: u64,
) -> Result<bool> {
    let provider = world.l1_provider();
    let instance = ZkSyncLiteSunsetClaimDistributorL1::new(claim_contract, provider);
    instance
        .isClaimed(U256::from(index))
        .call()
        .await
        .map_err(|err| eyre!(err.to_string()))
}

/// Aggregated result of the on-chain checks performed during `prepare_claim`,
/// bundled into a single Multicall3 `aggregate3` eth_call so prepare latency
/// is one RTT regardless of how many tokens/leaves a user has.
pub(crate) struct PreparedProbeResult {
    pub merkle_root: B256,
    pub claim_finalized: HashMap<u64, bool>,
    pub token_migrated: HashMap<Address, bool>,
}

/// Issues a single Multicall3 eth_call bundling the merkle-root read with one
/// `isClaimed` lookup per leaf index and one `isTokenMigrated` lookup per
/// unique token. Per-call failures fall back to "not finalized" / "not
/// migrated" so behavior matches the previous per-row probe loop.
pub(crate) async fn batched_prepare_probes(
    world: &World,
    claim_distributor: Address,
    migration_contract: Address,
    claim_indices: &[u64],
    tokens: &[Address],
) -> Result<PreparedProbeResult> {
    use MockZkSyncLiteMigrationState::isTokenMigratedCall;
    use ZkSyncLiteSunsetClaimDistributorL1::{MERKLE_ROOTCall, isClaimedCall};

    let mut calls = Vec::with_capacity(1 + claim_indices.len() + tokens.len());
    calls.push(Multicall3Call3 {
        target: claim_distributor,
        allowFailure: true,
        callData: Bytes::copy_from_slice(&MERKLE_ROOTCall {}.abi_encode()),
    });
    for &index in claim_indices {
        calls.push(Multicall3Call3 {
            target: claim_distributor,
            allowFailure: true,
            callData: Bytes::copy_from_slice(
                &isClaimedCall {
                    index: U256::from(index),
                }
                .abi_encode(),
            ),
        });
    }
    for &token in tokens {
        calls.push(Multicall3Call3 {
            target: migration_contract,
            allowFailure: true,
            callData: Bytes::copy_from_slice(&isTokenMigratedCall { token }.abi_encode()),
        });
    }

    let aggregate = aggregate3Call { calls };
    let multicall = multicall3_address(world);
    let tx = TransactionRequest::default()
        .to(multicall)
        .input(TransactionInput::new(Bytes::copy_from_slice(
            &aggregate.abi_encode(),
        )));

    let return_bytes = world
        .l1_provider()
        .call(tx)
        .await
        .map_err(|err| eyre!("aggregate3 call failed: {err}"))?;
    let results = aggregate3Call::abi_decode_returns(&return_bytes)
        .map_err(|err| eyre!("aggregate3 decode failed: {err}"))?;

    let expected = 1 + claim_indices.len() + tokens.len();
    if results.len() != expected {
        return Err(eyre!(
            "aggregate3 returned {} entries, expected {expected}",
            results.len()
        ));
    }
    let mut iter = results.into_iter();

    let merkle_root_res = iter.next().expect("length checked above");
    if !merkle_root_res.success {
        return Err(eyre!("MERKLE_ROOT sub-call failed inside aggregate3"));
    }
    let merkle_root = MERKLE_ROOTCall::abi_decode_returns(&merkle_root_res.returnData)
        .map_err(|err| eyre!("MERKLE_ROOT decode failed: {err}"))?;

    let mut claim_finalized = HashMap::with_capacity(claim_indices.len());
    for &index in claim_indices {
        let r = iter.next().expect("length checked above");
        let value = if r.success {
            isClaimedCall::abi_decode_returns(&r.returnData).unwrap_or_else(|err| {
                warn!(
                    component = "claims",
                    claim_index = index,
                    error = %err,
                    "isClaimed response decode failed; treating claim as not finalized"
                );
                false
            })
        } else {
            warn!(
                component = "claims",
                claim_index = index,
                claim_distributor = %claim_distributor,
                "isClaimed sub-call failed inside aggregate3; treating claim as not finalized"
            );
            false
        };
        claim_finalized.insert(index, value);
    }

    let mut token_migrated = HashMap::with_capacity(tokens.len());
    for &token in tokens {
        let r = iter.next().expect("length checked above");
        let value = if r.success {
            isTokenMigratedCall::abi_decode_returns(&r.returnData).unwrap_or_else(|err| {
                warn!(
                    component = "claims",
                    token = %token,
                    error = %err,
                    "isTokenMigrated response decode failed; treating token as not migrated"
                );
                false
            })
        } else {
            warn!(
                component = "claims",
                token = %token,
                migration_contract = %migration_contract,
                "isTokenMigrated sub-call failed inside aggregate3; treating token as not migrated"
            );
            false
        };
        token_migrated.insert(token, value);
    }

    Ok(PreparedProbeResult {
        merkle_root,
        claim_finalized,
        token_migrated,
    })
}

/// Outcome of a worker broadcast attempt.
///
/// Retryable errors are scheduled for another pass with backoff. Terminal
/// errors come from on-chain rejection (revert during gas estimation, bad
/// calldata, sponsor out of funds, etc.) and are pointless to retry — the next
/// attempt would deterministically fail the same way.
#[derive(Debug)]
pub(crate) enum BroadcastError {
    Retryable { detail: String },
    Terminal { detail: String },
}

/// Allocates the next sponsor nonce. Holds the in-process mutex briefly to
/// ensure callers see strictly increasing values, seeding from the pending
/// chain nonce on the first call (or after a reset).
pub(crate) async fn allocate_sponsor_nonce(world: &World) -> Result<u64> {
    let sponsor = world.sponsor_address();
    let mut guard = world.sponsor_nonce_lock().lock().await;
    let nonce = match *guard {
        Some(n) => n,
        None => world
            .l1_provider()
            .get_transaction_count(sponsor)
            .pending()
            .await
            .map_err(|err| eyre!(err.to_string()))?,
    };
    *guard = Some(nonce + 1);
    Ok(nonce)
}

/// Reseeds the cached sponsor nonce from chain on the next allocation.
pub(crate) async fn reset_sponsor_nonce(world: &World) {
    let mut guard = world.sponsor_nonce_lock().lock().await;
    *guard = None;
}

/// Broadcasts a batch of permissionless `claimTo` calls to the distributor via
/// Multicall3's `aggregate`. Every item's funds are routed to `claimant` by the
/// distributor (the leaf binds the destination). `aggregate` reverts the whole
/// transaction if any sub-call reverts — mirroring the atomic batch semantics
/// of the removed `claimManyOnBehalf`.
///
/// The caller owns nonce assignment (typically via [`allocate_sponsor_nonce`])
/// so retries can re-broadcast with the same nonce without skipping a slot.
pub(crate) async fn broadcast_claim_multicall(
    world: &World,
    claim_contract: Address,
    claimant: Address,
    items: &[ClaimGatewayItem],
    nonce: u64,
) -> std::result::Result<B256, BroadcastError> {
    if items.is_empty() {
        return Err(BroadcastError::Terminal {
            detail: "no claim items to submit".to_string(),
        });
    }

    let signer = world.sponsor_signer();
    let sponsor = world.sponsor_address();
    let multicall_address = multicall3_address(world);
    let input_data = encode_multicall_claim_to(claim_contract, claimant, items);
    let tx = TransactionRequest::default()
        .from(sponsor)
        .to(multicall_address)
        .input(TransactionInput::new(input_data))
        .nonce(nonce);

    let wallet = EthereumWallet::from(signer);
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(world.config().execution.rpc_url().clone());

    match provider.send_transaction(tx).await {
        Ok(pending) => Ok(*pending.tx_hash()),
        Err(err) => Err(classify_broadcast_error(err.to_string())),
    }
}

/// Classifies an `eth_sendRawTransaction` error string. Any signal that the
/// chain itself rejected the call (revert during gas estimation, intrinsic gas
/// failures, sponsor out of funds, replay/nonce issues that mean the slot is
/// already burnt) is terminal — retrying would deterministically fail again.
/// Transport/network/timeout errors are treated as retryable.
fn classify_broadcast_error(detail: String) -> BroadcastError {
    let lowered = detail.to_lowercase();
    let terminal_markers = [
        "revert",
        "execution reverted",
        "intrinsic gas",
        "insufficient funds",
        "nonce too low",
        "already known",
        "replacement transaction underpriced",
        "exceeds block gas limit",
        "invalid sender",
        "gas required exceeds allowance",
    ];
    if terminal_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        BroadcastError::Terminal { detail }
    } else {
        BroadcastError::Retryable { detail }
    }
}

#[cfg(test)]
mod tests {
    use super::{BroadcastError, classify_broadcast_error};

    #[test]
    fn revert_strings_are_terminal() {
        assert!(matches!(
            classify_broadcast_error("execution reverted: AlreadyClaimed".into()),
            BroadcastError::Terminal { .. }
        ));
        assert!(matches!(
            classify_broadcast_error("err: insufficient funds for gas".into()),
            BroadcastError::Terminal { .. }
        ));
        assert!(matches!(
            classify_broadcast_error("nonce too low".into()),
            BroadcastError::Terminal { .. }
        ));
    }

    #[test]
    fn transport_errors_are_retryable() {
        assert!(matches!(
            classify_broadcast_error("connection refused".into()),
            BroadcastError::Retryable { .. }
        ));
        assert!(matches!(
            classify_broadcast_error("timeout while reading response".into()),
            BroadcastError::Retryable { .. }
        ));
    }
}
