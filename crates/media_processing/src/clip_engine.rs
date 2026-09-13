use std::path::{Path, PathBuf};
use std::sync::Mutex;
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use ndarray::{Array2, Array4};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use tokenizers::Tokenizer;

pub struct ClipEngine {
    pub vision_session: Mutex<Session>,
    pub text_session: Mutex<Session>,
    pub tokenizer: Tokenizer,
    #[allow(dead_code)]
    pub model_dir: PathBuf,
}

impl ClipEngine {
    /// Initializes both vision.onnx and text.onnx with CPU multi-threading
    pub fn init(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let clip_dir = model_dir.join("clip-vitb:32");
        let vision_path = clip_dir.join("vision.onnx");
        let text_path = clip_dir.join("text.onnx");
        let tokenizer_path = clip_dir.join("tokenizer.json");

        if !vision_path.exists() {
            return Err(format!("CLIP vision model missing at: {}", vision_path.display()).into());
        }
        if !text_path.exists() {
            return Err(format!("CLIP text model missing at: {}", text_path.display()).into());
        }
        if !tokenizer_path.exists() {
            return Err(format!("CLIP tokenizer.json missing at: {}", tokenizer_path.display()).into());
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| (n.get()).saturating_sub(1).max(1).min(8))
            .unwrap_or(4);

        let build_session = |p: &Path| -> Result<Session, Box<dyn std::error::Error + Send + Sync>> {
            let session = Session::builder()
                .map_err(|e| e.to_string())?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| e.to_string())?
                .with_intra_threads(num_threads)
                .map_err(|e| e.to_string())?
                .commit_from_file(p)
                .map_err(|e| e.to_string())?;
            Ok(session)
        };

        let vision_session = build_session(&vision_path)?;
        let text_session = build_session(&text_path)?;

        // Load offline tokenizer directly from disk
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to load CLIP tokenizer from {}: {e}", tokenizer_path.display()))?;

        Ok(Self {
            vision_session: Mutex::new(vision_session),
            text_session: Mutex::new(text_session),
            tokenizer,
            model_dir: model_dir.to_path_buf(),
        })
    }

    // -------------------------------------------------------------------------
    // Visual Pipeline (vision.onnx)
    // -------------------------------------------------------------------------

    pub fn extract_image_embedding(
        &self,
        img: &DynamicImage,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let (w, h) = img.dimensions();

        let (target_w, target_h) = if w < h {
            let scale = 224.0 / w as f32;
            (224, (h as f32 * scale).round() as u32)
        } else {
            let scale = 224.0 / h as f32;
            ((w as f32 * scale).round() as u32, 224)
        };

        let resized = img.resize_exact(target_w, target_h, FilterType::CatmullRom);

        let crop_x = (target_w.saturating_sub(224)) / 2;
        let crop_y = (target_h.saturating_sub(224)) / 2;
        let cropped = resized.crop_imm(crop_x, crop_y, 224, 224).to_rgb8();

        let mean = [0.48145466f32, 0.4578275, 0.40821073];
        let std = [0.26862954f32, 0.26130258, 0.27577711];

        let mut input = Array4::<f32>::zeros((1, 3, 224, 224));
        for y in 0..224 {
            for x in 0..224 {
                let pixel = cropped.get_pixel(x, y);
                input[[0, 0, y as usize, x as usize]] =
                    ((pixel[0] as f32 / 255.0) - mean[0]) / std[0];
                input[[0, 1, y as usize, x as usize]] =
                    ((pixel[1] as f32 / 255.0) - mean[1]) / std[1];
                input[[0, 2, y as usize, x as usize]] =
                    ((pixel[2] as f32 / 255.0) - mean[2]) / std[2];
            }
        }

        let input_tensor = Tensor::from_array(input).map_err(|e| e.to_string())?;

        let mut session = self.vision_session.lock().map_err(|e| e.to_string())?;
        let outputs = session.run(ort::inputs![
            "pixel_values" => input_tensor
        ]).map_err(|e| e.to_string())?;

        let (_, slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;

        let raw_output: Vec<f32> = slice.iter().take(512).copied().collect();

        // L2 Unit Normalization
        let norm = raw_output.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        let embedding: Vec<f32> = raw_output.iter().map(|x| x / norm).collect();

        Ok(embedding)
    }

    pub fn extract_video_embedding(
        &self,
        frames: &[DynamicImage],
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        if frames.is_empty() {
            return Err("No frames provided for video embedding".into());
        }

        let mut accumulated = vec![0.0f32; 512];
        let mut count = 0.0f32;

        for frame in frames {
            if let Ok(emb) = self.extract_image_embedding(frame) {
                for i in 0..512 {
                    accumulated[i] += emb[i];
                }
                count += 1.0;
            }
        }

        if count == 0.0 {
            return Err("All frame extractions failed".into());
        }

        let mut final_vec: Vec<f32> = accumulated.into_iter().map(|x| x / count).collect();
        let norm = final_vec.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for v in final_vec.iter_mut() {
            *v /= norm;
        }

        Ok(final_vec)
    }

    // -------------------------------------------------------------------------
    // Text Pipeline (text.onnx)
    // -------------------------------------------------------------------------

    /// Tokenizes prompt, runs text.onnx, and returns an L2-normalized 512-dim embedding
    pub fn extract_text_embedding(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| format!("Tokenization failed: {e}"))?;

        let mut token_ids = encoding.get_ids().to_vec();
        let mut mask = encoding.get_attention_mask().to_vec();

        // Standard CLIP sequence length is fixed at 77 tokens
        const CONTEXT_LENGTH: usize = 77;
        token_ids.truncate(CONTEXT_LENGTH);
        mask.truncate(CONTEXT_LENGTH);

        while token_ids.len() < CONTEXT_LENGTH {
            token_ids.push(0); // Pad token ID
            mask.push(0);      // Mask out pad
        }

        let mut input_ids = Array2::<i64>::zeros((1, CONTEXT_LENGTH));
        let mut attention_mask = Array2::<i64>::zeros((1, CONTEXT_LENGTH));

        for i in 0..CONTEXT_LENGTH {
            input_ids[[0, i]] = token_ids[i] as i64;
            attention_mask[[0, i]] = mask[i] as i64;
        }

        let ids_tensor = Tensor::from_array(input_ids).map_err(|e| e.to_string())?;
        let mask_tensor = Tensor::from_array(attention_mask).map_err(|e| e.to_string())?;

        let mut session = self.text_session.lock().map_err(|e| e.to_string())?;

        // Most CLIP text ONNX exports accept input_ids and attention_mask
        let outputs = session.run(ort::inputs![
            "input_ids" => ids_tensor,
            "attention_mask" => mask_tensor
        ]).map_err(|e| e.to_string())?;

        let (_, slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;

        let raw_output: Vec<f32> = slice.iter().take(512).copied().collect();

        // L2 Normalization (ensures dot-product with vision embeddings equals cosine similarity)
        let norm = raw_output.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        let embedding: Vec<f32> = raw_output.iter().map(|x| x / norm).collect();

        Ok(embedding)
    }
}