use crate::contracts::MockZkSyncLiteMigrationState;
use crate::world::World;
use alloy::{
    primitives::{Address, Bytes},
    sol_types::SolCall,
};
use eyre::{Result, eyre};

pub(crate) fn encode_migrate_token_calldata(token: Address) -> Bytes {
    let call = MockZkSyncLiteMigrationState::migrateTokenCall { token };
    Bytes::copy_from_slice(&call.abi_encode())
}

pub(crate) async fn query_l1_claim_distributor(
    world: &World,
    migration_contract: Address,
) -> Result<Address> {
    let provider = world.l1_provider();
    let instance = MockZkSyncLiteMigrationState::new(migration_contract, provider);
    instance
        .l1ClaimDistributor()
        .call()
        .await
        .map_err(|err| eyre!(err.to_string()))
}

pub(crate) async fn query_is_token_migrated(
    world: &World,
    migration_contract: Address,
    token: Address,
) -> Result<bool> {
    let provider = world.l1_provider();
    let instance = MockZkSyncLiteMigrationState::new(migration_contract, provider);
    instance
        .isTokenMigrated(token)
        .call()
        .await
        .map_err(|err| eyre!(err.to_string()))
}
