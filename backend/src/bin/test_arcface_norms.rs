use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage};
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

#[inline]
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-6)
}

fn run_norm(
    session: &mut Session,
    img: &DynamicImage,
    mode: &str,
) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

    for (x, y, pixel) in rgb.enumerate_pixels() {
        let (r, g, b) = (pixel[0] as f32, pixel[1] as f32, pixel[2] as f32);
        let (v0, v1, v2) = match mode {
            // Mode A: [-1, 1] standard (x - 127.5) / 127.5
            "norm_symmetric" => ((r - 127.5) / 127.5, (g - 127.5) / 127.5, (b - 127.5) / 127.5),
            // Mode B: [0, 1] float
            "norm_unit" => (r / 255.0, g / 255.0, b / 255.0),
            // Mode C: Raw [0, 255] float
            "raw_255" => (r, g, b),
            // Mode D: ImageNet standardization
            "imagenet" => (
                (r / 255.0 - 0.485) / 0.229,
                (g / 255.0 - 0.456) / 0.224,
                (b / 255.0 - 0.406) / 0.225,
            ),
            // Mode E: BGR [-1, 1]
            "bgr_symmetric" => ((b - 127.5) / 127.5, (g - 127.5) / 127.5, (r - 127.5) / 127.5),
            _ => (r, g, b),
        };
        tensor[[0, 0, y as usize, x as usize]] = v0;
        tensor[[0, 1, y as usize, x as usize]] = v1;
        tensor[[0, 2, y as usize, x as usize]] = v2;
    }

    let ort_tensor = Tensor::from_array(tensor).unwrap();
    let outputs = session.run(ort::inputs![ort_tensor]).unwrap();
    let (_, slice) = outputs[0].try_extract_tensor::<f32>().unwrap();
    slice.to_vec()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_path = if Path::new("storage/models/arcfaceresnet100-8.onnx").exists() {
        PathBuf::from("storage/models/arcfaceresnet100-8.onnx")
    } else {
        PathBuf::from("../storage/models/arcfaceresnet100-8.onnx")
    };

    println!("Loading ArcFace model: {:?}", model_path);
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&model_path)?;

    let face1 = image::open("diag_output/img1_face_1.jpg")?;
    let face2 = image::open("diag_output/img1_face_2.jpg")?;

    let modes = ["norm_symmetric", "norm_unit", "raw_255", "imagenet", "bgr_symmetric"];

    println!("\n=== Testing Similarity Between 2 Different People Across Normalizations ===");
    for &mode in &modes {
        let e1 = run_norm(&mut session, &face1, mode);
        let e2 = run_norm(&mut session, &face2, mode);
        let sim = cosine(&e1, &e2);
        println!("Mode: {:<16} -> Similarity: {:.4}", mode, sim);
    }

    Ok(())
}