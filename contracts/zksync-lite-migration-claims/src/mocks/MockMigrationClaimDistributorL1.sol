// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

contract MockMigrationClaimDistributorL1 {
    bytes32 public MERKLE_ROOT;

    constructor(bytes32 merkleRoot) {
        MERKLE_ROOT = merkleRoot;
    }

    receive() external payable {}

    function tokenBalance(address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}
