type WalletLabelOption = {
  name: string;
};

const FALLBACK_WALLET_NAME = "Browser Wallet";

export function walletActionLabel(wallet: WalletLabelOption): string {
  const walletName = wallet.name.trim();
  return walletName.length > 0 ? walletName : FALLBACK_WALLET_NAME;
}
