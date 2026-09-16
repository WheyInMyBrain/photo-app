use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use image::{imageops::FilterType, DynamicImage, GenericImageView, RgbImage};
use ndarray::Array4;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use super::face_detector::{RawDetection, ScrfdPostProcessor};

pub struct PreprocessedLetterbox {
    pub tensor: Array4<f32>,
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub original_w: u32,
    pub original_h: u32,
}

#[derive(Clone)]
pub struct FaceEngine {
    pub detector: Arc<Mutex<Session>>,
    pub recognizer: Arc<Mutex<Session>>,
    #[allow(dead_code)]
    pub model_dir: PathBuf,
}

impl FaceEngine {
    pub fn init(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let detector_path = model_dir.join("10g_bnkps.onnx");
        let recognizer_path = model_dir.join("w600k_r50.onnx");

        if !detector_path.exists() {
            return Err(format!("Detector model missing at: {}", detector_path.display()).into());
        }
        if !recognizer_path.exists() {
            return Err(format!("Recognizer model missing at: {}", recognizer_path.display()).into());
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| (n.get()).saturating_sub(1).max(1).min(8))
            .unwrap_or(4);

        let build_cpu_session = |path: &Path| -> Result<Session, Box<dyn std::error::Error + Send + Sync>> {
            let session = Session::builder()
                .map_err(|e| e.to_string())?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| e.to_string())?
                .with_intra_threads(num_threads)
                .map_err(|e| e.to_string())?
                .commit_from_file(path)
                .map_err(|e| e.to_string())?;
            Ok(session)
        };

        let detector = build_cpu_session(&detector_path)?;
        let recognizer = build_cpu_session(&recognizer_path)?;

        Ok(Self {
            detector: Arc::new(Mutex::new(detector)),
            recognizer: Arc::new(Mutex::new(recognizer)),
            model_dir: model_dir.to_path_buf(),
        })
    }

    pub fn preprocess_scrfd(&self, img: &DynamicImage) -> PreprocessedLetterbox {
        let (orig_w, orig_h) = img.dimensions();
        let target_size = 640.0f32;

        let scale = (target_size / orig_w as f32).min(target_size / orig_h as f32);
        let new_w = (orig_w as f32 * scale).round() as u32;
        let new_h = (orig_h as f32 * scale).round() as u32;

        let resized = img.resize_exact(new_w, new_h, FilterType::Triangle);
        let rgb_resized = resized.to_rgb8();

        let pad_x = (target_size - new_w as f32) / 2.0;
        let pad_y = (target_size - new_h as f32) / 2.0;

        let pad_x_int = pad_x.round() as usize;
        let pad_y_int = pad_y.round() as usize;

        let mut tensor = Array4::<f32>::zeros((1, 3, 640, 640));

        for (x, y, pixel) in rgb_resized.enumerate_pixels() {
            let target_x = pad_x_int + x as usize;
            let target_y = pad_y_int + y as usize;

            if target_x < 640 && target_y < 640 {
                let r = pixel[0] as f32;
                let g = pixel[1] as f32;
                let b = pixel[2] as f32;

                tensor[[0, 0, target_y, target_x]] = (b - 127.5) / 128.0;
                tensor[[0, 1, target_y, target_x]] = (g - 127.5) / 128.0;
                tensor[[0, 2, target_y, target_x]] = (r - 127.5) / 128.0;
            }
        }

        PreprocessedLetterbox {
            tensor,
            scale,
            pad_x,
            pad_y,
            original_w: orig_w,
            original_h: orig_h,
        }
    }

    /// Aligns face using 5 landmarks to canonical 112x112 ArcFace template
    pub fn align_face_112(
        img: &DynamicImage,
        landmarks: &[(f32, f32)],
    ) -> DynamicImage {
        const TEMPLATE: [(f32, f32); 5] = [
            (38.2946, 51.6963), // Left eye
            (73.5318, 51.5014), // Right eye
            (56.0252, 71.7366), // Nose tip
            (41.5493, 92.3655), // Left mouth
            (70.7299, 92.2041), // Right mouth
        ];

        // Fallback: If not enough landmarks, return an explicit 112x112 buffer
        if landmarks.len() < 5 {
            let resized = img.resize_exact(112, 112, FilterType::Triangle);
            return DynamicImage::ImageRgb8(resized.to_rgb8());
        }

        // 1. Compute means
        let (mut mean_src_x, mut mean_src_y) = (0.0f32, 0.0f32);
        let (mut mean_dst_x, mut mean_dst_y) = (0.0f32, 0.0f32);

        for i in 0..5 {
            mean_src_x += landmarks[i].0;
            mean_src_y += landmarks[i].1;
            mean_dst_x += TEMPLATE[i].0;
            mean_dst_y += TEMPLATE[i].1;
        }
        mean_src_x /= 5.0; mean_src_y /= 5.0;
        mean_dst_x /= 5.0; mean_dst_y /= 5.0;

        // 2. Umeyama estimation for similarity transform
        let (mut var_src, mut cov_xx, mut cov_xy, mut cov_yx, mut cov_yy) = 
            (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            
        for i in 0..5 {
            let sx = landmarks[i].0 - mean_src_x;
            let sy = landmarks[i].1 - mean_src_y;
            let dx = TEMPLATE[i].0 - mean_dst_x;
            let dy = TEMPLATE[i].1 - mean_dst_y;

            var_src += sx * sx + sy * sy;
            cov_xx += dx * sx;
            cov_xy += dx * sy;
            cov_yx += dy * sx;
            cov_yy += dy * sy;
        }

        if var_src < 1e-6 {
            let resized = img.resize_exact(112, 112, FilterType::Triangle);
            return DynamicImage::ImageRgb8(resized.to_rgb8());
        }

        let a = (cov_xx + cov_yy) / var_src;
        let b = (cov_xy - cov_yx) / var_src;

        let tx = mean_dst_x - (a * mean_src_x - b * mean_src_y);
        let ty = mean_dst_y - (b * mean_src_x + a * mean_src_y);

        let det = a * a + b * b;
        if det < 1e-6 {
            let resized = img.resize_exact(112, 112, FilterType::Triangle);
            return DynamicImage::ImageRgb8(resized.to_rgb8());
        }

        let inv_a = a / det;
        let inv_b = -b / det;

        let (orig_w, orig_h) = img.dimensions();
        let rgb = img.to_rgb8();
        let mut aligned = RgbImage::new(112, 112);

        // 3. Bilinear backward warp (Strictly 0..112)
        for out_y in 0..112 {
            for out_x in 0..112 {
                let shifted_x = out_x as f32 - tx;
                let shifted_y = out_y as f32 - ty;

                let src_x = inv_a * shifted_x - inv_b * shifted_y;
                let src_y = inv_b * shifted_x + inv_a * shifted_y;

                if src_x >= 0.0 && src_x < (orig_w - 1) as f32 && src_y >= 0.0 && src_y < (orig_h - 1) as f32 {
                    let x0 = src_x.floor() as u32;
                    let y0 = src_y.floor() as u32;
                    let x1 = x0 + 1;
                    let y1 = y0 + 1;

                    let dx = src_x - x0 as f32;
                    let dy = src_y - y0 as f32;

                    let p00 = rgb.get_pixel(x0, y0);
                    let p10 = rgb.get_pixel(x1, y0);
                    let p01 = rgb.get_pixel(x0, y1);
                    let p11 = rgb.get_pixel(x1, y1);

                    let mut pixel = [0u8; 3];
                    for c in 0..3 {
                        let top = (1.0 - dx) * p00[c] as f32 + dx * p10[c] as f32;
                        let bottom = (1.0 - dx) * p01[c] as f32 + dx * p11[c] as f32;
                        pixel[c] = ((1.0 - dy) * top + dy * bottom).clamp(0.0, 255.0) as u8;
                    }
                    aligned.put_pixel(out_x, out_y, image::Rgb(pixel));
                }
            }
        }

        DynamicImage::ImageRgb8(aligned)
    }

    /// Prepares a 112x112 aligned face chip for w600k_r50.onnx
    pub fn preprocess_arcface(&self, aligned_face: &DynamicImage) -> Array4<f32> {
        // Enforce exact 112x112 constraint regardless of input variant
        let resized = if aligned_face.width() != 112 || aligned_face.height() != 112 {
            aligned_face.resize_exact(112, 112, FilterType::Nearest)
        } else {
            aligned_face.clone()
        };

        let rgb = resized.to_rgb8();
        let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

        for (x, y, pixel) in rgb.enumerate_pixels() {
            let px = x as usize;
            let py = y as usize;

            // Guard against out-of-bounds indexing
            if px < 112 && py < 112 {
                tensor[[0, 0, py, px]] = (pixel[0] as f32 - 127.5) / 127.5;
                tensor[[0, 1, py, px]] = (pixel[1] as f32 - 127.5) / 127.5;
                tensor[[0, 2, py, px]] = (pixel[2] as f32 - 127.5) / 127.5;
            }
        }

        tensor
    }

    pub fn detect_faces(
        &self,
        img: &DynamicImage,
        conf_threshold: f32,
        iou_threshold: f32,
    ) -> Result<Vec<RawDetection>, Box<dyn std::error::Error + Send + Sync>> {
        let prep = self.preprocess_scrfd(img);

        let input_tensor = Tensor::from_array(prep.tensor).map_err(|e| e.to_string())?;
        let mut detector = self.detector.lock().map_err(|e| e.to_string())?;
        let outputs = detector.run(ort::inputs![input_tensor]).map_err(|e| e.to_string())?;

        let mut all_detections = Vec::new();
        let strides = [8, 16, 32];

        for (i, &stride) in strides.iter().enumerate() {
            let (_, score_slice) = outputs[i].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            let (_, bbox_slice) = outputs[i + 3].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;

            let kps_slice = if (i + 6) < outputs.len() {
                outputs[i + 6].try_extract_tensor::<f32>().ok().map(|(_, s)| s)
            } else {
                None
            };

            let stride_detections = ScrfdPostProcessor::decode_stride(
                score_slice,
                bbox_slice,
                kps_slice,
                stride,
                conf_threshold,
                prep.scale,
                prep.pad_x,
                prep.pad_y,
                prep.original_w,
                prep.original_h,
            );

            all_detections.extend(stride_detections);
        }

        let filtered = ScrfdPostProcessor::nms(all_detections, iou_threshold);
        Ok(filtered)
    }

    fn run_raw_inference(&self, tensor: Array4<f32>) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let input_tensor = Tensor::from_array(tensor).map_err(|e| e.to_string())?;
        let mut recognizer = self.recognizer.lock().map_err(|e| e.to_string())?;
        let outputs = recognizer.run(ort::inputs![input_tensor]).map_err(|e| e.to_string())?;
        let (_, slice) = outputs[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(slice.to_vec())
    }

    pub fn extract_embedding(
        &self,
        aligned_face: &DynamicImage,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let tensor_orig = self.preprocess_arcface(aligned_face);
        let tensor_flip = self.preprocess_arcface(&aligned_face.fliph());

        let raw_orig = self.run_raw_inference(tensor_orig)?;
        let raw_flip = self.run_raw_inference(tensor_flip)?;

        let mut aggregated = vec![0.0f32; 512];
        for k in 0..512 {
            aggregated[k] = raw_orig[k] + raw_flip[k];
        }

        let norm: f32 = aggregated.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for v in aggregated.iter_mut() {
            *v /= norm;
        }

        Ok(aggregated)
    }
}