// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface IZkSyncMigrate {
    function migrateToken(address token) external;
    function isTokenMigrated(address token) external view returns (bool);
}

interface IMulticall3 {
    struct Call {
        address target;
        bytes callData;
    }

    function aggregate(Call[] calldata calls) external payable returns (uint256 blockNumber, bytes[] memory returnData);
}

/// @notice Batch-migrates every token in a CSV by calling `migrateToken(address)`
/// on the zkSync Lite proxy through Multicall3.aggregate, splitting the work
/// across multiple transactions of up to `CHUNK_SIZE` calls each.
///
/// CSV format (matches sepolia/tokens.csv): `id,address,symbol,decimals` with
/// a header row. Tokens that would revert (already migrated, or contract
/// balance == 0) are filtered before the batches are built — Multicall3.aggregate
/// is atomic, so a single revert kills its whole tx.
///
/// Env vars:
///   ZKSYNC_PROXY (default zkSync Lite Sepolia proxy) — zkSync Lite proxy address.
///   TOKENS_CSV   (default `../../sepolia/tokens.csv`) — relative to the
///                foundry project root.
///   MULTICALL3   (default canonical 0xcA11...CA11).
///   CHUNK_SIZE   (default 50) — max migrateToken calls per aggregate tx.
contract MigrateTokens is Script {
    address constant DEFAULT_ZKSYNC_PROXY = 0xaBEA9132b05A70803a4E85094fD0e1800777fBEF;
    address constant DEFAULT_MULTICALL3 = 0xcA11bde05977b3631167028862bE2a173976CA11;
    uint256 constant DEFAULT_CHUNK_SIZE = 50;

    function run() external {
        address zksync = 0xaBEA9132b05A70803a4E85094fD0e1800777fBEF;
        address multicall = vm.envOr("MULTICALL3", DEFAULT_MULTICALL3);
        string memory path = vm.envOr("TOKENS_CSV", string("../../sepolia/tokens.csv"));
        uint256 chunkSize = vm.envOr("CHUNK_SIZE", DEFAULT_CHUNK_SIZE);
        require(chunkSize > 0, "CHUNK_SIZE must be > 0");

        string[] memory lines = _split(vm.readFile(path), 0x0a);

        IMulticall3.Call[] memory buf = new IMulticall3.Call[](lines.length);
        uint256 count = 0;
        uint256 skipped = 0;

        for (uint256 i = 0; i < lines.length; i++) {
            string memory line = _stripCR(lines[i]);
            if (bytes(line).length == 0) continue;

            string[] memory cols = _split(line, 0x2c);
            if (cols.length < 4) continue;
            // skip header
            if (keccak256(bytes(cols[0])) == keccak256("id")) continue;

            address token = vm.parseAddress(cols[1]);
            string memory symbol = cols[2];

            if (IZkSyncMigrate(zksync).isTokenMigrated(token)) {
                console.log("skip already migrated:", symbol);
                skipped++;
                continue;
            }

            uint256 balance = token == address(0) ? zksync.balance : IERC20(token).balanceOf(zksync);
            if (balance == 0) {
                console.log("skip zero balance:", symbol);
                skipped++;
                continue;
            }

            buf[count] = IMulticall3.Call({
                target: zksync, callData: abi.encodeWithSelector(IZkSyncMigrate.migrateToken.selector, token)
            });
            count++;
            console.log("queue", symbol, balance);
        }

        if (count == 0) {
            console.log("nothing to migrate, skipped:", skipped);
            return;
        }

        uint256 chunks = (count + chunkSize - 1) / chunkSize;
        console.log("queued:", count);
        console.log("skipped:", skipped);
        console.log("chunks:", chunks);

        for (uint256 start = 0; start < count; start += chunkSize) {
            uint256 end = start + chunkSize;
            if (end > count) end = count;
            IMulticall3.Call[] memory calls = new IMulticall3.Call[](end - start);
            for (uint256 j = 0; j < calls.length; j++) {
                calls[j] = buf[start + j];
            }
            console.log("aggregate batch", start / chunkSize + 1, "size:", calls.length);
            vm.startBroadcast();
            IMulticall3(multicall).aggregate(calls);
            vm.stopBroadcast();
        }
    }

    function _split(string memory s, bytes1 delimiter) internal pure returns (string[] memory) {
        bytes memory input = bytes(s);
        uint256 parts = 1;
        for (uint256 i = 0; i < input.length; i++) {
            if (input[i] == delimiter) parts++;
        }

        string[] memory out = new string[](parts);
        uint256 start = 0;
        uint256 part = 0;
        for (uint256 i = 0; i < input.length; i++) {
            if (input[i] != delimiter) continue;
            out[part] = _slice(input, start, i);
            part++;
            start = i + 1;
        }
        out[part] = _slice(input, start, input.length);
        return out;
    }

    function _slice(bytes memory input, uint256 start, uint256 end) internal pure returns (string memory) {
        bytes memory out = new bytes(end - start);
        for (uint256 i = 0; i < out.length; i++) {
            out[i] = input[start + i];
        }
        return string(out);
    }

    /// @dev Trailing `\r` shows up when CSVs are saved with CRLF line endings.
    function _stripCR(string memory s) internal pure returns (string memory) {
        bytes memory b = bytes(s);
        if (b.length == 0 || b[b.length - 1] != 0x0d) return s;
        bytes memory out = new bytes(b.length - 1);
        for (uint256 i = 0; i < out.length; i++) {
            out[i] = b[i];
        }
        return string(out);
    }
}
