-- ============================================================================
-- 0002: PERFORMANCE OPTIMIZATION INDEXES FOR HDD-TUNED QUERIES
-- ============================================================================

-- 1. FIX: `get_dynamic_filters` Tag Aggregations & Joins
-- Speeds up joining filtered assets with tags and eliminates table scans
CREATE INDEX IF NOT EXISTS idx_asset_tags_user_asset_tag 
    ON asset_tags(user_id, asset_id, tag_id);

CREATE INDEX IF NOT EXISTS idx_asset_tags_user_tag_asset 
    ON asset_tags(user_id, tag_id, asset_id);

-- 2. FIX: `get_dynamic_filters` People & Face Aggregations
-- Speeds up filtering by person_id and gathering face counts
CREATE INDEX IF NOT EXISTS idx_asset_faces_user_asset_person 
    ON asset_faces(user_id, asset_id, person_id);

CREATE INDEX IF NOT EXISTS idx_asset_faces_user_person_asset 
    ON asset_faces(user_id, person_id, asset_id);

-- 3. FIX: Album Asset Filtering & Keyset Joins
-- Speeds up queries filtering by album (`aa.album_id = $1 AND aa.user_id = $2`)
CREATE INDEX IF NOT EXISTS idx_album_assets_user_album_asset 
    ON album_assets(user_id, album_id, asset_id);

-- 4. FIX: Missing Foreign Key Indexes (Prevents lock contention and slow cascades)
-- When assets or posts are deleted/purged, Postgres scans these tables without indexes
CREATE INDEX IF NOT EXISTS idx_scraped_items_asset_id 
    ON scraped_media_items(asset_id) 
    WHERE asset_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_albums_cover_asset_id 
    ON albums(cover_asset_id) 
    WHERE cover_asset_id IS NOT NULL;

-- 5. FIX: Metadata Aggregation Filters (Timeline, Cities, Cameras)
-- Gives instant index-only or index-assisted scans for the dynamic filter summary bar
CREATE INDEX IF NOT EXISTS idx_assets_user_active_metadata
    ON assets (user_id, captured_at DESC NULLS LAST, city, camera_model)
    WHERE deleted_at IS NULL;