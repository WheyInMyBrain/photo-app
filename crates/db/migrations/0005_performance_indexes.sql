-- Partial indexes for instant photo/video tab switching
CREATE INDEX IF NOT EXISTS idx_assets_user_videos_active 
    ON assets(user_id, id) 
    WHERE duration_seconds IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_assets_user_photos_active 
    ON assets(user_id, id) 
    WHERE duration_seconds IS NULL AND deleted_at IS NULL;

-- Covering index for user tag lookups
CREATE INDEX IF NOT EXISTS idx_tags_user_id_covering 
    ON tags(user_id, id, name);

-- Covering index for faces lookup
CREATE INDEX IF NOT EXISTS idx_asset_faces_asset_person
    ON asset_faces(asset_id, person_id);