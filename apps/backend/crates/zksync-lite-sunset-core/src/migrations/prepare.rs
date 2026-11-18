use crate::migration_gateway;
use crate::world::World;
use alloy::primitives::{Address, Bytes};
use serde::{Deserialize, Serialize};

use super::shared::{
    MigrationError, MigrationId, MigrationItem, MigrationRequestPayload, MigrationStatus,
    migration_contract, serialize_payload,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPrepareRequest {
    pub address: Address,
    pub token_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPrepareResponse {
    pub migration_id: MigrationId,
    pub address: Address,
    pub chain_id: u64,
    pub contract_address: Address,
    pub item: MigrationItem,
    pub call_data: Bytes,
}

pub async fn prepare_migration(
    world: &World,
    req: MigrationPrepareRequest,
) -> Result<MigrationPrepareResponse, MigrationError> {
    let contract = migration_contract(world)?;

    if migration_gateway::query_is_token_migrated(world, contract, req.token_address)
        .await
        .map_err(MigrationError::onchain_rpc)?
    {
        return Err(MigrationError::AlreadyMigrated {
            token_address: req.token_address,
        });
    }

    let item = MigrationItem {
        token_address: req.token_address,
    };
    let migration_id = persist_prepared_request(world, req.address, &item).await?;

    Ok(MigrationPrepareResponse {
        migration_id,
        address: req.address,
        chain_id: world.config().auth.migrate_chain_id,
        contract_address: contract,
        call_data: migration_gateway::encode_migrate_token_calldata(item.token_address),
        item,
    })
}

async fn persist_prepared_request(
    world: &World,
    user: Address,
    item: &MigrationItem,
) -> Result<MigrationId, MigrationError> {
    let sequence = sqlx::query_scalar::<_, i64>("SELECT nextval('migration_request_id_seq')")
        .fetch_one(world.db_pool())
        .await
        .map_err(MigrationError::storage)?;
    let migration_id = MigrationId::from_sequence(sequence as u64);

    let payload = MigrationRequestPayload { item: item.clone() };

    sqlx::query(
        r#"
        INSERT INTO migration_requests (
            migration_id,
            user_address,
            status,
            token_address,
            payload_json,
            prepared_at,
            created_at,
            updated_at
        )
        VALUES ($1, $2, $3, $4, $5::jsonb, NOW(), NOW(), NOW())
        "#,
    )
    .bind(migration_id.as_str())
    .bind(format!("{user:#x}"))
    .bind(MigrationStatus::Prepared.as_db())
    .bind(format!("{:#x}", item.token_address))
    .bind(serialize_payload(&payload)?)
    .execute(world.db_pool())
    .await
    .map_err(MigrationError::storage)?;

    Ok(migration_id)
}
