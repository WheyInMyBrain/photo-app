use image::{imageops::FilterType, ImageReader, DynamicImage, ImageFormat, RgbImage};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
use std::path::Path;

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn load_image(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "heic" || ext == "heif" {
            Self::load_heic(path)
        } else {
            let img = ImageReader::open(path)?
                .with_guessed_format()?
                .decode()?;
            Ok(img)
        }
    }

    fn load_heic(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
        let lib_heif = LibHeif::new();
        let ctx = HeifContext::read_from_file(path.to_str().unwrap())?;
        let handle = ctx.primary_image_handle()?;
        
        let decoded = lib_heif.decode(
            &handle,
            ColorSpace::Rgb(RgbChroma::Rgb),
            None,
        )?;

        let width = decoded.width();
        let height = decoded.height();
        let planes = decoded.planes();
        let plane = planes.interleaved.ok_or("Missing interleaved RGB plane in HEIC")?;

        // Account for row stride (padding) if any
        let mut rgb_buffer = Vec::with_capacity((width * height * 3) as usize);
        for row in 0..height as usize {
            let start = row * plane.stride;
            let end = start + (width as usize * 3);
            rgb_buffer.extend_from_slice(&plane.data[start..end]);
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

        // 1. Generate 320px thumbnail
        let thumb = img.thumbnail(320, 320);
        let mut thumb_file = std::fs::File::create(&thumb_file_path)?;
        thumb.write_to(&mut thumb_file, ImageFormat::WebP)?;

        // 2. Generate 1600px preview
        let preview = img.resize(1600, 1600, FilterType::Triangle);
        let mut preview_file = std::fs::File::create(&preview_file_path)?;
        preview.write_to(&mut preview_file, ImageFormat::WebP)?;

        // Extract shard folder name (e.g. "05")
        let shard = target_shard_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("00");

        // Store uniform relative path for the API: "thumbs/05/<id>_thumb.webp"
        let rel_thumb = format!("thumbs/{}/{}", shard, thumb_name);
        let rel_preview = format!("thumbs/{}/{}", shard, preview_name);

        Ok((rel_thumb, rel_preview))
    }
}