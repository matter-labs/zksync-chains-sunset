-- The user_token_states table duplicated on-chain state (`isTokenMigrated`,
-- `isClaimed`) and drifted whenever migration/claim transactions happened
-- outside our worker. Eligibility and prepare endpoints now read these flags
-- straight from the migration contract and claim distributor on every call.
DROP TABLE IF EXISTS user_token_states;
