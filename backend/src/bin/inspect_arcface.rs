use std::path::PathBuf;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use image::{imageops::FilterType, DynamicImage};
use ndarray::Array4;

#[inline]
fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn extract_emb(session: &mut Session, img: &DynamicImage) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        let r = pixel[0] as f32;
        let g = pixel[1] as f32;
        let b = pixel[2] as f32;

        // Test BGR normalization: (x - 127.5) / 128.0
        tensor[[0, 0, y as usize, x as usize]] = (b - 127.5) / 128.0;
        tensor[[0, 1, y as usize, x as usize]] = (g - 127.5) / 128.0;
        tensor[[0, 2, y as usize, x as usize]] = (r - 127.5) / 128.0;
    }

    let input_tensor = Tensor::from_array(tensor).unwrap();
    let outputs = session.run(ort::inputs![input_tensor]).unwrap();
    let (_, slice) = outputs[0].try_extract_tensor::<f32>().unwrap();

    let mut emb = slice.to_vec();
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in emb.iter_mut() {
        *v /= norm;
    }
    emb
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_path = if PathBuf::from("storage/models/arcfaceresnet100-8.onnx").exists() {
        PathBuf::from("storage/models/arcfaceresnet100-8.onnx")
    } else {
        PathBuf::from("../storage/models/arcfaceresnet100-8.onnx")
    };

    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&model_path)?;

    println!("=== ArcFace Model Architecture ===");
    for (i, input) in session.inputs().iter().enumerate() {
        println!("Input #{}: name='{}'", i, input.name());
    }
    for (i, output) in session.outputs().iter().enumerate() {
        println!("Output #{}: name='{}'", i, output.name());
    }

    // Load two different face thumbnails from storage/thumbs/faces
    let thumbs_dir = if PathBuf::from("storage/thumbs/faces").exists() {
        PathBuf::from("storage/thumbs/faces")
    } else {
        PathBuf::from("../storage/thumbs/faces")
    };

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&thumbs_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |ext| ext == "webp"))
        .collect();

    entries.sort();

    if entries.len() < 2 {
        eprintln!("Need at least 2 faces in {:?} to test.", thumbs_dir);
        return Ok(());
    }

    println!("\nTesting pairwise embeddings from {} cropped faces:", entries.len());
    let mut embs = Vec::new();
    for p in &entries {
        let img = image::open(p)?;
        let emb = extract_emb(&mut session, &img);
        println!("File: {:?}, first 5 floats: {:?}", p.file_name().unwrap(), &emb[0..5]);
        embs.push((p.clone(), emb));
    }

    println!("\n=== Pairwise Similarities ===");
    for i in 0..embs.len() {
        for j in (i + 1)..embs.len() {
            let sim = cosine_sim(&embs[i].1, &embs[j].1);
            println!(
                "{} vs {} -> Similarity: {:.4}",
                embs[i].0.file_name().unwrap().to_str().unwrap(),
                embs[j].0.file_name().unwrap().to_str().unwrap(),
                sim
            );
        }
    }

    Ok(())
}