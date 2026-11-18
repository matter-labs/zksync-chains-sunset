import { describe, expect, it } from "bun:test";

import {
  chainName,
  configuredWalletChainId,
  parseHexChainId,
  readRpcUrlForChain,
  walletChainForChainId,
  walletChains,
  walletRpcUrlForChain,
  type WalletChainEnv,
} from "../../shared/src/wallet/chains";

describe("wallet chain resolution", () => {
  const hostname = "dev.local";

  it("maps the local seeded and staging chain IDs to the shared RPC env", () => {
    const env: WalletChainEnv = {
      publicChainRpcUrl: "https://rpc.example",
    };

    expect(walletRpcUrlForChain(31338, env, hostname)).toBe(
      "https://rpc.example",
    );
    expect(walletRpcUrlForChain(11155111, env, hostname)).toBe(
      "https://rpc.example",
    );
    expect(walletRpcUrlForChain(314159, env, hostname)).toBeNull();
  });

  it("can split browser read RPCs from wallet network RPCs", () => {
    const env: WalletChainEnv = {
      publicChainRpcUrl: "http://127.0.0.1:5010",
      publicWalletChainRpcUrl: "http://192.168.1.59:5010",
    };

    expect(readRpcUrlForChain(31338, env, hostname)).toBe(
      "http://127.0.0.1:5010",
    );
    expect(walletRpcUrlForChain(31338, env, hostname)).toBe(
      "http://192.168.1.59:5010",
    );

    const chain = walletChainForChainId(31338, env, hostname);
    expect(chain?.rpcUrls.default.http).toEqual(["http://192.168.1.59:5010"]);
  });

  it("maps explicit staging runtime chain IDs to the shared RPC env", () => {
    const env: WalletChainEnv = {
      publicChainId: "314159",
      publicChainRpcUrl: "https://rpc-staging.example",
    };

    expect(walletRpcUrlForChain(314159, env, hostname)).toBe(
      "https://rpc-staging.example",
    );
    expect(chainName(314159)).toBe("zkSync Lite Staging");

    const chain = walletChainForChainId(314159, env, hostname);
    expect(chain?.id).toBe(314159);
    expect(chain?.rpcUrls.default.http).toEqual([
      "https://rpc-staging.example",
    ]);
  });

  it("maps explicit mainnet builds to the configured RPC env", () => {
    const env: WalletChainEnv = {
      publicChainId: "1",
      publicChainRpcUrl: "https://rpc-mainnet.example",
    };

    expect(walletRpcUrlForChain(1, env, hostname)).toBe(
      "https://rpc-mainnet.example",
    );

    const chain = walletChainForChainId(1, env, hostname);
    expect(chain?.id).toBe(1);
    expect(chain?.rpcUrls.default.http).toEqual([
      "https://rpc-mainnet.example",
    ]);

    const chains = walletChains(env, hostname);
    expect(chains.filter((candidate) => candidate.id === 1).length).toBe(1);
    expect(
      chains.find((candidate) => candidate.id === 1)?.rpcUrls.default.http,
    ).toEqual(["https://rpc-mainnet.example"]);
  });

  it("defines explicit local fallback chains for the single-chain dev flow", () => {
    const l1Chain = walletChainForChainId(31338, {}, hostname);
    const anvilChain = walletChainForChainId(31337, {}, hostname);

    expect(chainName(31338)).toBe("Local Seeded Fork");
    expect(chainName(31337)).toBe("Local Anvil");
    expect(l1Chain?.rpcUrls.default.http).toEqual(["http://dev.local:5010"]);
    expect(anvilChain?.rpcUrls.default.http).toEqual(["http://dev.local:8545"]);
  });

  it("resolves the app wallet chain from explicit env or the runtime host", () => {
    expect(configuredWalletChainId({ publicChainId: "31337" }, hostname)).toBe(
      31337,
    );
    expect(
      configuredWalletChainId(
        { publicChainRpcUrl: "https://rpc.example" },
        "claim.example",
      ),
    ).toBe(11155111);
    expect(configuredWalletChainId({}, "127.0.0.1")).toBe(31338);
  });

  it("accepts numeric chain IDs from WalletConnect providers", () => {
    expect(parseHexChainId("0x1")).toBe(1);
    expect(parseHexChainId(1)).toBe(1);
  });
});
