ALTER TABLE scraped_media_items ADD COLUMN caption TEXT;
ALTER TABLE scraped_media_items ADD COLUMN published_at TEXT;
ALTER TABLE scraped_media_items ADD COLUMN location_name TEXT;
ALTER TABLE scraped_media_items ADD COLUMN latitude REAL;
ALTER TABLE scraped_media_items ADD COLUMN longitude REAL;
ALTER TABLE scraped_media_items ADD COLUMN tags TEXT;
ALTER TABLE scraped_media_items ADD COLUMN source_url TEXT;