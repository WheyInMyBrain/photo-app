use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use image::{imageops::FilterType, DynamicImage, GenericImageView, Rgb, RgbImage};
use ndarray::Array4;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    pub category: i32, // 0 = general, 4 = character, 9 = rating
}

#[derive(Clone)]
pub struct TagEngine {
    pub session: Arc<Mutex<Session>>,
    pub tags: Arc<Vec<TagInfo>>,
    pub input_size: u32,
    pub input_name: String,
    pub is_nhwc: bool,
}

impl TagEngine {
    pub fn init(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let tagger_dir = model_dir.join("wd-tagger");
        let model_path = tagger_dir.join("model.onnx");
        let tags_csv_path = tagger_dir.join("selected_tags.csv");

        if !model_path.exists() {
            return Err(format!("WD Tagger model missing at: {}", model_path.display()).into());
        }
        if !tags_csv_path.exists() {
            return Err(format!("selected_tags.csv missing at: {}", tags_csv_path.display()).into());
        }

        let file = File::open(&tags_csv_path)?;
        let mut rdr = csv::Reader::from_reader(file);
        let mut tags = Vec::new();

        for result in rdr.records() {
            let record = result?;
            if record.len() >= 3 {
                let raw_name = record[1].to_string();
                let clean_name = raw_name.replace('_', " ");
                let category: i32 = record[2].parse().unwrap_or(0);

                tags.push(TagInfo {
                    name: clean_name,
                    category,
                });
            }
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| (n.get()).saturating_sub(1).max(1).min(8))
            .unwrap_or(4);

        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .with_intra_threads(num_threads)
            .map_err(|e| e.to_string())?
            .commit_from_file(&model_path)
            .map_err(|e| e.to_string())?;

        // Inspect input node safely using session.inputs() method
        let inputs = session.inputs();
        let first_input = inputs.first().ok_or("No input nodes found in ONNX graph")?;
        let input_name = first_input.name().to_string();

        // WD-Tagger ONNX exports use 448x448 NHWC [1, 448, 448, 3] by default
        let input_size = 448u32;
        let is_nhwc = true;

        tracing::info!(
            "WD Tagger initialized: {} tags loaded. Target size: {}x{}, input: '{}'",
            tags.len(),
            input_size,
            input_size,
            input_name
        );

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tags: Arc::new(tags),
            input_size,
            input_name,
            is_nhwc,
        })
    }

    pub fn preprocess_image(&self, img: &DynamicImage) -> Array4<f32> {
        let (w, h) = img.dimensions();
        let max_dim = w.max(h);

        let mut square = RgbImage::from_pixel(max_dim, max_dim, Rgb([255, 255, 255]));
        let offset_x = (max_dim - w) / 2;
        let offset_y = (max_dim - h) / 2;

        let rgb_img = img.to_rgb8();
        image::imageops::overlay(&mut square, &rgb_img, offset_x as i64, offset_y as i64);

        let resized = image::imageops::resize(
            &square,
            self.input_size,
            self.input_size,
            FilterType::Triangle,
        );

        let s = self.input_size as usize;
        if self.is_nhwc {
            let mut tensor = Array4::<f32>::zeros((1, s, s, 3));
            for (x, y, pixel) in resized.enumerate_pixels() {
                // RGB -> BGR [0.0, 255.0]
                tensor[[0, y as usize, x as usize, 0]] = pixel[2] as f32;
                tensor[[0, y as usize, x as usize, 1]] = pixel[1] as f32;
                tensor[[0, y as usize, x as usize, 2]] = pixel[0] as f32;
            }
            tensor
        } else {
            let mut tensor = Array4::<f32>::zeros((1, 3, s, s));
            for (x, y, pixel) in resized.enumerate_pixels() {
                tensor[[0, 0, y as usize, x as usize]] = pixel[2] as f32;
                tensor[[0, 1, y as usize, x as usize]] = pixel[1] as f32;
                tensor[[0, 2, y as usize, x as usize]] = pixel[0] as f32;
            }
            tensor
        }
    }

    pub fn tag_image(
        &self,
        img: &DynamicImage,
        threshold: f32,
    ) -> Result<Vec<(String, f32)>, Box<dyn std::error::Error + Send + Sync>> {
        let input_tensor_data = self.preprocess_image(img);
        let tensor = Tensor::from_array(input_tensor_data).map_err(|e| e.to_string())?;

        let mut session = self.session.lock().map_err(|e| e.to_string())?;
        let outputs = session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .map_err(|e| e.to_string())?;

        let (_, probs_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;

        let needs_sigmoid = probs_slice.iter().any(|&v| v < 0.0 || v > 1.0);

        let mut detected_tags = Vec::new();
        for (i, tag) in self.tags.iter().enumerate() {
            if i >= probs_slice.len() {
                break;
            }

            // Exclude rating categories (category 9)
            if tag.category == 9 {
                continue;
            }

            let raw = probs_slice[i];
            let prob = if needs_sigmoid {
                1.0 / (1.0 + (-raw).exp())
            } else {
                raw
            };

            if prob >= threshold {
                detected_tags.push((tag.name.clone(), prob));
            }
        }

        detected_tags.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(detected_tags)
    }
}