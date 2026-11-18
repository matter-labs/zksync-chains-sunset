CREATE TABLE IF NOT EXISTS snapshot_accounts (
    id BIGINT PRIMARY KEY,
    nonce BIGINT NOT NULL,
    address TEXT NOT NULL,
    pubkey_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshot_tokens (
    id BIGINT PRIMARY KEY,
    address TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decimals SMALLINT NOT NULL CHECK (decimals >= 0 AND decimals <= 255)
);

CREATE TABLE IF NOT EXISTS snapshot_balances (
    account_id BIGINT NOT NULL,
    coin_id BIGINT NOT NULL,
    balance NUMERIC(78, 0) NOT NULL,
    PRIMARY KEY (account_id, coin_id)
);

CREATE TABLE IF NOT EXISTS snapshot_eth_account_types (
    account_id BIGINT PRIMARY KEY,
    account_type TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_snapshot_balances_coin_id ON snapshot_balances (coin_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_balances_account_id ON snapshot_balances (account_id);

CREATE TABLE IF NOT EXISTS claim_snapshot_metadata (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    label TEXT NOT NULL,
    merkle_root TEXT NOT NULL,
    source_checksum TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS claim_entries (
    claim_index BIGINT PRIMARY KEY,
    claimant_address TEXT NOT NULL,
    token_address TEXT NOT NULL,
    amount TEXT NOT NULL,
    leaf_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (claimant_address, token_address)
);

CREATE INDEX IF NOT EXISTS idx_claim_entries_claimant_token
    ON claim_entries (claimant_address, token_address);

CREATE TABLE IF NOT EXISTS claim_proofs (
    claim_index BIGINT PRIMARY KEY
        REFERENCES claim_entries(claim_index) ON DELETE CASCADE,
    proof_hashes TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_token_states (
    user_address TEXT NOT NULL,
    token_address TEXT NOT NULL,
    migrated BOOLEAN NOT NULL DEFAULT FALSE,
    claimed BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_address, token_address)
);

CREATE TABLE IF NOT EXISTS claim_requests (
    claim_id TEXT PRIMARY KEY,
    user_address TEXT NOT NULL,
    status TEXT NOT NULL,
    claims_hash TEXT,
    nonce BIGINT,
    expiry BIGINT,
    signature TEXT,
    payload_json JSONB NOT NULL,
    tx_hash TEXT,
    block_number BIGINT,
    block_hash TEXT,
    error TEXT,
    error_code TEXT,
    error_detail TEXT,
    prepared_at TIMESTAMPTZ,
    submitted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE SEQUENCE IF NOT EXISTS claim_request_id_seq START WITH 1 INCREMENT BY 1;
CREATE INDEX IF NOT EXISTS idx_claim_requests_user_status
    ON claim_requests (user_address, status);

CREATE TABLE IF NOT EXISTS migration_requests (
    migration_id TEXT PRIMARY KEY,
    user_address TEXT NOT NULL,
    status TEXT NOT NULL,
    token_address TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    tx_hash TEXT,
    block_number BIGINT,
    block_hash TEXT,
    error TEXT,
    error_code TEXT,
    error_detail TEXT,
    prepared_at TIMESTAMPTZ,
    submitted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE SEQUENCE IF NOT EXISTS migration_request_id_seq START WITH 1 INCREMENT BY 1;
CREATE INDEX IF NOT EXISTS idx_migration_requests_user_status
    ON migration_requests (user_address, status);
