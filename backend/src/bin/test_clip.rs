use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage};
use ndarray::{Array2, Array4};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use tokenizers::Tokenizer;

fn resolve(rel: &str) -> PathBuf {
    if Path::new(rel).exists() {
        PathBuf::from(rel)
    } else {
        PathBuf::from(format!("../{}", rel))
    }
}

// CLIP Image Preprocessing:
// Resize/crop to 224x224, scale to [0.0, 1.0], then normalize with ImageNet stats:
// Mean: [0.48145466, 0.4578275, 0.40821073]
// Std:  [0.26862954, 0.26130258, 0.27577711]
fn preprocess_image(img: &DynamicImage) -> Array4<f32> {
    let resized = img.resize_to_fill(224, 224, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mean = [0.48145466f32, 0.4578275f32, 0.40821073f32];
    let std = [0.26862954f32, 0.26130258f32, 0.27577711f32];

    let mut tensor = Array4::<f32>::zeros((1, 3, 224, 224));
    for (x, y, p) in rgb.enumerate_pixels() {
        for c in 0..3 {
            let val = p[c] as f32 / 255.0;
            tensor[[0, c, y as usize, x as usize]] = (val - mean[c]) / std[c];
        }
    }
    tensor
}

fn tokenize_labels(
    tokenizer: &Tokenizer,
    labels: &[&str],
) -> (Array2<i64>, Array2<i64>) {
    let n = labels.len();
    let seq_len = 77; // Standard CLIP context sequence length
    let mut input_ids = Array2::<i64>::zeros((n, seq_len));
    let mut attention_mask = Array2::<i64>::zeros((n, seq_len));

    for (i, label) in labels.iter().enumerate() {
        // Enclosing prompts in context stabilizes CLIP classification
        let prompt = format!("a photo of {}", label);
        let encoding = tokenizer.encode(prompt, true).expect("Tokenization failed");

        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();

        for (j, (&id, &m)) in ids.iter().zip(mask.iter()).enumerate().take(seq_len) {
            input_ids[[i, j]] = id as i64;
            attention_mask[[i, j]] = m as i64;
        }
    }

    (input_ids, attention_mask)
}

fn softmax(logits: &mut [f32]) {
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for v in logits.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    if sum > 0.0 {
        for v in logits.iter_mut() {
            *v /= sum;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_dir = resolve("storage/models/clip-vit");
    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    println!("Loading CLIP Model: {:?}", model_path);
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .commit_from_file(&model_path)?;

    println!("Inputs required by model:");
    for input in session.inputs() {
        println!(" - {} {:?}", input.name(), input.dtype());
    }

    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| format!("Failed to load tokenizer: {}", e))?;

    // Candidate tags
    let tags = [
        "people posing together",
        "outdoor selfie",
        "indoor room",
        "food on a table",
        "scenery and nature",
        "night time street",
        "text document receipt",
    ];

    let (input_ids, attention_mask) = tokenize_labels(&tokenizer, &tags);

    let test_photos = [
        ("IMG1", resolve("storage/originals/public/college/sem 1/IMG-20240216-WA0056.jpg")),
        ("IMG3", resolve("storage/originals/public/college/sem 1/IMG-20240225-WA0005.jpg")),
    ];

    for (name, path) in &test_photos {
        if !path.exists() {
            eprintln!("File not found: {:?}", path);
            continue;
        }

        let img = image::open(path)?;
        let pixel_values = preprocess_image(&img);

        // Convert ndarray arrays into ORT Tensors
        let t_pixel = Tensor::from_array(pixel_values)?;
        let t_input_ids = Tensor::from_array(input_ids.clone())?;
        let t_attention = Tensor::from_array(attention_mask.clone())?;

        // Run combined Vision-Text CLIP forward pass
        let outputs = session.run(ort::inputs![
            "input_ids" => t_input_ids,
            "pixel_values" => t_pixel,
            "attention_mask" => t_attention
        ])?;

        // Optimum exports produce "logits_per_image"
        let target_output_idx = outputs.iter().position(|(k, _)| k == "logits_per_image").unwrap_or(0);
        let (_, logits_slice) = outputs[target_output_idx].try_extract_tensor::<f32>()?;

        let mut scores = logits_slice.to_vec();
        softmax(&mut scores);

        println!("\n=== Predictions for {} ===", name);
        let mut ranked: Vec<(&str, f32)> = tags.iter().copied().zip(scores).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        for (tag, prob) in ranked {
            let bar_len = (prob * 30.0).round() as usize;
            let bar = "█".repeat(bar_len);
            println!("{:<25} {:>6.2}%  {}", tag, prob * 100.0, bar);
        }
    }

    Ok(())
}