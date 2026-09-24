-- ============================================================================
-- 0. USERS & AUTHENTICATION
-- ============================================================================
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    api_key TEXT UNIQUE,
    backup_enabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS passkey_credentials (
    id TEXT PRIMARY KEY NOT NULL,          -- base64url credential_id
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key BLOB NOT NULL,
    sign_count INTEGER NOT NULL DEFAULT 0,
    name TEXT,                             -- e.g. "MacBook TouchID"
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_passkeys_user ON passkey_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_users_api_key ON users(api_key);
CREATE INDEX IF NOT EXISTS idx_users_backup ON users(backup_enabled);

-- ============================================================================
-- 1. CORE ASSETS TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL,
    file_name TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    folder_path TEXT NOT NULL,
    thumb_path TEXT NOT NULL,
    preview_path TEXT NOT NULL,
    file_size_bytes INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    width INTEGER,
    height INTEGER,
    aspect_ratio REAL,
    duration_seconds REAL,
    fps REAL,
    video_codec TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),

    -- CLIP BLOB
    clip_processed INTEGER NOT NULL DEFAULT 0 CHECK (clip_processed IN (0, 1)),
    clip_embedding BLOB,

    -- Pipeline flags (O(1) queue lookups via partial indexes)
    face_processed INTEGER NOT NULL DEFAULT 0 CHECK (face_processed IN (0, 1)),
    tags_processed INTEGER NOT NULL DEFAULT 0 CHECK (tags_processed IN (0, 1)),

    -- Soft delete tracking (ISO 8601 UTC timestamp, NULL = active)
    deleted_at TEXT DEFAULT NULL,

    -- Temporal indexing
    captured_at TEXT,
    captured_at_local TEXT,
    timezone_offset TEXT,
    year INTEGER,
    month INTEGER CHECK (month BETWEEN 1 AND 12),
    day INTEGER CHECK (day BETWEEN 1 AND 31),
    day_of_week INTEGER CHECK (day_of_week BETWEEN 0 AND 6),
    hour INTEGER CHECK (hour BETWEEN 0 AND 23),
    
    -- Geo metadata
    latitude REAL,
    longitude REAL,
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
    focal_length_35mm_equiv INTEGER,
    aperture_f_stop REAL,
    iso_speed INTEGER,
    exposure_time_str TEXT,
    exposure_time_seconds REAL,
    flash_fired INTEGER CHECK (flash_fired IN (0, 1)),
    white_balance TEXT,
    orientation INTEGER DEFAULT 1,
    
    -- Social & Web Origin Tracking
    author TEXT,                           -- e.g. "@username" or camera owner
    copyright TEXT,
    source_platform TEXT,                  -- 'direct', 'instagram', 'reddit', 'local'
    source_url TEXT,                       -- original post link
    source_post_id TEXT,                   -- external post identifier
    caption TEXT,                          -- primary post title or IG/Reddit caption
    raw_metadata JSON,                     -- unconstrained JSON fallback
    
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Enforce uniqueness per user so two users can own the same file independently
    UNIQUE(user_id, sha256),
    UNIQUE(user_id, rel_path)
);

-- Main Feed & Pagination (Partial Index: per user, active only)
CREATE INDEX IF NOT EXISTS idx_assets_cursor_pagination 
    ON assets(user_id, captured_at DESC, id DESC)
    WHERE deleted_at IS NULL;

-- Trash View Index
CREATE INDEX IF NOT EXISTS idx_assets_trash
    ON assets(user_id, deleted_at DESC, id DESC)
    WHERE deleted_at IS NOT NULL;

-- Geo Coordinate Bounding Box Index
CREATE INDEX IF NOT EXISTS idx_assets_lat_long
    ON assets(user_id, latitude, longitude)
    WHERE latitude IS NOT NULL AND longitude IS NOT NULL AND deleted_at IS NULL;

-- Pipeline Queue Indexes
CREATE INDEX IF NOT EXISTS idx_assets_unprocessed_faces 
    ON assets(user_id, id) 
    WHERE face_processed = 0 AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_unprocessed_tags 
    ON assets(user_id, id) 
    WHERE tags_processed = 0 AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_clip 
    ON assets(user_id, id) 
    WHERE clip_embedding IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_assets_unprocessed_clip
    ON assets(user_id, id) 
    WHERE clip_processed = 0 AND deleted_at IS NULL;

-- Filtering & Aggregation Indexes
CREATE INDEX IF NOT EXISTS idx_assets_folder_seek 
    ON assets(user_id, folder_path)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_platform_lookup
    ON assets(user_id, source_platform)
    WHERE source_platform IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_geo 
    ON assets(user_id, city, country_code)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_timeline 
    ON assets(user_id, year DESC, month DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_camera 
    ON assets(user_id, camera_model)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_favorites
    ON assets(user_id, captured_at DESC)
    WHERE is_favorite = 1 AND deleted_at IS NULL;


-- ============================================================================
-- 2. TAGS & MULTI-LABEL TABLES (USER-SCOPED)
-- ============================================================================
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL COLLATE NOCASE,
    category INTEGER NOT NULL DEFAULT 0,  -- 0=General, 1=Hashtag, 2=Author/Artist, 4=Character, 9=Rating
    usage_count INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'model' CHECK (source IN ('model', 'manual', 'scraped')),
    UNIQUE(user_id, name COLLATE NOCASE)
);

CREATE INDEX IF NOT EXISTS idx_tags_user_autocomplete 
    ON tags(user_id, name COLLATE NOCASE, usage_count DESC);

CREATE TABLE IF NOT EXISTS asset_tags (
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    confidence REAL NOT NULL DEFAULT 1.0,
    source TEXT NOT NULL DEFAULT 'AI' CHECK (source IN ('AI', 'USER', 'SCRAPE')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (asset_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_asset_tags_tag_id_confidence 
    ON asset_tags(tag_id, confidence DESC, asset_id);

CREATE INDEX IF NOT EXISTS idx_asset_tags_asset_id_confidence 
    ON asset_tags(asset_id, confidence DESC, tag_id);


-- ============================================================================
-- 3. PEOPLE & FACES TABLES (USER-SCOPED)
-- ============================================================================
CREATE TABLE IF NOT EXISTS persons (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT,
    cover_face_id TEXT,
    face_count INTEGER NOT NULL DEFAULT 1,
    centroid_embedding BLOB NOT NULL,
    is_hidden INTEGER NOT NULL DEFAULT 0 CHECK (is_hidden IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_persons_user_feed 
    ON persons(user_id, is_hidden, face_count DESC);

CREATE INDEX IF NOT EXISTS idx_persons_user_name 
    ON persons(user_id, name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS asset_faces (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    person_id TEXT REFERENCES persons(id) ON DELETE SET NULL,
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    detection_score REAL NOT NULL,
    face_thumb_path TEXT NOT NULL,
    embedding BLOB NOT NULL,
    is_verified INTEGER NOT NULL DEFAULT 0 CHECK (is_verified IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_asset_faces_person_seek 
    ON asset_faces(person_id, is_verified, detection_score DESC);

CREATE INDEX IF NOT EXISTS idx_asset_faces_asset_lookup 
    ON asset_faces(asset_id, bbox_x ASC);


-- ============================================================================
-- 4. ALBUMS TABLES (USER-SCOPED)
-- ============================================================================
CREATE TABLE IF NOT EXISTS albums (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    album_type TEXT NOT NULL CHECK (album_type IN ('MANUAL', 'SMART')),
    cover_asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    filter_criteria JSON,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_albums_user ON albums(user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS album_assets (
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INTEGER DEFAULT 0,
    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (album_id, asset_id)
) WITHOUT ROWID;


-- ============================================================================
-- 5. FULL-TEXT SEARCH (FTS5) & AUTO-CLEANUP
-- ============================================================================
CREATE VIRTUAL TABLE IF NOT EXISTS asset_search_index USING fts5(
    asset_id UNINDEXED,
    user_id UNINDEXED,
    caption,       -- e.g., "Beautiful sunset over Marine Drive"
    author,        -- e.g., "natgeo notrealbutwow"
    persons,       -- e.g., "ram sharma john"
    tags,          -- e.g., "fashion ootd viral summer"
    location,      -- e.g., "mumbai maharashtra india"
    temporal,      -- e.g., "2026 2026-09 september sep monday"
    camera,        -- e.g., "sony ilce-7m4 a7iv 50mm"
    file_name,     -- e.g., "instagram_post_1.mp4"
    tokenize = 'unicode61 remove_diacritics 2 tokenchars ''-._#@'''
);

CREATE TRIGGER IF NOT EXISTS trg_assets_fts_cleanup
AFTER DELETE ON assets
BEGIN
    DELETE FROM asset_search_index WHERE asset_id = OLD.id;
END;


-- ============================================================================
-- 6. ATOMIC DB-BACKED JOB QUEUE (USER-SCOPED)
-- ============================================================================
CREATE TABLE IF NOT EXISTS processing_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    folder_path TEXT NOT NULL,
    disk_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    file_size_bytes INTEGER NOT NULL,
    job_type TEXT NOT NULL DEFAULT 'thumbnail',
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'processing', 'completed', 'failed'
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    payload JSON,                          -- Holds context: { "tags": [...], "caption": "...", "author": "..." }
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_jobs_user_status_created 
    ON processing_jobs(user_id, status, created_at);

CREATE INDEX IF NOT EXISTS idx_jobs_global_pending 
    ON processing_jobs(status, created_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_jobs_type_status 
    ON processing_jobs(job_type, status);

CREATE INDEX IF NOT EXISTS idx_processing_jobs_queue 
    ON processing_jobs (job_type, status, attempts, created_at);


-- ============================================================================
-- 7. SCRAPED POSTS, MEDIA STAGING, VARIANTS & DISCOVERY QUEUE
-- ============================================================================

-- 7.1 Primary Inspected Post or Feed
CREATE TABLE IF NOT EXISTS scraped_posts (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,                     -- 'direct', 'instagram', 'reddit', 'tiktok'
    external_post_id TEXT NOT NULL,             -- URL hash, shortcode, or post ID
    source_url TEXT NOT NULL,                   -- Canonical link
    author TEXT NOT NULL,                       -- '@username' or 'web'
    caption TEXT,                               -- Caption or thread title
    tags JSON NOT NULL DEFAULT '[]',            -- ["viral", "fashion", "ootd"]
    published_at TEXT,                          -- Original post timestamp (ISO-8601)
    next_page_url TEXT,                         -- Pagination cursor (e.g. ?max_id=...)
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, platform, external_post_id)
);

CREATE INDEX IF NOT EXISTS idx_scraped_posts_lookup 
    ON scraped_posts(user_id, platform, external_post_id);


-- 7.2 Discovered Sibling / Child Posts (Crawled from grids, feeds, or rel="next")
CREATE TABLE IF NOT EXISTS discovered_queue (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parent_post_id TEXT REFERENCES scraped_posts(id) ON DELETE CASCADE,
    discovered_url TEXT NOT NULL,               -- e.g. "https://instagram.com/p/DcafCJ7yrYV/"
    platform TEXT NOT NULL,                     -- 'instagram', 'reddit', 'generic'
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'ignored')),
    depth INTEGER NOT NULL DEFAULT 1,           -- Crawl depth level
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, discovered_url)
);

CREATE INDEX IF NOT EXISTS idx_discovered_queue_pending 
    ON discovered_queue(user_id, status, created_at)
    WHERE status = 'pending';


-- 7.3 Canonical Media Item (Best Available Master)
CREATE TABLE IF NOT EXISTS scraped_media_items (
    id TEXT PRIMARY KEY NOT NULL,
    scraped_post_id TEXT NOT NULL REFERENCES scraped_posts(id) ON DELETE CASCADE,
    item_index INTEGER NOT NULL,                -- 0 for single post, 0..N for carousel
    media_type TEXT NOT NULL CHECK (media_type IN ('image', 'video')),
    cdn_url TEXT NOT NULL,                      -- Primary highest-resolution link
    audio_url TEXT,                             -- Separate audio stream (DASH/CMAF/HLS)
    thumbnail_url TEXT,                         -- Feed/grid preview poster
    suggested_filename TEXT NOT NULL,
    width INTEGER,                              -- Highest quality width
    height INTEGER,                             -- Highest quality height
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'downloaded', 'skipped')),
    asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(scraped_post_id, item_index)
);

CREATE INDEX IF NOT EXISTS idx_scraped_items_post 
    ON scraped_media_items(scraped_post_id, item_index);

CREATE INDEX IF NOT EXISTS idx_scraped_items_asset 
    ON scraped_media_items(asset_id) 
    WHERE asset_id IS NOT NULL;


-- 7.4 Alternate Resolutions & Encodes (720p, 1080p, 4K, Mobile Bitrates)
CREATE TABLE IF NOT EXISTS scraped_media_variants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_item_id TEXT NOT NULL REFERENCES scraped_media_items(id) ON DELETE CASCADE,
    url TEXT NOT NULL,                          -- Direct CDN variant link
    width INTEGER,                              -- e.g. 720
    height INTEGER,                             -- e.g. 1280
    label TEXT,                                 -- e.g. '720p', '1080p', '640w', 'DASH_AUDIO_128'
    file_size_bytes INTEGER,                    -- Known size or NULL
    is_master INTEGER NOT NULL DEFAULT 0 CHECK (is_master IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(media_item_id, url)
);

CREATE INDEX IF NOT EXISTS idx_variants_media_item 
    ON scraped_media_variants(media_item_id, width DESC);

-- Trigger: When an asset is deleted, reset staged item
CREATE TRIGGER IF NOT EXISTS trg_scraped_items_on_asset_delete
AFTER DELETE ON assets
BEGIN
    UPDATE scraped_media_items 
    SET asset_id = NULL, status = 'skipped' 
    WHERE asset_id = OLD.id;
END;

-- ============================================================================
-- 8. BACKUP MEDIA FOR USERS
-- ============================================================================

CREATE TABLE IF NOT EXISTS asset_backups (
    asset_id TEXT PRIMARY KEY NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    remote_path TEXT NOT NULL,
    synced_sha256 TEXT NOT NULL,
    backed_up_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_asset_backups_user ON asset_backups(user_id);