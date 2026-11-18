# zksync-lite-migration-claims

Foundry project containing the L1 claim contracts and dev-only migration mock used for the zkSync Lite sunset flow.

Dependencies:
- `lib/zk-governance` (reference implementation source)
- `lib/openzeppelin-contracts` (contract dependency used by the local distributor implementation)
- `lib/openzeppelin-contracts-upgradeable` (initializer-based proxy-compatible OZ base contracts)

Run from repo root:
- `just contracts claims-install`

Common commands (from repo root):
- `just contracts claims-build`
- `just contracts claims-test`

## Migration Mock Stack

The local migration mock stack exposes the simplified per-token flow used by the new L1-only migration design:

1. `MockZkSyncLiteMigrationState`
- per-token entrypoint: `migrateToken(TokenMigration)`
- token migration leaf: `(index, token, total)`
- configured claim root: a single claimant tree root for the L1 distributor
- purpose: mirror the current L1-only migration ABI and funding semantics

The migration mock verifies:

- the token-level migration proof against `TOKEN_MIGRATION_ROOT`
- `IL1ClaimDistributor(l1ClaimDistributor).MERKLE_ROOT() == claimRoot`
- the full token total is transferred to the configured L1 distributor

## Claim Entrypoints

There is a single L1 claim contract with two permissionless claim paths. Batching across multiple leaves is expected to be composed at the call site via Multicall3 (or equivalent) rather than via a contract-level helper.

1. `ZkSyncLiteSunsetClaimDistributorL1`
- `claim(index, token, amount, merkleProof)`
  - caller: claimant (self-claim shortcut; equivalent to `claimTo` with `claimant = msg.sender`)
  - signature required: no
- `claimTo(index, claimant, token, amount, merkleProof)`
  - caller: anyone (claimant, backend sponsor, arbitrary relayer)
  - signature required: no
- leaf format: `(index, claimant, token, amount)`

Notes:
- In every path, funds are transferred to the `claimant` encoded in the Merkle leaf, not to the transaction sender. The leaf binds the destination, so a permissionless entrypoint cannot redirect or steal a claim.
- Replay protection is the per-leaf claimed bitmap on the distributor; there are no on-chain nonces or signatures.
- Sponsored batches are assembled by the backend by wrapping multiple `claimTo` calls inside Multicall3's `aggregate`, which reverts atomically on any inner failure.
- User authorization for backend-submitted claims happens off-chain: the backend verifies an EIP-712 signature server-side before relaying. The distributor itself does not verify signatures.
- The distributor exposes `pause()` / `unpause()` guarded by the configured claim owner so claim execution can be stopped during an incident.
- Deployments initialize the distributor with `initialize(bytes32,uint256,address)` to set that owner explicitly.
