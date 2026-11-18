DROP TABLE IF EXISTS snapshot_eth_account_types;

ALTER TABLE claim_entries DROP COLUMN IF EXISTS account_type;
