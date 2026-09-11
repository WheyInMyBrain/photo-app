-- ============================================================================
-- 1. CORE ASSETS TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY NOT NULL,
    sha256 TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    rel_path TEXT NOT NULL UNIQUE,
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
    is_private INTEGER NOT NULL DEFAULT 0 CHECK (is_private IN (0, 1)),
    is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),

    -- Pipeline flags (O(1) queue lookups via partial indexes)
    face_processed INTEGER NOT NULL DEFAULT 0 CHECK (face_processed IN (0, 1)),
    tags_processed INTEGER NOT NULL DEFAULT 0 CHECK (tags_processed IN (0, 1)),

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

    -- Camera EXIF
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
    author TEXT,
    copyright TEXT,
    raw_metadata JSON,
    
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Core Feed & Pagination Indexes
CREATE INDEX IF NOT EXISTS idx_assets_cursor_pagination 
    ON assets(is_private, captured_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_assets_unprocessed_faces 
    ON assets(id) WHERE face_processed = 0;

CREATE INDEX IF NOT EXISTS idx_assets_unprocessed_tags 
    ON assets(id) WHERE tags_processed = 0;

CREATE INDEX IF NOT EXISTS idx_assets_folder_seek 
    ON assets(is_private, folder_path);

CREATE INDEX IF NOT EXISTS idx_assets_geo 
    ON assets(is_private, city, country_code);

CREATE INDEX IF NOT EXISTS idx_assets_timeline 
    ON assets(is_private, year DESC, month DESC);

CREATE INDEX IF NOT EXISTS idx_assets_camera 
    ON assets(is_private, camera_model);


-- ============================================================================
-- 2. TAGS & MULTI-LABEL TABLES
-- ============================================================================
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    category INTEGER NOT NULL DEFAULT 0, -- 0=General, 4=Character, 9=Rating
    usage_count INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'model' CHECK (source IN ('model', 'manual'))
);

CREATE INDEX IF NOT EXISTS idx_tags_search_autocomplete 
    ON tags(name COLLATE NOCASE, usage_count DESC);

CREATE TABLE IF NOT EXISTS asset_tags (
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    confidence REAL NOT NULL,
    source TEXT NOT NULL DEFAULT 'AI' CHECK (source IN ('AI', 'USER')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (asset_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_asset_tags_tag_id_confidence 
    ON asset_tags(tag_id, confidence DESC, asset_id);

CREATE INDEX IF NOT EXISTS idx_asset_tags_asset_id_confidence 
    ON asset_tags(asset_id, confidence DESC, tag_id);


-- ============================================================================
-- 3. PEOPLE & FACES TABLES
-- ============================================================================
CREATE TABLE IF NOT EXISTS persons (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT,
    cover_face_id TEXT,
    face_count INTEGER NOT NULL DEFAULT 1,
    centroid_embedding BLOB NOT NULL,
    is_hidden INTEGER NOT NULL DEFAULT 0 CHECK (is_hidden IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_persons_feed 
    ON persons(is_hidden, face_count DESC);

CREATE INDEX IF NOT EXISTS idx_persons_name 
    ON persons(name COLLATE NOCASE);

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
-- 4. ALBUMS TABLES
-- ============================================================================
CREATE TABLE IF NOT EXISTS albums (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    album_type TEXT NOT NULL CHECK (album_type IN ('MANUAL', 'SMART')),
    is_private INTEGER NOT NULL DEFAULT 0 CHECK (is_private IN (0, 1)),
    cover_asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    filter_criteria JSON,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS album_assets (
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INTEGER DEFAULT 0,
    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (album_id, asset_id)
) WITHOUT ROWID;


-- ============================================================================
-- 5. OPTIMIZED FULL-TEXT SEARCH (FTS5) & TRIGGERS
-- ============================================================================
-- External-content inverted search index with accent stripping and punctuation support
CREATE VIRTUAL TABLE IF NOT EXISTS asset_search_index USING fts5(
    asset_id UNINDEXED,
    is_private UNINDEXED,
    persons,       -- e.g., "ram sharma john"
    tags,          -- e.g., "hotel swimming pool indoor building"
    location,      -- e.g., "mumbai maharashtra india"
    temporal,      -- e.g., "2024 2024-09 september sep thursday"
    camera,        -- e.g., "sony ilce-7m4 a7iv 50mm"
    file_name,     -- e.g., "IMG_2041.jpg"
    tokenize = 'unicode61 remove_diacritics 2 tokenchars ''-._'''
);

-- Automatically purge search index entries whenever an asset is deleted
CREATE TRIGGER IF NOT EXISTS trg_assets_fts_cleanup
AFTER DELETE ON assets
BEGIN
    DELETE FROM asset_search_index WHERE asset_id = OLD.id;
END;

-- Vault security state & passkey credentials
CREATE TABLE IF NOT EXISTS vault_security (
    id INTEGER PRIMARY KEY CHECK (id = 1), -- Single system master row
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS passkey_credentials (
    id TEXT PRIMARY KEY NOT NULL,          -- base64url credential_id
    public_key BLOB NOT NULL,
    sign_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);