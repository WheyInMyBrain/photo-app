use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage};
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

fn resolve(rel: &str) -> PathBuf {
    if Path::new(rel).exists() { PathBuf::from(rel) } else { PathBuf::from(format!("../{}", rel)) }
}

#[inline]
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn embed(session: &mut Session, img: &DynamicImage, mode: &str) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

    for (x, y, pixel) in rgb.enumerate_pixels() {
        let (r, g, b) = (pixel[0] as f32, pixel[1] as f32, pixel[2] as f32);
        let py = y as usize;
        let px = x as usize;

        match mode {
            "raw_255_rgb" => {
                tensor[[0, 0, py, px]] = r;
                tensor[[0, 1, py, px]] = g;
                tensor[[0, 2, py, px]] = b;
            }
            "norm_sym_rgb" => {
                tensor[[0, 0, py, px]] = (r - 127.5) / 127.5;
                tensor[[0, 1, py, px]] = (g - 127.5) / 127.5;
                tensor[[0, 2, py, px]] = (b - 127.5) / 127.5;
            }
            "norm_sym_bgr" => {
                tensor[[0, 0, py, px]] = (b - 127.5) / 127.5;
                tensor[[0, 1, py, px]] = (g - 127.5) / 127.5;
                tensor[[0, 2, py, px]] = (r - 127.5) / 127.5;
            }
            "unit_rgb" => {
                tensor[[0, 0, py, px]] = r / 255.0;
                tensor[[0, 1, py, px]] = g / 255.0;
                tensor[[0, 2, py, px]] = b / 255.0;
            }
            "imagenet_rgb" => {
                tensor[[0, 0, py, px]] = (r / 255.0 - 0.485) / 0.229;
                tensor[[0, 1, py, px]] = (g / 255.0 - 0.456) / 0.224;
                tensor[[0, 2, py, px]] = (b / 255.0 - 0.406) / 0.225;
            }
            _ => unreachable!(),
        }
    }

    let input = Tensor::from_array(tensor).unwrap();
    let outputs = session.run(ort::inputs![input]).unwrap();
    let (_, slice) = outputs[0].try_extract_tensor::<f32>().unwrap();

    let mut emb = slice.to_vec();
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in emb.iter_mut() {
        *v /= norm;
    }
    emb
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_path = resolve("storage/models/w600k_r50.onnx");
    let f1_path = resolve("diag_output/IMG1__56__face_1.jpg");
    let f2_path = resolve("diag_output/IMG2__58__face_2.jpg");

    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&model_path)?;

    println!("Model: {:?}", model_path);
    for (i, input) in session.inputs().iter().enumerate() {
        println!("Input #{}: name='{}', type={:?}", i, input.name(), input.dtype());
    }
    for (i, output) in session.outputs().iter().enumerate() {
        println!("Output #{}: name='{}', type={:?}", i, output.name(), output.dtype());
    }

    let img1 = image::open(&f1_path)?;
    let img2 = image::open(&f2_path)?;

    println!("\n=== Testing Identical Person Across All Preprocessing Modes ===");
    let modes = ["raw_255_rgb", "norm_sym_rgb", "norm_sym_bgr", "unit_rgb", "imagenet_rgb"];

    for mode in modes {
        let e1 = embed(&mut session, &img1, mode);
        let e2 = embed(&mut session, &img2, mode);
        let sim = cosine(&e1, &e2);
        println!("Mode: {:<16} -> Similarity: {:.4}", mode, sim);
    }

    Ok(())
}