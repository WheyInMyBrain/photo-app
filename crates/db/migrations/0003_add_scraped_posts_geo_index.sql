CREATE INDEX IF NOT EXISTS idx_scraped_posts_geo
    ON scraped_posts(user_id, latitude, longitude)
    WHERE latitude IS NOT NULL AND longitude IS NOT NULL;