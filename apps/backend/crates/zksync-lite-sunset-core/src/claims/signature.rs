use crate::contracts::IERC1271;
use crate::signed_batches::compute_claim_batch_signing_hash;
use crate::world::World;
use alloy::{
    contract::Error as ContractError,
    primitives::{Address, B256, Bytes, FixedBytes, Signature, hex},
    providers::Provider,
};
use tracing::debug;

use super::shared::ClaimError;

const ERC1271_MAGIC_VALUE: FixedBytes<4> = FixedBytes::new([0x16, 0x26, 0xba, 0x7e]);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawClaimSignature(Bytes);

impl RawClaimSignature {
    fn decode_hex(signature: &str) -> Result<Self, ClaimError> {
        let value = signature
            .strip_prefix("0x")
            .or_else(|| signature.strip_prefix("0X"))
            .unwrap_or(signature);
        let bytes = hex::decode(value).map_err(|_| ClaimError::InvalidSignature)?;
        Ok(Self(Bytes::from(bytes)))
    }

    fn to_erc1271_bytes(&self) -> Bytes {
        self.0.clone()
    }

    fn as_eoa_signature(&self) -> Option<Signature> {
        Signature::try_from(self.0.as_ref()).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignatureVerificationMethod {
    Eoa,
    Erc1271,
}

impl SignatureVerificationMethod {
    const fn validation_path(self) -> &'static str {
        match self {
            Self::Eoa => "eoa_recovered",
            Self::Erc1271 => "erc1271_valid",
        }
    }
}

pub(crate) struct ClaimBatchSignatureVerification<'a> {
    pub world: &'a World,
    pub claimant: Address,
    pub claims_hash: B256,
    pub expiry: u64,
    pub nonce: u64,
    pub chain_id: u64,
    pub verifying_contract: Address,
    pub signature: &'a str,
}

pub(crate) async fn verify_claim_batch_signature(
    req: ClaimBatchSignatureVerification<'_>,
) -> Result<SignatureVerificationMethod, ClaimError> {
    let digest = compute_claim_batch_signing_hash(
        req.claimant,
        req.claims_hash,
        req.expiry,
        req.nonce,
        req.chain_id,
        req.verifying_contract,
    );

    let signature = RawClaimSignature::decode_hex(req.signature)?;

    if eoa_signature_matches(req.claimant, digest, &signature) {
        let method = SignatureVerificationMethod::Eoa;
        debug!(
            component = "claims",
            claimant = %req.claimant,
            validation_path = method.validation_path(),
            "claim signature accepted through EOA recovery"
        );
        return Ok(method);
    }

    if !address_has_code(req.world, req.claimant).await? {
        debug!(
            component = "claims",
            claimant = %req.claimant,
            validation_path = "invalid_eoa_signature",
            "claim signature rejected for EOA claimant"
        );
        return Err(ClaimError::InvalidSignature);
    }

    verify_erc1271_signature(req.world, req.claimant, digest, &signature).await
}

fn eoa_signature_matches(claimant: Address, digest: B256, signature: &RawClaimSignature) -> bool {
    let Some(signature) = signature.as_eoa_signature() else {
        return false;
    };
    let Ok(recovered) = signature.recover_address_from_prehash(&digest) else {
        return false;
    };
    recovered == claimant
}

async fn address_has_code(world: &World, address: Address) -> Result<bool, ClaimError> {
    let code = world
        .l1_provider()
        .get_code_at(address)
        .await
        .map_err(ClaimError::onchain_rpc)?;
    Ok(!code.is_empty())
}

async fn verify_erc1271_signature(
    world: &World,
    claimant: Address,
    digest: B256,
    signature: &RawClaimSignature,
) -> Result<SignatureVerificationMethod, ClaimError> {
    let account = IERC1271::new(claimant, world.l1_provider());
    let magic = account
        .isValidSignature(digest, signature.to_erc1271_bytes())
        .call()
        .await
        .map_err(map_erc1271_call_error)?;

    if magic == ERC1271_MAGIC_VALUE {
        let method = SignatureVerificationMethod::Erc1271;
        debug!(
            component = "claims",
            claimant = %claimant,
            validation_path = method.validation_path(),
            "claim signature accepted through ERC-1271"
        );
        return Ok(method);
    }

    debug!(
        component = "claims",
        claimant = %claimant,
        validation_path = "erc1271_invalid",
        "claim signature rejected by ERC-1271"
    );
    Err(ClaimError::InvalidSignature)
}

fn map_erc1271_call_error(err: ContractError) -> ClaimError {
    match &err {
        ContractError::AbiError(_) | ContractError::ZeroData(..) => ClaimError::InvalidSignature,
        ContractError::TransportError(err) if is_contract_signature_rejection(&err.to_string()) => {
            ClaimError::InvalidSignature
        }
        _ => ClaimError::onchain_rpc(err),
    }
}

fn is_contract_signature_rejection(detail: &str) -> bool {
    let lowered = detail.to_lowercase();
    lowered.contains("revert") || lowered.contains("execution reverted")
}

#[cfg(test)]
mod tests {
    use super::{ClaimError, ERC1271_MAGIC_VALUE, map_erc1271_call_error};
    use crate::contracts::IERC1271::isValidSignatureCall;
    use alloy::sol_types::SolCall;

    #[test]
    fn erc1271_magic_value_accepts_strict_abi_return() {
        let encoded = isValidSignatureCall::abi_encode_returns(&ERC1271_MAGIC_VALUE);

        let magic = isValidSignatureCall::abi_decode_returns_validate(&encoded)
            .expect("valid magic return");

        assert_eq!(magic, ERC1271_MAGIC_VALUE);
    }

    #[test]
    fn erc1271_decode_error_maps_to_invalid_signature() {
        let mut encoded = [0u8; 32];
        encoded[..4].copy_from_slice(ERC1271_MAGIC_VALUE.as_slice());
        encoded[4] = 1;

        let err = isValidSignatureCall::abi_decode_returns_validate(&encoded)
            .expect_err("dirty padding must fail ABI decoding");

        assert!(matches!(
            map_erc1271_call_error(err.into()),
            ClaimError::InvalidSignature
        ));
    }
}
