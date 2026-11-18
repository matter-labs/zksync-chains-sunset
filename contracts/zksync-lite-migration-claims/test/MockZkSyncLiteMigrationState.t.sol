// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {TestERC20} from "../src/mocks/TestERC20.sol";
import {MockMigrationClaimDistributorL1} from "../src/mocks/MockMigrationClaimDistributorL1.sol";
import {MockZkSyncLiteMigrationState} from "../src/mocks/MockZkSyncLiteMigrationState.sol";

contract MockZkSyncLiteMigrationStateTest is Test {
    bytes32 internal constant CLAIM_ROOT = keccak256("claim-root");

    MockZkSyncLiteMigrationState internal migrationState;
    MockMigrationClaimDistributorL1 internal l1ClaimDistributor;
    TestERC20 internal token;

    function setUp() public {
        migrationState = new MockZkSyncLiteMigrationState();
        l1ClaimDistributor = new MockMigrationClaimDistributorL1(CLAIM_ROOT);
        token = new TestERC20();

        migrationState.setL1ClaimDistributor(address(l1ClaimDistributor));
        migrationState.setClaimRoot(CLAIM_ROOT);
    }

    function test_migrateToken_transfers_eth_and_erc20_balances_to_l1_claim_distributor() public {
        uint256 ethTotal = 1.5 ether;
        uint256 erc20Total = 7 ether;

        vm.deal(address(migrationState), ethTotal);
        token.mint(address(migrationState), erc20Total);

        migrationState.migrateToken(address(0));
        migrationState.migrateToken(address(token));

        assertTrue(migrationState.isTokenMigrated(address(0)));
        assertTrue(migrationState.isTokenMigrated(address(token)));
        assertEq(address(l1ClaimDistributor).balance, ethTotal);
        assertEq(token.balanceOf(address(l1ClaimDistributor)), erc20Total);
        assertEq(address(migrationState).balance, 0);
        assertEq(token.balanceOf(address(migrationState)), 0);
    }

    function test_migrateToken_reverts_when_l1_claim_distributor_missing() public {
        MockZkSyncLiteMigrationState localState = new MockZkSyncLiteMigrationState();

        token.mint(address(localState), 2 ether);

        vm.expectRevert(abi.encodeWithSignature("Error(string)", "tm1"));
        localState.migrateToken(address(token));
    }

    function test_migrateToken_reverts_when_token_already_migrated() public {
        token.mint(address(migrationState), 2 ether);
        migrationState.migrateToken(address(token));

        token.mint(address(migrationState), 3 ether);
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "tm3"));
        migrationState.migrateToken(address(token));
    }

    function test_migrateToken_reverts_when_native_balance_zero() public {
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "tm5"));
        migrationState.migrateToken(address(0));
    }

    function test_migrateToken_reverts_when_erc20_balance_zero() public {
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "tm5"));
        migrationState.migrateToken(address(token));
    }
}
