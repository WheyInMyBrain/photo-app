-- ============================================================================
-- 9. PERFORMANCE FIXES FOR ALBUMS & TAG AGGREGATIONS
-- ============================================================================

-- 1. Fixes the 1.1s - 1.3s in-memory sort per album for cover thumbnails
--    Covers: WHERE album_id = ? ORDER BY position ASC, added_at DESC
CREATE INDEX IF NOT EXISTS idx_album_assets_cover_lookup
    ON album_assets(album_id, position ASC, added_at DESC, asset_id);

-- 2. Fixes the albums table scan on soft-delete filtering
--    Replaces/supplements idx_albums_user with deleted_at coverage
CREATE INDEX IF NOT EXISTS idx_albums_user_active_created
    ON albums(user_id, created_at DESC)
    WHERE deleted_at IS NULL;

-- 3. High-speed covering index for assets active existence checks & thumbnail resolution
--    Allows SQLite to verify active assets and pull thumb_path WITHOUT touching the main table heap
CREATE INDEX IF NOT EXISTS idx_assets_active_thumb
    ON assets(id, thumb_path)
    WHERE deleted_at IS NULL;

-- 4. Allows instant resolution of user assets for joins (avoids scanning assets in dynamic filters)
CREATE INDEX IF NOT EXISTS idx_assets_user_active_id
    ON assets(user_id, id)
    WHERE deleted_at IS NULL;

-- 5. Fixes the 4.5s - 4.8s dynamic tag aggregation query
--    Provides an index-only path for: JOIN asset_tags at ON ... WHERE tag_id = t.id
CREATE INDEX IF NOT EXISTS idx_asset_tags_tag_asset
    ON asset_tags(tag_id, asset_id);

-- 6. Covers the tag grouping and name lookup
CREATE INDEX IF NOT EXISTS idx_tags_user_id_name
    ON tags(user_id, id, name);