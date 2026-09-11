use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
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
    /// Initializes both ONNX models with CPU-optimized multithreading
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

    /// Prepares an image for SCRFD (640x640 letterbox)
    /// SCRFD expects BGR format normalized with (x - 127.5) / 128.0
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

        // SCRFD expects BGR format (channel 0 = Blue, 1 = Green, 2 = Red)
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

    /// Prepares a 112x112 cropped face chip for w600k_r50.onnx.
    /// InsightFace expects RGB normalized with (x - 127.5) / 127.5
    pub fn preprocess_arcface(&self, face_chip: &DynamicImage) -> Array4<f32> {
        let resized = face_chip.resize_exact(112, 112, FilterType::Triangle);
        let rgb = resized.to_rgb8();

        let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

        for (x, y, pixel) in rgb.enumerate_pixels() {
            let px = x as usize;
            let py = y as usize;

            tensor[[0, 0, py, px]] = (pixel[0] as f32 - 127.5) / 127.5;
            tensor[[0, 1, py, px]] = (pixel[1] as f32 - 127.5) / 127.5;
            tensor[[0, 2, py, px]] = (pixel[2] as f32 - 127.5) / 127.5;
        }

        tensor
    }

    /// Runs inference with SCRFD, decodes output strides (8, 16, 32), and applies NMS
    pub fn detect_faces(
        &self,
        img: &DynamicImage,
        conf_threshold: f32,
        iou_threshold: f32,
    ) -> Result<Vec<RawDetection>, Box<dyn std::error::Error + Send + Sync>> {
        let prep = self.preprocess_scrfd(img);

        let input_tensor = Tensor::from_array(prep.tensor)
            .map_err(|e| e.to_string())?;

        let mut detector = self.detector.lock().map_err(|e| e.to_string())?;
        let outputs = detector.run(ort::inputs![input_tensor])
            .map_err(|e| e.to_string())?;

        let mut all_detections = Vec::new();
        let strides = [8, 16, 32];

        for (i, &stride) in strides.iter().enumerate() {
            let (_, score_slice) = outputs[i].try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            let (_, bbox_slice) = outputs[i + 3].try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;

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

    /// Extracts an L2-normalized 512-dim embedding with horizontal flip augmentation
    pub fn extract_embedding(
        &self,
        face_chip: &DynamicImage,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        let tensor_orig = self.preprocess_arcface(face_chip);
        let tensor_flip = self.preprocess_arcface(&face_chip.fliph());

        let raw_orig = self.run_raw_inference(tensor_orig)?;
        let raw_flip = self.run_raw_inference(tensor_flip)?;

        let mut aggregated = vec![0.0f32; 512];
        for k in 0..512 {
            aggregated[k] = raw_orig[k] + raw_flip[k];
        }

        // L2 normalization: v = v / ||v||
        let norm: f32 = aggregated.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for v in aggregated.iter_mut() {
            *v /= norm;
        }

        Ok(aggregated)
    }
}