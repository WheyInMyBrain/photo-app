// src/filter/mod.rs

pub mod dimensions;
pub mod grouping;
pub mod rules;

pub use dimensions::DimensionFilterConfig;
pub use grouping::deduplicate_media;
use crate::models::ExtractedMediaMetadata;

/// Applies standard filters: removes non-media MIME types, pseudo-protocols,
/// stream chunks, extreme aspect ratio banners, and deduplicates variants
pub fn apply_default_filters(meta: &mut ExtractedMediaMetadata) -> &mut ExtractedMediaMetadata {
    let dim_config = DimensionFilterConfig::default();
    apply_custom_filters(meta, &dim_config)
}

/// Applies filters with user-specified dimension thresholds and deduplication
pub fn apply_custom_filters<'a>(
    meta: &'a mut ExtractedMediaMetadata,
    config: &DimensionFilterConfig,
) -> &'a mut ExtractedMediaMetadata {
    // 1. Initial prune: Drop invalid protocols, stream chunks, non-media MIMEs, and bad aspect ratios
    meta.items.retain(|item| {
        if rules::is_junk_item(item) {
            return false;
        }
        if !dimensions::is_valid_size(item, config) {
            return false;
        }
        true
    });

    // 2. Grouping & Deduplication pass: collapse alternate resolutions into the parent MediaItem
    let raw_items = std::mem::take(&mut meta.items);
    meta.items = grouping::deduplicate_media(raw_items);

    meta
}