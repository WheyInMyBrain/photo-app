-- ============================================================================
-- 0004: PERSISTENT INGESTION QUEUE & WORKER PROGRESS
-- ============================================================================

CREATE TABLE IF NOT EXISTS link_ingest_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_url TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT 'generic',
    requested_folder TEXT,
    status TEXT NOT NULL DEFAULT 'pending' 
        CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'rate_limited')),
    
    -- Retry & Backoff tracking
    attempts INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 3,
    last_error TEXT,
    next_retry_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Multi-batch & Profile pagination continuation state
    resume_cursor TEXT,
    batch_page_count INT NOT NULL DEFAULT 0,

    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Prevent submitting the exact same active URL multiple times for the same user
    CONSTRAINT uq_user_active_url UNIQUE (user_id, target_url)
);

-- 1. Fast worker claim index (Lock-free cursor polling with FOR UPDATE SKIP LOCKED)
CREATE INDEX IF NOT EXISTS idx_link_ingest_queue_claim
    ON link_ingest_queue (status, next_retry_at ASC, created_at ASC)
    WHERE status IN ('pending', 'rate_limited');

-- 2. User monitoring index
CREATE INDEX IF NOT EXISTS idx_link_ingest_queue_user_status
    ON link_ingest_queue (user_id, status, created_at DESC);