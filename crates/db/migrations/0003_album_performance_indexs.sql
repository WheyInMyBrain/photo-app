-- ============================================================================
-- 0003: ALBUM & VIRTUAL FOLDER PERFORMANCE INDEXES (HDD-TUNED)
-- ============================================================================

-- 1. FIX: `list_custom_albums` & `get_album_by_id` Lateral Fallback Cover & Count
-- Eliminates table scans during album count and position-ordered thumbnail discovery
CREATE INDEX IF NOT EXISTS idx_album_assets_cover_eval
    ON album_assets (album_id, position ASC, added_at DESC, asset_id);

-- 2. FIX: `get_sub_albums` Root-Level Discovery
-- Enables index-only scans for SPLIT_PART calculations on top-level folders
CREATE INDEX IF NOT EXISTS idx_assets_root_folder_split
    ON assets (user_id, (SPLIT_PART(folder_path, '/', 1)), captured_at DESC NULLS LAST, created_at DESC)
    INCLUDE (thumb_path)
    WHERE deleted_at IS NULL AND folder_path <> '' AND folder_path <> 'root';

-- 3. FIX: `get_sub_albums` Sub-Folder Prefix Traversal
-- Fast index seeks for nested prefix matching (e.g. folder_path LIKE 'Vacation/%')
CREATE INDEX IF NOT EXISTS idx_assets_folder_pattern_seek
    ON assets (user_id, folder_path varchar_pattern_ops, captured_at DESC NULLS LAST, created_at DESC)
    INCLUDE (thumb_path)
    WHERE deleted_at IS NULL;

-- 4. FIX: `get_all_folder_paths` Distinct Folder Suggestions
-- Eliminates sequential disk reads when calculating unique folder paths
CREATE INDEX IF NOT EXISTS idx_assets_user_folder_distinct
    ON assets (user_id, folder_path)
    WHERE deleted_at IS NULL AND folder_path <> '';

-- 5. FIX: Explicit Cover Asset Lookups
-- Prevents seq-scans when looking up explicit cover asset paths
CREATE INDEX IF NOT EXISTS idx_assets_id_cover_thumb
    ON assets (id)
    INCLUDE (thumb_path)
    WHERE deleted_at IS NULL;