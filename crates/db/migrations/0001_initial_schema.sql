-- ============================================================================
-- 0. EXTENSIONS & SETUP
-- ============================================================================
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";
CREATE EXTENSION IF NOT EXISTS "btree_gin";
CREATE EXTENSION IF NOT EXISTS "vector";

-- ============================================================================
-- 1. USERS & AUTHENTICATION
-- ============================================================================
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    api_key TEXT UNIQUE,
    backup_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Case-insensitive unique username enforcement
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_lower_username 
    ON users(LOWER(username));

CREATE TABLE IF NOT EXISTS passkey_credentials (
    id TEXT PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,
    sign_count BIGINT NOT NULL DEFAULT 0,
    name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_passkeys_user ON passkey_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_users_api_key ON users(api_key);
CREATE INDEX IF NOT EXISTS idx_users_backup ON users(backup_enabled);

-- ============================================================================
-- 2. CORE ASSETS
-- ============================================================================
CREATE TABLE IF NOT EXISTS assets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL,
    file_name TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    folder_path TEXT NOT NULL,
    thumb_path TEXT NOT NULL,
    preview_path TEXT NOT NULL,
    file_size_bytes BIGINT NOT NULL,
    mime_type TEXT NOT NULL,
    width INT,
    height INT,
    aspect_ratio REAL,
    duration_seconds REAL,
    fps REAL,
    video_codec TEXT,
    is_favorite BOOLEAN NOT NULL DEFAULT FALSE,

    -- Vector Search (CLIP Embeddings)
    clip_processed BOOLEAN NOT NULL DEFAULT FALSE,
    clip_embedding vector(512),

    -- Pipeline completion flags
    face_processed BOOLEAN NOT NULL DEFAULT FALSE,
    tags_processed BOOLEAN NOT NULL DEFAULT FALSE,

    -- Soft delete
    deleted_at TIMESTAMPTZ DEFAULT NULL,

    -- Temporal metadata
    captured_at TIMESTAMPTZ,
    captured_at_local TEXT,
    timezone_offset TEXT,
    year INT,
    month INT CHECK (month BETWEEN 1 AND 12),
    day INT CHECK (day BETWEEN 1 AND 31),
    day_of_week INT CHECK (day_of_week BETWEEN 0 AND 6),
    hour INT CHECK (hour BETWEEN 0 AND 23),
    
    -- Geographic metadata
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    altitude_meters REAL,
    city TEXT,
    subdivision TEXT,
    country TEXT,
    country_code TEXT,

    -- Camera & Origin EXIF
    camera_make TEXT,
    camera_model TEXT,
    lens_model TEXT,
    focal_length_mm REAL,
    focal_length_35mm_equiv INT,
    aperture_f_stop REAL,
    iso_speed INT,
    exposure_time_str TEXT,
    exposure_time_seconds REAL,
    flash_fired BOOLEAN,
    white_balance TEXT,
    orientation INT DEFAULT 1,
    
    -- Social & Web Origin Tracking
    author TEXT,
    copyright TEXT,
    source_platform TEXT,
    source_url TEXT,
    source_post_id TEXT,
    caption TEXT,
    raw_metadata JSONB,

    -- Weighted Full-Text Search Vector:
    -- Priority: 'A' = Caption, 'B' = File Name, 'C' = Author, City, 'D' = Camera Model
    search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', coalesce(caption, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(file_name, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(author, '') || ' ' || coalesce(city, '')), 'C') ||
        setweight(to_tsvector('simple', coalesce(camera_model, '')), 'D')
    ) STORED,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(user_id, sha256),
    UNIQUE(user_id, rel_path)
) WITH (fillfactor = 90);

-- ============================================================================
-- 2.1 ASSETS INDEXES
-- ============================================================================

-- Primary timeline cursor pagination
CREATE INDEX IF NOT EXISTS idx_assets_cursor_pagination 
    ON assets(user_id, captured_at DESC NULLS LAST, id DESC)
    WHERE deleted_at IS NULL;

-- Partial index for undated media keyset pagination
CREATE INDEX IF NOT EXISTS idx_assets_undated 
    ON assets(user_id, id DESC) 
    WHERE captured_at IS NULL AND deleted_at IS NULL;

-- Trash feed pagination
CREATE INDEX IF NOT EXISTS idx_assets_trash
    ON assets(user_id, deleted_at DESC, id DESC)
    WHERE deleted_at IS NOT NULL;

-- Full-text search (GIN)
CREATE INDEX IF NOT EXISTS idx_assets_search 
    ON assets USING GIN(search_vector);

-- Vector HNSW Cosine Index for Sub-Millisecond Similarity Queries
CREATE INDEX IF NOT EXISTS idx_assets_clip_hnsw 
    ON assets USING hnsw (clip_embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64)
    WHERE deleted_at IS NULL AND clip_embedding IS NOT NULL;

-- Geo coordinates bounding box
CREATE INDEX IF NOT EXISTS idx_assets_lat_long
    ON assets(user_id, latitude, longitude)
    WHERE latitude IS NOT NULL AND longitude IS NOT NULL AND deleted_at IS NULL;

-- Media type tab filters
CREATE INDEX IF NOT EXISTS idx_assets_user_photos 
    ON assets(user_id, captured_at DESC NULLS LAST, id DESC) 
    WHERE duration_seconds IS NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_user_videos 
    ON assets(user_id, captured_at DESC NULLS LAST, id DESC) 
    WHERE duration_seconds IS NOT NULL AND deleted_at IS NULL;

-- Pattern-matching index for hierarchical folder drilling
CREATE INDEX IF NOT EXISTS idx_assets_folder_pattern 
    ON assets(user_id, folder_path varchar_pattern_ops) 
    WHERE deleted_at IS NULL;

-- Favorites timeline
CREATE INDEX IF NOT EXISTS idx_assets_favorites
    ON assets(user_id, captured_at DESC NULLS LAST)
    WHERE is_favorite = TRUE AND deleted_at IS NULL;

-- Common metadata filter shortcuts
CREATE INDEX IF NOT EXISTS idx_assets_city 
    ON assets(user_id, city) 
    WHERE city IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_camera 
    ON assets(user_id, camera_model) 
    WHERE camera_model IS NOT NULL AND deleted_at IS NULL;

-- ============================================================================
-- 3. TAGS & MULTI-LABEL MAPPINGS
-- ============================================================================
CREATE TABLE IF NOT EXISTS tags (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    category INT NOT NULL DEFAULT 0,
    usage_count INT NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'model' CHECK (source IN ('model', 'manual', 'scraped')),
    UNIQUE(user_id, name)
);

-- User-scoped Trigram Autocomplete Index
CREATE INDEX IF NOT EXISTS idx_tags_user_autocomplete 
    ON tags USING GIN (user_id, name gin_trgm_ops);

CREATE TABLE IF NOT EXISTS asset_tags (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id BIGINT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    confidence REAL NOT NULL DEFAULT 1.0,
    source TEXT NOT NULL DEFAULT 'AI' CHECK (source IN ('AI', 'USER', 'SCRAPE')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (asset_id, tag_id)
);

-- Covering indexes for fast bidirectional seeks
CREATE INDEX IF NOT EXISTS idx_asset_tags_tag_user 
    ON asset_tags(tag_id, user_id, asset_id);

CREATE INDEX IF NOT EXISTS idx_asset_tags_user_asset 
    ON asset_tags(user_id, asset_id, tag_id);

-- ============================================================================
-- 4. PEOPLE & FACES
-- ============================================================================
CREATE TABLE IF NOT EXISTS persons (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT,
    cover_face_id UUID,
    face_count INT NOT NULL DEFAULT 1,
    centroid_embedding vector(512) NOT NULL,
    is_hidden BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT uq_persons_user_id_id UNIQUE (user_id, id)
);

CREATE INDEX IF NOT EXISTS idx_persons_feed 
    ON persons(user_id, is_hidden, face_count DESC);

CREATE INDEX IF NOT EXISTS idx_persons_name 
    ON persons(user_id, name) 
    WHERE name IS NOT NULL;

CREATE TABLE IF NOT EXISTS asset_faces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    person_id UUID,
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    detection_score REAL NOT NULL,
    face_thumb_path TEXT NOT NULL,
    embedding vector(512) NOT NULL,
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id, person_id) REFERENCES persons(user_id, id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_asset_faces_person 
    ON asset_faces(person_id, is_verified, detection_score DESC);

CREATE INDEX IF NOT EXISTS idx_asset_faces_user_asset 
    ON asset_faces(user_id, asset_id, person_id);

CREATE INDEX IF NOT EXISTS idx_asset_faces_asset_lookup 
    ON asset_faces(asset_id, bbox_x ASC);

CREATE INDEX IF NOT EXISTS idx_asset_faces_embedding_hnsw 
    ON asset_faces USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

-- ============================================================================
-- 5. OBJECTS & SKELETON POSES
-- ============================================================================
CREATE TABLE IF NOT EXISTS asset_objects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    class_id INT NOT NULL,
    label TEXT NOT NULL,
    score REAL NOT NULL,
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_asset_objects_asset 
    ON asset_objects(asset_id);

CREATE INDEX IF NOT EXISTS idx_asset_objects_user_label 
    ON asset_objects(user_id, label, score DESC);

CREATE TABLE IF NOT EXISTS asset_poses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    score REAL NOT NULL,
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    keypoints JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_asset_poses_asset 
    ON asset_poses(asset_id);

-- ============================================================================
-- 6. ALBUMS
-- ============================================================================
CREATE TABLE IF NOT EXISTS albums (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    album_type TEXT NOT NULL DEFAULT 'MANUAL' CHECK (album_type IN ('MANUAL', 'SMART')),
    cover_asset_id UUID REFERENCES assets(id) ON DELETE SET NULL,
    filter_criteria JSONB,
    deleted_at TIMESTAMPTZ DEFAULT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, title)
);

CREATE INDEX IF NOT EXISTS idx_albums_active_feed 
    ON albums(user_id, created_at DESC) 
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS album_assets (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    album_id UUID NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INT DEFAULT 0,
    added_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (album_id, asset_id)
);

-- Ordering and reverse traversal within albums
CREATE INDEX IF NOT EXISTS idx_album_assets_ordered 
    ON album_assets(album_id, position ASC, added_at DESC, asset_id);

CREATE INDEX IF NOT EXISTS idx_album_assets_reverse 
    ON album_assets(asset_id, album_id);

-- ============================================================================
-- 7. CONSOLIDATED JOB PIPELINE (1 ROW PER ASSET, HOT UPDATES ENABLED)
-- ============================================================================
CREATE TABLE IF NOT EXISTS processing_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    rel_path TEXT NOT NULL DEFAULT '',
    folder_path TEXT NOT NULL,
    disk_path TEXT NOT NULL,
    sha256 TEXT NOT NULL DEFAULT '',
    file_size_bytes BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    current_stage TEXT NOT NULL DEFAULT 'assemble',
    attempts INT NOT NULL DEFAULT 0,
    last_error TEXT,
    payload JSONB,

    -- Worker Pipeline Stage Flags
    assemble_done BOOLEAN NOT NULL DEFAULT FALSE,
    thumb_done    BOOLEAN NOT NULL DEFAULT FALSE,
    ai_faces_done BOOLEAN NOT NULL DEFAULT FALSE,
    ai_clip_done  BOOLEAN NOT NULL DEFAULT FALSE,
    ai_tags_done  BOOLEAN NOT NULL DEFAULT FALSE,
    ai_poses_done BOOLEAN NOT NULL DEFAULT FALSE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
) WITH (fillfactor = 70);

-- Fast recovery pulls for Worker 0 (Assemble)
CREATE INDEX IF NOT EXISTS idx_jobs_pending_assemble 
    ON processing_jobs(created_at ASC)
    WHERE status = 'pending' AND assemble_done = FALSE;

-- Fast recovery pulls for Worker 1 (Thumbnails)
CREATE INDEX IF NOT EXISTS idx_jobs_pending_thumb 
    ON processing_jobs(created_at ASC)
    WHERE status = 'pending' AND assemble_done = TRUE AND thumb_done = FALSE;

-- Fast recovery pulls for Worker 2 (AI Enrichment)
CREATE INDEX IF NOT EXISTS idx_jobs_pending_ai 
    ON processing_jobs(created_at ASC)
    WHERE status = 'pending' AND thumb_done = TRUE 
      AND (ai_faces_done = FALSE OR ai_clip_done = FALSE OR ai_tags_done = FALSE OR ai_poses_done = FALSE);

-- Atomic batch release index
CREATE INDEX IF NOT EXISTS idx_processing_jobs_batch 
    ON processing_jobs(user_id, (payload->>'batch_id'))
    WHERE status = 'staged';

-- User queue monitor
CREATE INDEX IF NOT EXISTS idx_processing_jobs_user_status 
    ON processing_jobs(user_id, status, created_at);

-- ============================================================================
-- 8. SCRAPES & INGESTION
-- ============================================================================
CREATE TABLE IF NOT EXISTS scraped_posts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    external_post_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    author TEXT NOT NULL,
    caption TEXT,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    published_at TIMESTAMPTZ,
    next_page_url TEXT,
    location_name TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, platform, external_post_id)
);

CREATE INDEX IF NOT EXISTS idx_scraped_posts_author 
    ON scraped_posts(user_id, platform, LOWER(author));

CREATE TABLE IF NOT EXISTS discovered_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parent_post_id UUID REFERENCES scraped_posts(id) ON DELETE CASCADE,
    discovered_url TEXT NOT NULL,
    platform TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'ignored')),
    depth INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, discovered_url)
);

CREATE INDEX IF NOT EXISTS idx_discovered_queue_pending 
    ON discovered_queue(user_id, status, created_at)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS scraped_media_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scraped_post_id UUID NOT NULL REFERENCES scraped_posts(id) ON DELETE CASCADE,
    item_index INT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('image', 'video')),
    cdn_url TEXT NOT NULL,
    audio_url TEXT,
    thumbnail_url TEXT,
    suggested_filename TEXT NOT NULL,
    width INT,
    height INT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'downloaded', 'skipped')),
    asset_id UUID REFERENCES assets(id) ON DELETE SET NULL,
    caption TEXT,
    published_at TIMESTAMPTZ,
    location_name TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    source_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(scraped_post_id, item_index)
);

CREATE INDEX IF NOT EXISTS idx_scraped_items_post 
    ON scraped_media_items(scraped_post_id, item_index);

CREATE INDEX IF NOT EXISTS idx_scraped_items_status 
    ON scraped_media_items(status) 
    WHERE status = 'downloaded';

-- Covering index to speed up deduplication lookups during bulk downloads
CREATE INDEX IF NOT EXISTS idx_scraped_items_downloaded_post
    ON scraped_media_items(scraped_post_id)
    WHERE status = 'downloaded';

CREATE TABLE IF NOT EXISTS scraped_media_variants (
    id BIGSERIAL PRIMARY KEY,
    media_item_id UUID NOT NULL REFERENCES scraped_media_items(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    width INT,
    height INT,
    label TEXT,
    file_size_bytes BIGINT,
    is_master BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(media_item_id, url)
);

CREATE INDEX IF NOT EXISTS idx_variants_item 
    ON scraped_media_variants(media_item_id, width DESC);

-- Statement-Level Set-Based Batch Cleanup Trigger for Purged Assets
CREATE OR REPLACE FUNCTION trg_fn_scraped_items_batch_cleanup()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE scraped_media_items smi
    SET asset_id = NULL, status = 'skipped'
    FROM deleted_assets da
    WHERE smi.asset_id = da.id;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_scraped_items_on_asset_delete ON assets;
CREATE TRIGGER trg_scraped_items_on_asset_delete
AFTER DELETE ON assets
REFERENCING OLD TABLE AS deleted_assets
FOR EACH STATEMENT
EXECUTE FUNCTION trg_fn_scraped_items_batch_cleanup();

-- ============================================================================
-- 9. USER BACKUPS
-- ============================================================================
CREATE TABLE IF NOT EXISTS asset_backups (
    asset_id UUID PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    remote_path TEXT NOT NULL,
    synced_sha256 TEXT NOT NULL,
    backed_up_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_asset_backups_user 
    ON asset_backups(user_id);