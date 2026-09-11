use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage, Rgb};
use imageproc::geometric_transformations::{rotate_about_center, Border, Interpolation};
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

fn embed_insightface(session: &mut Session, img: &DynamicImage) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        // InsightFace canonical RGB: [-1.0, 1.0]
        tensor[[0, 0, y as usize, x as usize]] = (pixel[0] as f32 - 127.5) / 127.5;
        tensor[[0, 1, y as usize, x as usize]] = (pixel[1] as f32 - 127.5) / 127.5;
        tensor[[0, 2, y as usize, x as usize]] = (pixel[2] as f32 - 127.5) / 127.5;
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

    let img1 = image::open(resolve("diag_output/IMG1__56__face_1.jpg"))?;
    let img2 = image::open(resolve("diag_output/IMG2__58__face_2.jpg"))?;
    let girl = image::open(resolve("diag_output/IMG1__56__face_2.jpg"))?;

    let e1 = embed_insightface(&mut session, &img1);
    let e_girl = embed_insightface(&mut session, &girl);

    println!("Baseline (Unaligned / Tilted):");
    let e2_base = embed_insightface(&mut session, &img2);
    println!("  Guy vs Guy (Tilted 0°) : {:.4}", cosine(&e1, &e2_base));
    println!("  Guy vs Girl (Baseline)  : {:.4}", cosine(&e1, &e_girl));

    println!("\nRotating Guy #2 to level his eyes with Guy #1:");
    // Test rotating in 5-degree increments to find the true upright orientation
    for angle in [-25.0, -20.0, -15.0, -10.0, 10.0, 15.0, 20.0, 25.0] {
        let rad = angle * std::f32::consts::PI / 180.0;
        let rotated = rotate_about_center(
            &img2.to_rgb8(),
            rad,
            Interpolation::Bilinear,
            Border::Constant(Rgb([0, 0, 0])),
        );
        let e2_rot = embed_insightface(&mut session, &DynamicImage::ImageRgb8(rotated));
        let sim = cosine(&e1, &e2_rot);
        println!("  Guy vs Guy (Rotated {:>4}°): {:.4}", angle, sim);
    }

    Ok(())
}