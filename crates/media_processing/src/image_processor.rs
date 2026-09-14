use image::{imageops::FilterType, DynamicImage, ImageFormat, ImageReader, RgbImage};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
use std::fs::File;
use std::path::Path;

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn load_image(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut img = if ext == "heic" || ext == "heif" {
            Self::load_heic(path)?
        } else {
            ImageReader::open(path)?
                .with_guessed_format()?
                .decode()?
        };

        // Fix EXIF orientation (handles portrait photos taken on phones)
        img = Self::apply_exif_orientation(path, img);

        Ok(img)
    }

    fn apply_exif_orientation(path: &Path, img: DynamicImage) -> DynamicImage {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return img,
        };

        let mut bufreader = std::io::BufReader::new(file);
        let exifreader = exif::Reader::new();
        let exif = match exifreader.read_from_container(&mut bufreader) {
            Ok(e) => e,
            Err(_) => return img,
        };

        if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
            if let Some(val) = field.value.get_uint(0) {
                return match val {
                    2 => img.fliph(),
                    3 => img.rotate180(),
                    4 => img.flipv(),
                    5 => img.rotate90().fliph(),
                    6 => img.rotate90(),
                    7 => img.rotate270().fliph(),
                    8 => img.rotate270(),
                    _ => img,
                };
            }
        }
        img
    }

    fn load_heic(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let lib_heif = LibHeif::new();
        let ctx = HeifContext::read_from_file(path.to_str().ok_or("Invalid path string")?)?;
        let handle = ctx.primary_image_handle()?;

        let decoded = lib_heif.decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)?;

        let width = decoded.width();
        let height = decoded.height();
        let planes = decoded.planes();
        let plane = planes.interleaved.ok_or("Missing interleaved RGB plane in HEIC")?;

        let row_bytes = (width * 3) as usize;
        let mut rgb_buffer = vec![0u8; (width * height * 3) as usize];

        // Direct block copy per row (avoids dynamic Vec reallocation)
        for row in 0..height as usize {
            let src_start = row * plane.stride;
            let src_end = src_start + row_bytes;
            let dst_start = row * row_bytes;
            let dst_end = dst_start + row_bytes;

            rgb_buffer[dst_start..dst_end].copy_from_slice(&plane.data[src_start..src_end]);
        }

        let img = RgbImage::from_raw(width, height, rgb_buffer)
            .ok_or("Failed to construct RGB image buffer from HEIC")?;

        Ok(DynamicImage::ImageRgb8(img))
    }

    pub fn generate_derivatives(
        img: &DynamicImage,
        asset_id: &str,
        target_shard_dir: &Path,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let thumb_name = format!("{}_thumb.webp", asset_id);
        let preview_name = format!("{}_preview.webp", asset_id);

        let thumb_file_path = target_shard_dir.join(&thumb_name);
        let preview_file_path = target_shard_dir.join(&preview_name);

        // 1. Grid Thumbnail (320px)
        let thumb = img.thumbnail(320, 320);
        let mut thumb_file = File::create(&thumb_file_path)?;
        thumb.write_to(&mut thumb_file, ImageFormat::WebP)?;
        thumb_file.sync_all().ok(); // Durability guarantee
        drop(thumb_file);

        // 2. High-res Preview (1600px with fast Triangle filter)
        let preview = img.resize(1600, 1600, FilterType::Triangle);
        let mut preview_file = File::create(&preview_file_path)?;
        preview.write_to(&mut preview_file, ImageFormat::WebP)?;
        preview_file.sync_all().ok(); // Durability guarantee
        drop(preview_file);

        let shard = target_shard_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("00");

        // Returns shard-relative path, e.g. "89/89c18dd9_thumb.webp"
        Ok((
            format!("{}/{}", shard, thumb_name),
            format!("{}/{}", shard, preview_name),
        ))
    }
}