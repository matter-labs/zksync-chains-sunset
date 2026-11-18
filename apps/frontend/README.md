# Frontend

Astro + TypeScript frontend workspace for the zkSync Lite Sunset claim and migrate UIs.

## Dev

Start backend (dummy API endpoints included):

```sh
just backend server-dev
```

Start frontend:

```sh
just frontend dev
```

This starts both apps:

- claim at `http://127.0.0.1:4321`
- migrate at `http://127.0.0.1:4322`

The apps call the backend at `PUBLIC_API_BASE_URL` when set.
For local preview/e2e, the workspace also supports:

- `PUBLIC_CHAIN_ID`
- `PUBLIC_CHAIN_RPC_URL`
- `PUBLIC_WALLET_CHAIN_RPC_URL`
- `PUBLIC_CLAIM_APP_URL`
- `PUBLIC_MIGRATE_APP_URL`
- `PUBLIC_WALLETCONNECT_PROJECT_ID`

`PUBLIC_CHAIN_ID` selects the wallet network used during connect/sign/send for
the L1-only flow. `PUBLIC_CHAIN_RPC_URL` is the browser-side RPC URL used for
frontend reads. `PUBLIC_WALLET_CHAIN_RPC_URL` can override the RPC URL passed to
wallets when adding/switching networks; this is useful when a desktop browser
can read from loopback while a mobile wallet needs a LAN URL.

If none of these are set, the apps use local fallbacks based on the required
chain ID:

- `31338 -> http://<current-host>:5010`
- `31337 -> http://<current-host>:8545`

For compose-backed local dev, `31338` is the dedicated seeded runtime chain.
That keeps MetaMask from confusing the local fork with public Sepolia.

## Compose E2E

Run the real compose-backed migrate-then-claim browser flow with:

```sh
just test-e2e
```

or:

```sh
just frontend test-e2e
```

For lightweight frontend-only browser coverage with mocked wallet and API
traffic, use:

```sh
bun run test:e2e
```

or:

```sh
just frontend test-e2e-ui
```

To boot the same supported seeded browser stack without launching
Playwright, use:

```sh
just frontend dev-e2e
```

For the seeded manual/operator path on the supported runtime, use:

```sh
just frontend dev-seeded-address 0xYourAddress
```

This boots the same seeded compose stack as `just dev-seeded-address`, then runs
Playwright against dedicated preview ports:

- claim at `http://127.0.0.1:4421`
- migrate at `http://127.0.0.1:4422`

`ETH_SEPOLIA_RPC_URL` is optional. When provided via your shell or
`infra/.env.local`, the local chain runs as a Sepolia fork. Without it, the
seeded stack still runs against a blank local Anvil chain with the same local
dev chain id.

`PUBLIC_WALLETCONNECT_PROJECT_ID` is optional. When it is set, the claim app
enables a WalletConnect path in addition to injected-wallet discovery. The
migrate app intentionally disables WalletConnect and supports injected wallets
only.

WalletConnect is provided through wagmi's `walletConnect` connector. Do not add
or initialize a separate WalletConnect SDK unless the app needs custom session
or QR modal behavior that wagmi cannot provide.

For local mobile-wallet testing, use a WalletConnect test project ID with the
seeded stack:

```sh
env PUBLIC_WALLETCONNECT_PROJECT_ID=<test-project-id> \
  just dev-seeded-address 0xYourMobileWalletAddress
```

The seeded dev script also auto-loads the repo-root `.env.local` file when it
exists. That file is gitignored, so local WalletConnect settings can live there:

```sh
PUBLIC_WALLETCONNECT_PROJECT_ID=<test-project-id>
WALLETCONNECT_DEV_HOST=192.168.1.59
```

With those values in `.env.local`, the normal command is enough:

```sh
just dev-seeded-address 0xYourMobileWalletAddress
```

To set up the withdraw flow after the seeded stack is running, migrate every
pending token with an Anvil-funded account:

```sh
just dev-migrate-all-tokens
```

When `PUBLIC_WALLETCONNECT_PROJECT_ID` is set, `just dev-seeded-address` builds
the claim frontend with WalletConnect and builds the migrate frontend without
WalletConnect:

- claim origin: `http://<lan-ip>:4321`
- migrate origin: `http://<lan-ip>:4322`
- browser RPC URL: `http://127.0.0.1:5010`
- wallet RPC URL: `http://<lan-ip>:5010`

The frontend API URL remains loopback by default (`http://127.0.0.1:3000`) for
desktop browser testing. Browser-side chain reads also remain loopback by
default, while the LAN RPC URL is passed to wallets so mobile wallets can reach
Anvil. The script also checks
frontend readiness over loopback because some local environments cannot curl
their own LAN address even while the browser/mobile device can use it.

Allowlist the claim origin, including the port, in the WalletConnect dashboard
for the test project. For example: `http://192.168.1.59:4321`.

If `WALLETCONNECT_DEV_HOST` is not set, the script auto-detects the LAN address
from `en0` on macOS. If the wrong LAN address is detected, override it:

```sh
env PUBLIC_WALLETCONNECT_PROJECT_ID=<test-project-id> \
  WALLETCONNECT_DEV_HOST=192.168.1.59 \
  just dev-seeded-address 0xYourMobileWalletAddress
```
