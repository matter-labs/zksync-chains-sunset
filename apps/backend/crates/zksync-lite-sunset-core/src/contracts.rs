use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    MockZkSyncLiteMigrationState,
    "../../../../contracts/zksync-lite-migration-claims/out/MockZkSyncLiteMigrationState.sol/MockZkSyncLiteMigrationState.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    ZkSyncLiteSunsetClaimDistributorL1,
    "../../../../contracts/zksync-lite-migration-claims/out/ZkSyncLiteSunsetClaimDistributorL1.sol/ZkSyncLiteSunsetClaimDistributorL1.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IERC1271,
    "../../../../contracts/zksync-lite-migration-claims/out/IERC1271.sol/IERC1271.json"
);
