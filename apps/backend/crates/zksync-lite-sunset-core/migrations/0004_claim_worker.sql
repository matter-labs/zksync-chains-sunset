-- Decouples sponsor-claim broadcast from the HTTP handler. `submit_claim` now
-- writes the row in `pending`; a backend worker drains pending rows, broadcasts
-- the tx, and transitions to `submitted`. Broadcast retry state lives on the
-- row itself so worker restarts (or replicas, if ever lifted to multi-instance)
-- can resume without losing schedule.
ALTER TABLE claim_requests
    ADD COLUMN IF NOT EXISTS attempts INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS assigned_nonce BIGINT;

CREATE INDEX IF NOT EXISTS idx_claim_requests_pending_ready
    ON claim_requests (next_attempt_at)
    WHERE status = 'pending';
