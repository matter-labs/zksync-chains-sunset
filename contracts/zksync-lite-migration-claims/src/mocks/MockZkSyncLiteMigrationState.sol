// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {MerkleProof} from "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";

interface IL1ClaimDistributor {
    function MERKLE_ROOT() external view returns (bytes32);
}

/// @notice Dev-only migration mock matching the current L1-only migration ABI.
contract MockZkSyncLiteMigrationState is Ownable {
    struct TokenMigration {
        uint256 index;
        address token;
        uint256 total;
        bytes32[] merkleProof;
    }

    bytes32 public TOKEN_MIGRATION_ROOT;
    address public l1ClaimDistributor;
    bytes32 public claimRoot;

    mapping(uint256 wordIndex => uint256 claimedWord) internal tokenMigrationExecutedBitMap;
    mapping(address token => bool migrated) internal migratedTokensByAddress;

    event TokenMigrationRootSet(bytes32 tokenMigrationRoot);
    event L1ClaimDistributorSet(address indexed claimDistributor);
    event ClaimRootSet(bytes32 claimRoot);
    event TokenMigrationExecuted(address indexed token);

    constructor() Ownable() {}

    receive() external payable {}

    function setTokenMigrationRoot(bytes32 tokenMigrationRoot) external onlyOwner {
        TOKEN_MIGRATION_ROOT = tokenMigrationRoot;
        emit TokenMigrationRootSet(tokenMigrationRoot);
    }

    function setL1ClaimDistributor(address claimDistributor) external onlyOwner {
        l1ClaimDistributor = claimDistributor;
        emit L1ClaimDistributorSet(claimDistributor);
    }

    function setClaimRoot(bytes32 newClaimRoot) external onlyOwner {
        claimRoot = newClaimRoot;
        emit ClaimRootSet(newClaimRoot);
    }

    function setTokenMigrated(address token, bool migrated) external onlyOwner {
        migratedTokensByAddress[token] = migrated;
    }

    function isTokenMigrated(address token) external view returns (bool) {
        return migratedTokensByAddress[token];
    }

    function migrateToken(address token) external {
        require(l1ClaimDistributor != address(0), "tm1");
        require(!migratedTokensByAddress[token], "tm3");

        _processMigration(token);
        migratedTokensByAddress[token] = true;

        emit TokenMigrationExecuted(token);
    }

    function _processMigration(address token) internal {
        if (token == address(0)) {
            uint256 total = address(this).balance;
            require(total > 0, "tm5");
            (bool success,) = payable(l1ClaimDistributor).call{value: total}("");
            require(success, "tm9");
            return;
        }

        IERC20 erc20 = IERC20(token);
        uint256 balanceBefore = erc20.balanceOf(address(this));
        require(balanceBefore > 0, "tm5");
        erc20.transfer(l1ClaimDistributor, balanceBefore);
        uint256 balanceAfter = erc20.balanceOf(address(this));
        require(balanceBefore - balanceAfter == balanceBefore, "tm9");
    }
}
