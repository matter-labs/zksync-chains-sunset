use crate::eligibility::serde_u256_dec_str;
use crate::types::{Address, U256};
use alloy::{
    primitives::{B256, Signature},
    sol,
    sol_types::{SolStruct, eip712_domain},
};
use serde::{Deserialize, Serialize};

const EIP712_DOMAIN_NAME: &str = "zkSync Lite Sunset";
const EIP712_DOMAIN_VERSION: &str = "1";

sol! {
    struct ActionAuthorization {
        address userAddress;
        address tokenAddress;
        string action;
        string rawAmount;
        uint256 chainId;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Claim,
    Migrate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedActionPayload {
    pub chain_id: u64,
    #[serde(with = "serde_u256_dec_str")]
    pub raw_amount: U256,
}

pub struct SignedActionRequest<'a> {
    pub address: Address,
    pub token_address: Address,
    pub action: ActionKind,
    pub payload: &'a SignedActionPayload,
    pub signature: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSignedActionRequest<'a> {
    pub address: Address,
    pub token_address: Address,
    pub action: ActionKind,
    pub payload: &'a SignedActionPayload,
    pub recovered_signer: Address,
}

pub enum SignatureValidationError {
    InvalidSignature,
    UnsupportedSigner {
        expected: Address,
        recovered: Address,
    },
}

pub fn parse_verified_signed_action_request(
    req: SignedActionRequest<'_>,
) -> Result<VerifiedSignedActionRequest<'_>, SignatureValidationError> {
    let signature: Signature = req
        .signature
        .parse()
        .map_err(|_| SignatureValidationError::InvalidSignature)?;
    let digest = action_authorization_digest(&req);
    let recovered = signature
        .recover_address_from_prehash(&digest)
        .map_err(|_| SignatureValidationError::InvalidSignature)?;

    // For now we fail hard unless the recovered EOA exactly matches the request address.
    // Contract/account-abstraction signer validation (e.g. ERC-1271 via on-chain checks)
    // will be added later when we replace this temporary auth flow.
    if recovered != req.address {
        return Err(SignatureValidationError::UnsupportedSigner {
            expected: req.address,
            recovered,
        });
    }

    Ok(VerifiedSignedActionRequest {
        address: req.address,
        token_address: req.token_address,
        action: req.action,
        payload: req.payload,
        recovered_signer: recovered,
    })
}

fn action_authorization_digest(req: &SignedActionRequest<'_>) -> B256 {
    let domain = eip712_domain! {
        name: EIP712_DOMAIN_NAME,
        version: EIP712_DOMAIN_VERSION,
        chain_id: req.payload.chain_id,
    };
    let action_authorization = ActionAuthorization {
        userAddress: req.address,
        tokenAddress: req.token_address,
        action: action_label(req.action).to_string(),
        rawAmount: req.payload.raw_amount.to_string(),
        chainId: U256::from(req.payload.chain_id),
    };

    action_authorization.eip712_signing_hash(&domain)
}

const fn action_label(action: ActionKind) -> &'static str {
    match action {
        ActionKind::Claim => "claim",
        ActionKind::Migrate => "migrate",
    }
}
