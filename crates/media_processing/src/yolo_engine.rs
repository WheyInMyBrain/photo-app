// photo-app/crates/media_processing/src/yolo_engine.rs
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use ndarray::Array4;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use crate::models::DetectedPose;
use crate::yolo_detector::YoloPostProcessor;

pub struct YoloLetterbox {
    pub tensor: Array4<f32>,
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub original_w: u32,
    pub original_h: u32,
}

#[derive(Clone)]
pub struct YoloEngine {
    pub pose_session: Option<Arc<Mutex<Session>>>,
    #[allow(dead_code)]
    pub model_dir: PathBuf,
}

impl YoloEngine {
    pub fn init(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pose_path = model_dir.join("yolo").join("yolo26x-pose.onnx");

        let num_threads = std::thread::available_parallelism()
            .map(|n| (n.get()).saturating_sub(1).max(1).min(8))
            .unwrap_or(4);

        let build_session = |path: &Path| -> Result<Session, Box<dyn std::error::Error + Send + Sync>> {
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

        let pose_session = if pose_path.exists() {
            Some(Arc::new(Mutex::new(build_session(&pose_path)?)))
        } else {
            None
        };

        if pose_session.is_none() {
            return Err(format!(
                "Pose model not found in: {}",
                model_dir.join("yolo").display()
            )
            .into());
        }

        Ok(Self {
            pose_session,
            model_dir: model_dir.to_path_buf(),
        })
    }

    /// Standard YOLO 640x640 letterbox with normalized [0.0..1.0] RGB channels.
    /// Uses exact integer-aligned offsets for both rasterization and metadata unpadding.
    pub fn preprocess_yolo(&self, img: &DynamicImage) -> YoloLetterbox {
        let (orig_w, orig_h) = img.dimensions();
        let target_size = 640.0f32;

        if orig_w == 0 || orig_h == 0 {
            return YoloLetterbox {
                tensor: Array4::<f32>::zeros((1, 3, 640, 640)),
                scale: 1.0,
                pad_x: 0.0,
                pad_y: 0.0,
                original_w: 1,
                original_h: 1,
            };
        }

        let scale = (target_size / orig_w as f32).min(target_size / orig_h as f32);
        let new_w = ((orig_w as f32 * scale).round() as u32).min(640).max(1);
        let new_h = ((orig_h as f32 * scale).round() as u32).min(640).max(1);

        let resized = img.resize_exact(new_w, new_h, FilterType::Triangle);
        let rgb_resized = resized.to_rgb8();

        let pad_x_int = ((640 - new_w) / 2) as usize;
        let pad_y_int = ((640 - new_h) / 2) as usize;

        let mut tensor = Array4::<f32>::from_elem((1, 3, 640, 640), 114.0 / 255.0);

        for (x, y, pixel) in rgb_resized.enumerate_pixels() {
            let target_x = pad_x_int + x as usize;
            let target_y = pad_y_int + y as usize;

            if target_x < 640 && target_y < 640 {
                tensor[[0, 0, target_y, target_x]] = pixel[0] as f32 / 255.0;
                tensor[[0, 1, target_y, target_x]] = pixel[1] as f32 / 255.0;
                tensor[[0, 2, target_y, target_x]] = pixel[2] as f32 / 255.0;
            }
        }

        YoloLetterbox {
            tensor,
            scale,
            pad_x: pad_x_int as f32,
            pad_y: pad_y_int as f32,
            original_w: orig_w,
            original_h: orig_h,
        }
    }

    pub fn detect_poses(
        &self,
        img: &DynamicImage,
        conf_threshold: f32,
        iou_threshold: f32,
    ) -> Result<Vec<DetectedPose>, Box<dyn std::error::Error + Send + Sync>> {
        let session_arc = match &self.pose_session {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let prep = self.preprocess_yolo(img);
        let input_tensor = Tensor::from_array(prep.tensor).map_err(|e| e.to_string())?;

        let mut session = session_arc.lock().map_err(|e| e.to_string())?;
        let outputs = session.run(ort::inputs![input_tensor]).map_err(|e| e.to_string())?;

        let (shape, raw_slice) = outputs[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;

        let raw_poses = YoloPostProcessor::decode_pose_output(
            raw_slice,
            &shape,
            conf_threshold,
            prep.scale,
            prep.pad_x,
            prep.pad_y,
            prep.original_w,
            prep.original_h,
        );

        let filtered = YoloPostProcessor::nms_poses(raw_poses, iou_threshold);
        Ok(filtered)
    }
}