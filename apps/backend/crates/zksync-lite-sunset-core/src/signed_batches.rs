use alloy::{
    primitives::{Address, B256, U256, keccak256},
    sol,
    sol_types::{SolStruct, SolValue, eip712_domain},
};

pub const CLAIM_EIP712_DOMAIN_NAME: &str = "ZkSyncLiteSunsetClaimDistributor";
pub const EIP712_DOMAIN_VERSION: &str = "1";

sol! {
    struct ClaimBatch {
        address claimant;
        bytes32 claimsHash;
        uint256 expiry;
        uint256 nonce;
    }

    struct ClaimHashItem {
        uint256 index;
        address token;
        uint256 amount;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClaimHashItemInput {
    pub index: u64,
    pub token: Address,
    pub amount: U256,
}

pub(crate) fn compute_claims_hash(items: &[ClaimHashItemInput]) -> B256 {
    let mut packed = Vec::with_capacity(items.len() * 32);
    for item in items {
        let hash_item = ClaimHashItem {
            index: U256::from(item.index),
            token: item.token,
            amount: item.amount,
        };
        let item_hash = keccak256(hash_item.abi_encode());
        packed.extend_from_slice(item_hash.as_slice());
    }

    keccak256(packed)
}

pub fn compute_claim_batch_signing_hash(
    claimant: Address,
    claims_hash: B256,
    expiry: u64,
    nonce: u64,
    chain_id: u64,
    verifying_contract: Address,
) -> B256 {
    let domain = eip712_domain! {
        name: CLAIM_EIP712_DOMAIN_NAME,
        version: EIP712_DOMAIN_VERSION,
        chain_id: chain_id,
        verifying_contract: verifying_contract,
    };
    let claim_batch = ClaimBatch {
        claimant,
        claimsHash: claims_hash,
        expiry: U256::from(expiry),
        nonce: U256::from(nonce),
    };

    claim_batch.eip712_signing_hash(&domain)
}
