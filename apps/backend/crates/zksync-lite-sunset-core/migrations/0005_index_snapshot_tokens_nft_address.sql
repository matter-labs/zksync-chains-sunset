CREATE INDEX IF NOT EXISTS idx_snapshot_tokens_nft_lower_address
    ON snapshot_tokens (LOWER(address))
    WHERE id >= 1000;
