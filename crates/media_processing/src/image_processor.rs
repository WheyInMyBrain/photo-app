use image::{DynamicImage, ImageFormat, ImageReader, RgbImage};
use libheif_rs::{ColorSpace, HeifContext, ItemId, LibHeif, RgbChroma};
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info};

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn load_image(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let img = if ext == "heic" || ext == "heif" {
            Self::load_heic(path)?
        } else {
            Self::load_raster(path, &ext)?
        };

        info!(
            target: "perf",
            "[LOAD_IMAGE] {:?} total: {:.2?} ({}x{})",
            path.file_name().unwrap_or_default(),
            start.elapsed(),
            img.width(),
            img.height()
        );

        Ok(img)
    }

    /// Loads JPEG, WebP, PNG, etc. Reads EXIF orientation ONLY for formats that carry it (e.g., JPEG).
    fn load_raster(path: &Path, ext: &str) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let t_decode_start = Instant::now();
        let decoded = ImageReader::open(path)?.with_guessed_format()?.decode()?;
        let t_decode = t_decode_start.elapsed();

        // Skip EXIF extraction for WebP previews or PNGs to prevent spurious warning logs
        let oriented = if ext == "jpg" || ext == "jpeg" {
            let t_exif_start = Instant::now();
            let res = Self::apply_exif_fallback(path, decoded);
            debug!(target: "perf", "[LOAD_RASTER] Decode: {:.2?} | Exif: {:.2?}", t_decode, t_exif_start.elapsed());
            res
        } else {
            decoded
        };

        Ok(oriented)
    }

    /// Loads HEIC using libheif and extracts EXIF metadata directly from the HEIF container items
    fn load_heic(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let t_heic_start = Instant::now();
        let lib_heif = LibHeif::new();
        let ctx = HeifContext::read_from_file(path.to_str().ok_or("Invalid path string")?)?;
        let handle = ctx.primary_image_handle()?;

        let decoded = lib_heif.decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)?;

        let width = decoded.width();
        let height = decoded.height();
        let planes = decoded.planes();
        let plane = planes.interleaved.ok_or("Missing interleaved RGB plane in HEIC")?;

        let row_bytes = (width * 3) as usize;
        let total_bytes = (width * height * 3) as usize;
        let mut rgb_buffer = vec![0u8; total_bytes];

        if plane.stride == row_bytes {
            // Contiguous slice fast-path: single memcpy
            rgb_buffer.copy_from_slice(&plane.data[..total_bytes]);
        } else {
            for row in 0..height as usize {
                let src_start = row * plane.stride;
                let src_end = src_start + row_bytes;
                let dst_start = row * row_bytes;
                let dst_end = dst_start + row_bytes;

                rgb_buffer[dst_start..dst_end].copy_from_slice(&plane.data[src_start..src_end]);
            }
        }

        let rgb_img = RgbImage::from_raw(width, height, rgb_buffer)
            .ok_or("Failed to construct RGB image buffer from HEIC")?;

        let mut img = DynamicImage::ImageRgb8(rgb_img);
        let t_decode = t_heic_start.elapsed();

        let t_exif_start = Instant::now();
        let mut meta_ids: Vec<ItemId> = vec![0; 4];
        let count = handle.metadata_block_ids(&mut meta_ids, b"Exif");

        if count > 0 {
            if let Ok(raw_meta) = handle.metadata(meta_ids[0]) {
                let payload = if raw_meta.len() > 4 && &raw_meta[0..4] == &[0, 0, 0, 0] {
                    &raw_meta[4..]
                } else if raw_meta.len() > 6 && &raw_meta[0..6] == b"Exif\0\0" {
                    &raw_meta[6..]
                } else {
                    &raw_meta[..]
                };

                let mut cursor = std::io::Cursor::new(payload);
                if let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) {
                    if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
                        if let Some(val) = field.value.get_uint(0) {
                            debug!("Applying HEIF EXIF orientation {} for {:?}", val, path);
                            img = Self::rotate_by_tag(img, val);
                        }
                    }
                }
            }
        }
        let t_exif = t_exif_start.elapsed();

        debug!(
            target: "perf",
            "[LOAD_HEIC] Decode: {:.2?} | Exif: {:.2?}",
            t_decode, t_exif
        );

        Ok(img)
    }

    fn apply_exif_fallback(path: &Path, img: DynamicImage) -> DynamicImage {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return img,
        };

        let mut bufreader = std::io::BufReader::new(file);
        let exif = match exif::Reader::new().read_from_container(&mut bufreader) {
            Ok(e) => e,
            Err(e) => {
                // Lowered from warn! to debug! so missing EXIF headers do not spam stderr
                debug!("No EXIF metadata found on {:?}: {e}", path);
                return img;
            }
        };

        if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
            if let Some(val) = field.value.get_uint(0) {
                debug!("Applying EXIF orientation tag {} for {:?}", val, path);
                return Self::rotate_by_tag(img, val);
            }
        }

        img
    }

    fn rotate_by_tag(img: DynamicImage, val: u32) -> DynamicImage {
        match val {
            2 => img.fliph(),
            3 => img.rotate180(),
            4 => img.flipv(),
            5 => img.rotate90().fliph(),
            6 => img.rotate90(),
            7 => img.rotate270().fliph(),
            8 => img.rotate270(),
            _ => img,
        }
    }

    pub fn generate_derivatives(
        img: &DynamicImage,
        asset_id: &str,
        target_shard_dir: &Path,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let total_start = Instant::now();

        let thumb_name = format!("{}_thumb.webp", asset_id);
        let preview_name = format!("{}_preview.webp", asset_id);

        let thumb_file_path = target_shard_dir.join(&thumb_name);
        let preview_file_path = target_shard_dir.join(&preview_name);

        // 1. Generate High-Res Preview first (1600px max edge)
        let t_preview_resize_start = Instant::now();
        let preview = img.thumbnail(1600, 1600);
        let t_preview_resize = t_preview_resize_start.elapsed();

        let t_preview_write_start = Instant::now();
        let mut preview_file = File::create(&preview_file_path)?;
        preview.write_to(&mut preview_file, ImageFormat::WebP)?;
        preview_file.sync_all().ok();
        drop(preview_file);
        let t_preview_write = t_preview_write_start.elapsed();

        // 2. Generate Grid Thumbnail FROM THE PREVIEW (320px)
        // Downscaling 1600 -> 320 is ~5-10x faster than downscaling 8000 -> 320
        let t_thumb_resize_start = Instant::now();
        let thumb = preview.thumbnail(320, 320);
        let t_thumb_resize = t_thumb_resize_start.elapsed();

        let t_thumb_write_start = Instant::now();
        let mut thumb_file = File::create(&thumb_file_path)?;
        thumb.write_to(&mut thumb_file, ImageFormat::WebP)?;
        thumb_file.sync_all().ok();
        drop(thumb_file);
        let t_thumb_write = t_thumb_write_start.elapsed();

        let shard = target_shard_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("00");

        info!(
            target: "perf",
            "[DERIVATIVES] [{}] Total: {:.2?} | Preview(scale: {:.2?}, enc: {:.2?}) | Thumb(scale: {:.2?}, enc: {:.2?})",
            asset_id,
            total_start.elapsed(),
            t_preview_resize,
            t_preview_write,
            t_thumb_resize,
            t_thumb_write
        );

        Ok((
            format!("{}/{}", shard, thumb_name),
            format!("{}/{}", shard, preview_name),
        ))
    }
}