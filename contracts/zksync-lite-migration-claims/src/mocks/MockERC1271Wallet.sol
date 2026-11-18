// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC1271WalletMock} from "@openzeppelin/contracts/mocks/ERC1271WalletMock.sol";

contract MockERC1271Wallet is ERC1271WalletMock {
    error NotOwner();
    error CallFailed();

    constructor(address owner_) ERC1271WalletMock(owner_) {}

    receive() external payable {}

    function execute(address target, bytes calldata data) external payable returns (bytes memory) {
        if (msg.sender != owner()) revert NotOwner();

        (bool success, bytes memory result) = target.call{value: msg.value}(data);
        if (!success) revert CallFailed();
        return result;
    }
}
