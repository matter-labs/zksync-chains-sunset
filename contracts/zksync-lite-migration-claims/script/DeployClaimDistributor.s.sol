// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {TransparentUpgradeableProxy} from "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import {ZkSyncLiteSunsetClaimDistributorL1} from "../src/ZkSyncLiteSunsetClaimDistributorL1.sol";

/// @notice Deploys the `ZkSyncLiteSunsetClaimDistributorL1` implementation behind a
/// `TransparentUpgradeableProxy` and initializes it with the provided Merkle root,
/// claim window start, and initial owner.
///
/// Env vars:
///   MERKLE_ROOT    (required) — Merkle root over claim leaves.
///   WINDOW_START   (required) — Unix timestamp (inclusive) at which claims open.
///   INITIAL_OWNER  (required) — Address that can pause/unpause the distributor.
///   PROXY_ADMIN    (required) — Address allowed to upgrade the proxy. MUST differ
///                               from INITIAL_OWNER and any expected caller of the
///                               implementation, since the transparent proxy admin
///                               cannot call through to the implementation.
contract DeployClaimDistributor is Script {
    function run() external returns (address proxy, address implementation) {
        bytes32 merkleRoot = 0x2540bc01dc5786ef3e773440ecb39785b341e8fccf0ff3537c970b497ad45cd5;
        uint256 windowStart = 1;
        address initialOwner = 0x6A541B199F0505413970ff014FE5e50AdC93f457;
        address proxyAdmin = 0x9C5819D1F9391B9818F5BD6D701755fBF8d013AC;

        require(merkleRoot != bytes32(0), "MERKLE_ROOT must be non-zero");
        require(initialOwner != address(0), "INITIAL_OWNER must be non-zero");
        require(proxyAdmin != address(0), "PROXY_ADMIN must be non-zero");
        require(proxyAdmin != initialOwner, "PROXY_ADMIN must differ from INITIAL_OWNER");

        bytes memory initData =
            abi.encodeWithSignature("initialize(bytes32,uint256,address)", merkleRoot, windowStart, initialOwner);

        vm.startBroadcast();
        ZkSyncLiteSunsetClaimDistributorL1 impl = new ZkSyncLiteSunsetClaimDistributorL1();
        TransparentUpgradeableProxy proxyContract = new TransparentUpgradeableProxy(address(impl), proxyAdmin, initData);
        vm.stopBroadcast();

        implementation = address(impl);
        proxy = address(proxyContract);

        console.log("implementation:", implementation);
        console.log("proxy:", proxy);
        console.log("proxy admin:", proxyAdmin);
        console.log("initial owner:", initialOwner);
        console.log("window start:", windowStart);
        console.logBytes32(merkleRoot);
    }
}
