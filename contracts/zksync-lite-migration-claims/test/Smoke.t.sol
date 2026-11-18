// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {TransparentUpgradeableProxy} from "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import {Test} from "forge-std/Test.sol";
import {ZkSyncLiteSunsetClaimDistributorL1} from "../src/ZkSyncLiteSunsetClaimDistributorL1.sol";

contract ERC20Mock is ERC20 {
    constructor() ERC20("Mock", "MOCK") {}

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract SmokeTest is Test {
    receive() external payable {}

    function _deployDistributor(bytes32 root, uint256 windowStart)
        internal
        returns (ZkSyncLiteSunsetClaimDistributorL1)
    {
        ZkSyncLiteSunsetClaimDistributorL1 implementation = new ZkSyncLiteSunsetClaimDistributorL1();
        bytes memory initData =
            abi.encodeWithSignature("initialize(bytes32,uint256,address)", root, windowStart, address(this));
        TransparentUpgradeableProxy proxy =
            new TransparentUpgradeableProxy(address(implementation), address(0xBEEF), initData);
        return ZkSyncLiteSunsetClaimDistributorL1(payable(address(proxy)));
    }

    function test_distributor_claim_single_leaf() public {
        ERC20Mock token = new ERC20Mock();
        uint256 index = 0;
        uint256 amount = 123;
        bytes32 root = keccak256(abi.encodePacked(index, address(this), address(token), amount));

        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp - 1);
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        distributor.claim(index, address(token), amount, proof);

        assertTrue(distributor.isClaimed(index));
        assertEq(token.balanceOf(address(this)), amount);
    }

    function test_distributor_claim_single_leaf_eth() public {
        uint256 index = 1;
        uint256 amount = 123;
        bytes32 root = keccak256(abi.encodePacked(index, address(this), address(0), amount));

        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp - 1);

        (bool funded,) = address(distributor).call{value: amount}("");
        assertTrue(funded);

        uint256 balanceBefore = address(this).balance;
        bytes32[] memory proof = new bytes32[](0);
        distributor.claim(index, address(0), amount, proof);

        assertTrue(distributor.isClaimed(index));
        assertEq(address(this).balance, balanceBefore + amount);
    }

    function test_distributor_claim_to_permissionless() public {
        ERC20Mock token = new ERC20Mock();
        address claimant = makeAddr("smoke-claimant");
        address relayer = makeAddr("smoke-relayer");
        uint256 index = 7;
        uint256 amount = 789;
        bytes32 root = keccak256(abi.encodePacked(index, claimant, address(token), amount));

        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp - 1);
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        vm.prank(relayer);
        distributor.claimTo(index, claimant, address(token), amount, proof);

        assertTrue(distributor.isClaimed(index));
        assertEq(token.balanceOf(claimant), amount);
    }
}
