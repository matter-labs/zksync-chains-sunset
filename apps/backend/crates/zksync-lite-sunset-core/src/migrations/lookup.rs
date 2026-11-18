use crate::world::World;
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

use super::shared::MigrationError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationTokensResponse {
    pub tokens: Vec<MigrationToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationToken {
    pub token_address: Address,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SnapshotTokenDbRow {
    token_address: String,
    symbol: String,
    decimals: i16,
}

pub async fn list_migration_tokens(
    world: &World,
) -> Result<MigrationTokensResponse, MigrationError> {
    let rows = sqlx::query_as::<_, SnapshotTokenDbRow>(
        r#"
        SELECT
            LOWER(st.address) AS token_address,
            st.symbol,
            st.decimals
        FROM snapshot_tokens st
        WHERE st.id < 1000
        ORDER BY LOWER(st.address) ASC
        "#,
    )
    .fetch_all(world.db_pool())
    .await
    .map_err(MigrationError::storage)?;

    let mut tokens = Vec::with_capacity(rows.len());
    for row in rows {
        let token_address = row
            .token_address
            .parse()
            .map_err(|_| MigrationError::storage("invalid token_address in snapshot_tokens"))?;
        let decimals = u8::try_from(row.decimals)
            .map_err(|_| MigrationError::storage("invalid decimals in snapshot_tokens"))?;

        tokens.push(MigrationToken {
            token_address,
            symbol: row.symbol,
            decimals,
        });
    }

    Ok(MigrationTokensResponse { tokens })
}
