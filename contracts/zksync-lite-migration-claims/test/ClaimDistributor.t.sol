// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {TransparentUpgradeableProxy} from "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import {Test} from "forge-std/Test.sol";
import {ZkSyncLiteSunsetClaimDistributorL1} from "../src/ZkSyncLiteSunsetClaimDistributorL1.sol";

contract ClaimTokenMock is ERC20 {
    constructor() ERC20("ClaimToken", "CLM") {}

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract ProxyAdminCaller {
    function callClaim(
        ZkSyncLiteSunsetClaimDistributorL1 distributor,
        uint256 index,
        address token,
        uint256 amount,
        bytes32[] calldata proof
    ) external returns (bool, bytes memory) {
        return address(distributor)
            .call(abi.encodeCall(ZkSyncLiteSunsetClaimDistributorL1.claim, (index, token, amount, proof)));
    }
}

contract ClaimDistributorTest is Test {
    bytes4 internal constant ERROR_STRING_SELECTOR = bytes4(keccak256("Error(string)"));

    receive() external payable {}

    function test_initialize_once_proxy_reverts() public {
        bytes32 root = keccak256("root");
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));

        (bool success, bytes memory data) = address(distributor)
            .call(abi.encodeWithSignature("initialize(bytes32,uint256,address)", root, block.timestamp, address(this)));
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
    }

    function test_initialize_on_implementation_reverts() public {
        ZkSyncLiteSunsetClaimDistributorL1 implementation = new ZkSyncLiteSunsetClaimDistributorL1();
        (bool success, bytes memory data) = address(implementation)
            .call(
                abi.encodeWithSignature(
                    "initialize(bytes32,uint256,address)", bytes32(uint256(1)), block.timestamp, address(this)
                )
            );
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
    }

    function test_initialize_with_explicit_owner_sets_owner() public {
        bytes32 root = keccak256("root");
        address ownerAddress = makeAddr("claim-owner");
        ZkSyncLiteSunsetClaimDistributorL1 distributor =
            _deployDistributorWithOwner(root, block.timestamp, address(0xBEEF), ownerAddress);

        assertEq(distributor.owner(), ownerAddress);
        assertFalse(distributor.paused());
    }

    function test_initialize_with_zero_owner_reverts() public {
        ZkSyncLiteSunsetClaimDistributorL1 implementation = new ZkSyncLiteSunsetClaimDistributorL1();
        bytes memory initData = abi.encodeWithSignature(
            "initialize(bytes32,uint256,address)", keccak256("root"), block.timestamp, address(0)
        );

        (bool success, bytes memory data) = address(implementation).call(initData);
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
    }

    function test_claim_before_window_start_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp + 1, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(
            success,
            data,
            ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__ClaimWindowNotOpen.selector
        );
    }

    function test_claim_at_or_after_window_start_succeeds() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        distributor.claim(index, address(token), amount, proof);

        assertTrue(distributor.isClaimed(index));
        assertEq(token.balanceOf(address(this)), amount);
    }

    function test_claim_to_permissionless_sends_funds_to_leaf_claimant() public {
        ClaimTokenMock token = new ClaimTokenMock();
        address claimant = makeAddr("claimant");
        address relayer = makeAddr("relayer");
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, claimant, address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        vm.prank(relayer);
        distributor.claimTo(index, claimant, address(token), amount, proof);

        assertTrue(distributor.isClaimed(index));
        assertEq(token.balanceOf(claimant), amount);
        assertEq(token.balanceOf(relayer), 0);
    }

    function test_claim_to_wrong_claimant_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        address claimant = makeAddr("claimant");
        address attacker = makeAddr("attacker");
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, claimant, address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        vm.prank(attacker);
        (bool success, bytes memory data) = address(distributor)
            .call(
                abi.encodeCall(
                    ZkSyncLiteSunsetClaimDistributorL1.claimTo, (index, attacker, address(token), amount, proof)
                )
            );
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__InvalidProof.selector
        );
    }

    function test_claim_reverts_when_paused() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        distributor.pause();

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
        assertFalse(distributor.isClaimed(index));
    }

    function test_claim_to_reverts_when_paused() public {
        ClaimTokenMock token = new ClaimTokenMock();
        address claimant = makeAddr("claimant");
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, claimant, address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        distributor.pause();

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = address(distributor)
            .call(
                abi.encodeCall(
                    ZkSyncLiteSunsetClaimDistributorL1.claimTo, (index, claimant, address(token), amount, proof)
                )
            );
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
        assertFalse(distributor.isClaimed(index));
    }

    function test_only_owner_can_pause_and_unpause() public {
        bytes32 root = keccak256("root");
        address ownerAddress = makeAddr("claim-owner");
        address attacker = makeAddr("attacker");
        ZkSyncLiteSunsetClaimDistributorL1 distributor =
            _deployDistributorWithOwner(root, block.timestamp, address(0xBEEF), ownerAddress);

        vm.prank(attacker);
        vm.expectRevert(bytes("Ownable: caller is not the owner"));
        distributor.pause();

        vm.prank(ownerAddress);
        distributor.pause();
        assertTrue(distributor.paused());

        vm.prank(attacker);
        vm.expectRevert(bytes("Ownable: caller is not the owner"));
        distributor.unpause();

        vm.prank(ownerAddress);
        distributor.unpause();
        assertFalse(distributor.paused());
    }

    function test_unpause_restores_claims() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        distributor.pause();
        distributor.unpause();

        bytes32[] memory proof = new bytes32[](0);
        distributor.claim(index, address(token), amount, proof);
        assertTrue(distributor.isClaimed(index));
        assertEq(token.balanceOf(address(this)), amount);
    }

    function test_claim_twice_same_index_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        distributor.claim(index, address(token), amount, proof);

        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__AlreadyClaimed.selector
        );
        assertTrue(distributor.isClaimed(index));
    }

    function test_invalid_proof_wrong_index_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 validIndex = 0;
        uint256 badIndex = 1;
        uint256 amount = 10;
        bytes32 root = _leaf(validIndex, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, badIndex, address(token), amount, proof);
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__InvalidProof.selector
        );
    }

    function test_invalid_proof_wrong_claimant_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        address claimant = makeAddr("claimant");
        bytes32 root = _leaf(index, claimant, address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__InvalidProof.selector
        );
    }

    function test_invalid_proof_wrong_token_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        ClaimTokenMock wrongToken = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(wrongToken), amount, proof);
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__InvalidProof.selector
        );
    }

    function test_invalid_proof_wrong_amount_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount + 1, proof);
        _assertRevertSelector(
            success, data, ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__InvalidProof.selector
        );
    }

    function test_eth_claim_underfunded_reverts() public {
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(0), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(0), amount, proof);
        _assertRevertSelector(
            success,
            data,
            ZkSyncLiteSunsetClaimDistributorL1.ZkSyncLiteSunsetClaimDistributor__EthTransferFailed.selector
        );
        assertFalse(distributor.isClaimed(index));
    }

    function test_erc20_claim_underfunded_reverts() public {
        ClaimTokenMock token = new ClaimTokenMock();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);
        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(0xBEEF));

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = _callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
        assertFalse(distributor.isClaimed(index));
    }

    function test_proxy_admin_cannot_fallback_to_claim() public {
        ClaimTokenMock token = new ClaimTokenMock();
        ProxyAdminCaller admin = new ProxyAdminCaller();
        uint256 index = 0;
        uint256 amount = 10;
        bytes32 root = _leaf(index, address(this), address(token), amount);

        ZkSyncLiteSunsetClaimDistributorL1 distributor = _deployDistributor(root, block.timestamp, address(admin));
        token.mint(address(distributor), amount);

        bytes32[] memory proof = new bytes32[](0);
        (bool success, bytes memory data) = admin.callClaim(distributor, index, address(token), amount, proof);
        _assertRevertSelector(success, data, ERROR_STRING_SELECTOR);
    }

    function _deployDistributor(bytes32 root, uint256 windowStart, address admin)
        internal
        returns (ZkSyncLiteSunsetClaimDistributorL1)
    {
        return _deployDistributorWithOwner(root, windowStart, admin, address(this));
    }

    function _deployDistributorWithOwner(bytes32 root, uint256 windowStart, address admin, address ownerAddress)
        internal
        returns (ZkSyncLiteSunsetClaimDistributorL1)
    {
        ZkSyncLiteSunsetClaimDistributorL1 implementation = new ZkSyncLiteSunsetClaimDistributorL1();
        bytes memory initData =
            abi.encodeWithSignature("initialize(bytes32,uint256,address)", root, windowStart, ownerAddress);
        TransparentUpgradeableProxy proxy = new TransparentUpgradeableProxy(address(implementation), admin, initData);
        return ZkSyncLiteSunsetClaimDistributorL1(payable(address(proxy)));
    }

    function _leaf(uint256 index, address claimant, address token, uint256 amount) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked(index, claimant, token, amount));
    }

    function _hashPair(bytes32 a, bytes32 b) internal pure returns (bytes32) {
        return a < b ? keccak256(abi.encodePacked(a, b)) : keccak256(abi.encodePacked(b, a));
    }

    function _callClaim(
        ZkSyncLiteSunsetClaimDistributorL1 distributor,
        uint256 index,
        address token,
        uint256 amount,
        bytes32[] memory proof
    ) internal returns (bool, bytes memory) {
        return address(distributor)
            .call(abi.encodeCall(ZkSyncLiteSunsetClaimDistributorL1.claim, (index, token, amount, proof)));
    }

    function _assertRevertSelector(bool success, bytes memory revertData, bytes4 expectedSelector) internal {
        assertFalse(success);
        assertEq(_selector(revertData), expectedSelector);
    }

    function _selector(bytes memory data) internal pure returns (bytes4 selector) {
        if (data.length < 4) {
            return bytes4(0);
        }
        assembly {
            selector := mload(add(data, 32))
        }
    }
}
