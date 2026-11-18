// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {Multicall3} from "../src/mocks/Multicall3.sol";

contract Multicall3Target {
    function value() external pure returns (uint256) {
        return 42;
    }

    function fail() external pure {
        revert("target failed");
    }
}

contract Multicall3Test is Test {
    Multicall3 internal multicall;
    Multicall3Target internal target;

    function setUp() public {
        multicall = new Multicall3();
        target = new Multicall3Target();
    }

    function test_aggregate3_returns_success_and_allowed_failure_results() public {
        Multicall3.Call3[] memory calls = new Multicall3.Call3[](2);
        calls[0] = Multicall3.Call3({
            target: address(target), allowFailure: false, callData: abi.encodeCall(Multicall3Target.value, ())
        });
        calls[1] = Multicall3.Call3({
            target: address(target), allowFailure: true, callData: abi.encodeCall(Multicall3Target.fail, ())
        });

        Multicall3.Result[] memory results = multicall.aggregate3(calls);

        assertEq(results.length, 2);
        assertTrue(results[0].success);
        assertEq(abi.decode(results[0].returnData, (uint256)), 42);
        assertFalse(results[1].success);
    }

    function test_aggregate3_reverts_failed_required_call() public {
        Multicall3.Call3[] memory calls = new Multicall3.Call3[](1);
        calls[0] = Multicall3.Call3({
            target: address(target), allowFailure: false, callData: abi.encodeCall(Multicall3Target.fail, ())
        });

        vm.expectRevert("Multicall3: call failed");
        multicall.aggregate3(calls);
    }
}
