// src/filter/dimensions.rs

use crate::models::{MediaItem, MediaType};

/// Configuration for dimension, area, and aspect ratio checks
#[derive(Debug, Clone)]
pub struct DimensionFilterConfig {
    /// Minimum width allowed (default: 200px)
    pub min_width: usize,
    /// Minimum height allowed (default: 200px)
    pub min_height: usize,
    /// Minimum total area (width * height). Default: 60,000 (~245x245)
    pub min_area: usize,
    /// Minimum width-to-height ratio (rejects tall skinny stripes, borders, vertical dividers)
    pub min_aspect_ratio: f64,
    /// Maximum width-to-height ratio (rejects thin horizontal lines, banner ribbons)
    pub max_aspect_ratio: f64,
    /// If true, unprobed assets (dimensions == None) will be preserved
    pub keep_unprobed: bool,
}

impl Default for DimensionFilterConfig {
    fn default() -> Self {
        Self {
            min_width: 200,
            min_height: 200,
            min_area: 60_000,
            min_aspect_ratio: 0.15,
            max_aspect_ratio: 6.5,
            keep_unprobed: true,
        }
    }
}

/// Checks if a media item passes dimension, area, and aspect ratio constraints
pub fn is_valid_size(item: &MediaItem, config: &DimensionFilterConfig) -> bool {
    // Videos usually report None initially unless parsed from HLS variant lines
    if item.media_type == MediaType::Video {
        if let Some(ref dims) = item.dimensions {
            return dims.width >= 120 && dims.height >= 120;
        }
        return true;
    }

    match &item.dimensions {
        Some(dims) => {
            if dims.width == 0 || dims.height == 0 {
                return false;
            }

            let width_ok = dims.width >= config.min_width;
            let height_ok = dims.height >= config.min_height;
            let area_ok = (dims.width * dims.height) >= config.min_area;

            let ratio = dims.width as f64 / dims.height as f64;
            let ratio_ok = ratio >= config.min_aspect_ratio && ratio <= config.max_aspect_ratio;

            width_ok && height_ok && area_ok && ratio_ok
        }
        None => config.keep_unprobed,
    }
}