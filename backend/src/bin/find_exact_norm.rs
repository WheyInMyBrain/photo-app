use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage};
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

fn resolve(rel: &str) -> PathBuf {
    if Path::new(rel).exists() { PathBuf::from(rel) } else { PathBuf::from(format!("../{}", rel)) }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn embed(session: &mut Session, img: &DynamicImage, mean: [f32; 3], std: [f32; 3], bgr: bool) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

    for (x, y, pixel) in rgb.enumerate_pixels() {
        let (r, g, b) = (pixel[0] as f32, pixel[1] as f32, pixel[2] as f32);
        let (c0, c1, c2) = if bgr { (b, g, r) } else { (r, g, b) };

        tensor[[0, 0, y as usize, x as usize]] = (c0 / 255.0 - mean[0]) / std[0];
        tensor[[0, 1, y as usize, x as usize]] = (c1 / 255.0 - mean[1]) / std[1];
        tensor[[0, 2, y as usize, x as usize]] = (c2 / 255.0 - mean[2]) / std[2];
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
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&model_path)?;

    // guy vs guy (same person)
    let guy1 = image::open(resolve("diag_output/IMG1__56__face_1.jpg"))?;
    let guy2 = image::open(resolve("diag_output/IMG2__58__face_2.jpg"))?;
    // girl (different person)
    let girl = image::open(resolve("diag_output/IMG1__56__face_2.jpg"))?;

    let configs: &[(&str, [f32; 3], [f32; 3], bool)] = &[
        // (Name, Mean, Std, is_bgr)
        ("TorchVision (RGB)", [0.485, 0.456, 0.406], [0.229, 0.224, 0.225], false),
        ("TorchVision (BGR)", [0.406, 0.456, 0.485], [0.225, 0.224, 0.229], true),
        ("InsightFace 0.5/0.5 (RGB)", [0.5, 0.5, 0.5], [0.5, 0.5, 0.5], false),
        ("InsightFace 0.5/0.5 (BGR)", [0.5, 0.5, 0.5], [0.5, 0.5, 0.5], true),
        ("Centering Only (RGB)", [0.5, 0.5, 0.5], [1.0, 1.0, 1.0], false),
    ];

    println!("{:<28} | {:<16} | {:<16} | {:<10}", "Config", "Same (Guy/Guy)", "Diff (Guy/Girl)", "Delta (Separation)");
    println!("{:-<80}", "");

    for &(name, mean, std, is_bgr) in configs {
        let e_g1 = embed(&mut session, &guy1, mean, std, is_bgr);
        let e_g2 = embed(&mut session, &guy2, mean, std, is_bgr);
        let e_girl = embed(&mut session, &girl, mean, std, is_bgr);

        let same_sim = cosine(&e_g1, &e_g2);
        let diff_sim = cosine(&e_g1, &e_girl);
        let delta = same_sim - diff_sim;

        println!("{:<28} | {:<16.4} | {:<16.4} | {:<10.4}", name, same_sim, diff_sim, delta);
    }

    Ok(())
}