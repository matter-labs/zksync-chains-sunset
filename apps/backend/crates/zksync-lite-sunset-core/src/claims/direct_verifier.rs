use crate::claim_gateway::{self, ClaimGatewayItem};
use crate::contracts::ZkSyncLiteSunsetClaimDistributorL1::Claimed;
use crate::world::World;
use alloy::{
    consensus::Transaction,
    network::TransactionResponse,
    primitives::{Address, B256, U256},
    providers::Provider,
    rpc::types::TransactionReceipt,
};
use tracing::debug;

use super::shared::{ClaimError, ClaimRequestPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmittedClaimEvidence {
    OuterTransaction,
    DistributorLogs { matched_logs: usize },
}

impl SubmittedClaimEvidence {
    const fn validation_path(self) -> &'static str {
        match self {
            Self::OuterTransaction => "direct_outer_tx_match",
            Self::DistributorLogs { .. } => "direct_claimed_logs_match",
        }
    }

    const fn matched_logs(self) -> usize {
        match self {
            Self::OuterTransaction => 0,
            Self::DistributorLogs { matched_logs } => matched_logs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OuterTransactionMismatch {
    expected_from: Address,
    actual_from: Address,
    expected_to: Address,
    actual_to: Option<Address>,
    input_matches: bool,
}

impl OuterTransactionMismatch {
    fn detail(&self) -> String {
        let mut details = Vec::new();
        if self.actual_from != self.expected_from {
            details.push(format!(
                "expected sender {:#x}, got {:#x}",
                self.expected_from, self.actual_from
            ));
        }
        if self.actual_to != Some(self.expected_to) {
            details.push(format!(
                "expected destination {:#x}, got {:?}",
                self.expected_to, self.actual_to
            ));
        }
        if !self.input_matches {
            details.push("unexpected calldata".to_string());
        }
        details.join("; ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExpectedClaimedLog {
    index: U256,
    claimant: Address,
    token: Address,
    amount: U256,
}

pub(crate) async fn verify_submitted_claim_transaction(
    world: &World,
    claim_contract: Address,
    tx_hash: B256,
    payload: &ClaimRequestPayload,
    expected_user: Address,
) -> Result<SubmittedClaimEvidence, ClaimError> {
    let tx = world
        .l1_provider()
        .get_transaction_by_hash(tx_hash)
        .await
        .map_err(ClaimError::onchain_rpc)?
        .ok_or(ClaimError::SubmittedTransactionNotFound { tx_hash })?;

    match verify_outer_transaction(world, claim_contract, payload, expected_user, &tx) {
        Ok(evidence) => {
            debug!(
                component = "claims",
                tx_hash = %tx_hash,
                claimant = %expected_user,
                validation_path = evidence.validation_path(),
                "direct claim submission accepted through outer transaction match"
            );
            return Ok(evidence);
        }
        Err(mismatch) => {
            debug!(
                component = "claims",
                tx_hash = %tx_hash,
                claimant = %expected_user,
                outer_transaction_mismatch = %mismatch.detail(),
                "direct outer transaction did not match prepared transaction; checking receipt logs"
            );
        }
    }

    let receipt = world
        .l1_provider()
        .get_transaction_receipt(tx_hash)
        .await
        .map_err(ClaimError::onchain_rpc)?
        .ok_or_else(|| ClaimError::SubmittedTransactionMismatch {
            detail: format!("submitted transaction receipt not found: {tx_hash:#x}"),
        })?;

    let evidence = verify_claimed_logs(claim_contract, &receipt, payload, expected_user)?;
    debug!(
        component = "claims",
        tx_hash = %tx_hash,
        claimant = %expected_user,
        validation_path = evidence.validation_path(),
        matched_logs = evidence.matched_logs(),
        "direct claim submission accepted through distributor Claimed logs"
    );
    Ok(evidence)
}

fn verify_outer_transaction<T>(
    world: &World,
    claim_contract: Address,
    payload: &ClaimRequestPayload,
    expected_user: Address,
    tx: &T,
) -> Result<SubmittedClaimEvidence, OuterTransactionMismatch>
where
    T: TransactionResponse + Transaction,
{
    let gateway_items = ClaimGatewayItem::from_batch_items(&payload.items);
    let (expected_to, expected_input) = claim_gateway::encode_direct_claim_transaction(
        world,
        claim_contract,
        expected_user,
        &gateway_items,
    );
    let actual_from = tx.from();
    let actual_to = tx.to();
    let input_matches = tx.input() == &expected_input;

    if actual_from == expected_user && actual_to == Some(expected_to) && input_matches {
        return Ok(SubmittedClaimEvidence::OuterTransaction);
    }

    Err(OuterTransactionMismatch {
        expected_from: expected_user,
        actual_from,
        expected_to,
        actual_to,
        input_matches,
    })
}

fn verify_claimed_logs(
    claim_contract: Address,
    receipt: &TransactionReceipt,
    payload: &ClaimRequestPayload,
    expected_user: Address,
) -> Result<SubmittedClaimEvidence, ClaimError> {
    if !receipt.status() {
        return Err(ClaimError::SubmittedTransactionMismatch {
            detail: "submitted claim transaction reverted".to_string(),
        });
    }

    let expected = expected_claimed_logs(payload, expected_user);
    let mut matched = vec![false; expected.len()];

    for log in receipt.logs() {
        if log.address() != claim_contract {
            continue;
        }

        let Ok(decoded) = log.log_decode::<Claimed>() else {
            continue;
        };
        let claimed = decoded.inner.data;
        if let Some((idx, _)) = expected.iter().enumerate().find(|(idx, expected_log)| {
            !matched[*idx] && claimed_log_matches(expected_log, &claimed)
        }) {
            matched[idx] = true;
        }
    }

    if matched.iter().all(|value| *value) {
        return Ok(SubmittedClaimEvidence::DistributorLogs {
            matched_logs: matched.len(),
        });
    }

    debug!(
        component = "claims",
        claimant = %expected_user,
        validation_path = "direct_claimed_logs_missing",
        expected_logs = expected.len(),
        matched_logs = matched.iter().filter(|value| **value).count(),
        "direct claim submitted transaction is missing expected Claimed logs"
    );
    Err(ClaimError::SubmittedTransactionMismatch {
        detail: "submitted transaction did not execute every prepared claim item".to_string(),
    })
}

fn expected_claimed_logs(
    payload: &ClaimRequestPayload,
    claimant: Address,
) -> Vec<ExpectedClaimedLog> {
    payload
        .items
        .iter()
        .map(|item| ExpectedClaimedLog {
            index: U256::from(item.index),
            claimant,
            token: item.token_address,
            amount: item.amount,
        })
        .collect()
}

fn claimed_log_matches(expected: &ExpectedClaimedLog, claimed: &Claimed) -> bool {
    claimed.index == expected.index
        && claimed.claimant == expected.claimant
        && claimed.token == expected.token
        && claimed.amount == expected.amount
}
