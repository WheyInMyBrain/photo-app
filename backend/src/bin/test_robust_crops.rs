use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

#[derive(Clone, Debug)]
struct Detection {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    score: f32,
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = a.x2.min(b.x2);
    let y2 = a.y2.min(b.y2);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = (a.x2 - a.x1) * (a.y2 - a.y1) + (b.x2 - b.x1) * (b.y2 - b.y1) - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn nms(mut list: Vec<Detection>, thresh: f32) -> Vec<Detection> {
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let mut picked = Vec::new();
    let mut suppressed = vec![false; list.len()];
    for i in 0..list.len() {
        if suppressed[i] { continue; }
        picked.push(list[i].clone());
        for j in (i + 1)..list.len() {
            if !suppressed[j] && iou(&list[i], &list[j]) > thresh {
                suppressed[j] = true;
            }
        }
    }
    picked
}

fn detect_faces(session: &mut Session, img: &DynamicImage) -> Vec<Detection> {
    let (orig_w, orig_h) = (img.width(), img.height());
    let target = 640.0f32;
    let scale = (target / orig_w as f32).min(target / orig_h as f32);
    let nw = (orig_w as f32 * scale).round() as u32;
    let nh = (orig_h as f32 * scale).round() as u32;

    let resized = img.resize_exact(nw, nh, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let pad_x = (target - nw as f32) / 2.0;
    let pad_y = (target - nh as f32) / 2.0;

    let mut tensor = Array4::<f32>::zeros((1, 3, 640, 640));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        let tx = pad_x as usize + x as usize;
        let ty = pad_y as usize + y as usize;
        if tx < 640 && ty < 640 {
            tensor[[0, 0, ty, tx]] = (pixel[2] as f32 - 127.5) / 128.0;
            tensor[[0, 1, ty, tx]] = (pixel[1] as f32 - 127.5) / 128.0;
            tensor[[0, 2, ty, tx]] = (pixel[0] as f32 - 127.5) / 128.0;
        }
    }

    let input = Tensor::from_array(tensor).unwrap();
    let outputs = session.run(ort::inputs![input]).unwrap();

    let strides = [8, 16, 32];
    let mut raw = Vec::new();

    for (i, &stride) in strides.iter().enumerate() {
        let (_, score_slice) = outputs[i].try_extract_tensor::<f32>().unwrap();
        let (_, bbox_slice) = outputs[i + 3].try_extract_tensor::<f32>().unwrap();

        let feat_h = 640 / stride;
        let feat_w = 640 / stride;
        let mut idx = 0;

        for y in 0..feat_h {
            for x in 0..feat_w {
                for _a in 0..2 {
                    if idx >= score_slice.len() { break; }
                    let score = score_slice[idx];
                    if score >= 0.50 {
                        let ax = (x as f32 + 0.5) * stride as f32;
                        let ay = (y as f32 + 0.5) * stride as f32;
                        let bi = idx * 4;
                        let l = bbox_slice[bi] * stride as f32;
                        let t = bbox_slice[bi + 1] * stride as f32;
                        let r = bbox_slice[bi + 2] * stride as f32;
                        let b = bbox_slice[bi + 3] * stride as f32;

                        let x1 = ((ax - l - pad_x) / scale).max(0.0).min(orig_w as f32);
                        let y1 = ((ay - t - pad_y) / scale).max(0.0).min(orig_h as f32);
                        let x2 = ((ax + r - pad_x) / scale).max(0.0).min(orig_w as f32);
                        let y2 = ((ay + b - pad_y) / scale).max(0.0).min(orig_h as f32);

                        if (x2 - x1) > 20.0 && (y2 - y1) > 20.0 {
                            raw.push(Detection { x1, y1, x2, y2, score });
                        }
                    }
                    idx += 1;
                }
            }
        }
    }

    nms(raw, 0.40)
}

fn embed_raw(session: &mut Session, img: &DynamicImage) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));
    for (x, y, p) in rgb.enumerate_pixels() {
        tensor[[0, 0, y as usize, x as usize]] = (p[0] as f32 - 127.5) / 127.5;
        tensor[[0, 1, y as usize, x as usize]] = (p[1] as f32 - 127.5) / 127.5;
        tensor[[0, 2, y as usize, x as usize]] = (p[2] as f32 - 127.5) / 127.5;
    }
    let input = Tensor::from_array(tensor).unwrap();
    let outputs = session.run(ort::inputs![input]).unwrap();
    let (_, slice) = outputs[0].try_extract_tensor::<f32>().unwrap();

    let mut emb = slice.to_vec();
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in emb.iter_mut() { *v /= norm; }
    emb
}

/// Computes multi-scale + horizontal flip averaged embeddings
fn extract_robust_embedding(session: &mut Session, img: &DynamicImage, d: &Detection) -> Vec<f32> {
    let bw = d.x2 - d.x1;
    let bh = d.y2 - d.y1;
    let cx = d.x1 + bw * 0.5;
    let cy = d.y1 + bh * 0.45;

    let scales = [1.25, 1.38, 1.50];
    let mut aggregated = vec![0.0f32; 512];

    for &scale in &scales {
        let side = (bw.max(bh) * scale).round();
        let x0 = (cx - side * 0.5).round().max(0.0) as u32;
        let y0 = (cy - side * 0.5).round().max(0.0) as u32;
        let w = (side as u32).min(img.width().saturating_sub(x0)).max(1);
        let h = (side as u32).min(img.height().saturating_sub(y0)).max(1);

        let crop = img.crop_imm(x0, y0, w, h);
        let crop_flipped = crop.fliph();

        let e_orig = embed_raw(session, &crop);
        let e_flip = embed_raw(session, &crop_flipped);

        for k in 0..512 {
            aggregated[k] += e_orig[k] + e_flip[k];
        }
    }

    let norm: f32 = aggregated.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in aggregated.iter_mut() {
        *v /= norm;
    }
    aggregated
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn resolve(rel: &str) -> PathBuf {
    if Path::new(rel).exists() { PathBuf::from(rel) } else { PathBuf::from(format!("../{}", rel)) }
}

struct RobustFace {
    label: String,
    embedding: Vec<f32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scrfd_path = resolve("storage/models/10g_bnkps.onnx");
    let arcface_path = resolve("storage/models/w600k_r50.onnx");

    let mut det_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&scrfd_path)?;

    let mut rec_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&arcface_path)?;

    let photos = [
        ("IMG1", resolve("storage/originals/public/college/sem 1/IMG-20240216-WA0056.jpg")),
        ("IMG2", resolve("storage/originals/public/college/sem 1/IMG-20240216-WA0058.jpg")),
        ("IMG3", resolve("storage/originals/public/college/sem 1/IMG-20240225-WA0005.jpg")),
        ("IMG4", resolve("storage/originals/public/college/sem 1/IMG-20240225-WA0002.jpg")),
    ];

    let mut faces = Vec::new();

    for (name, path) in &photos {
        if !path.exists() { continue; }
        let img = image::open(path)?;
        let detections = detect_faces(&mut det_sess, &img);
        for (idx, det) in detections.iter().enumerate() {
            let emb = extract_robust_embedding(&mut rec_sess, &img, det);
            faces.push(RobustFace {
                label: format!("{}_f{}", name, idx + 1),
                embedding: emb,
            });
        }
    }

    println!("\n=== MULTI-SCALE + FLIP EMBEDDING COMPARISON ===");
    println!("Threshold: 0.35\n");

    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            let a = &faces[i];
            let b = &faces[j];
            let sim = cosine(&a.embedding, &b.embedding);
            let is_match = sim >= 0.35;

            println!(
                "{:<10} vs {:<10} -> {:<6.4}  [{}]",
                a.label, b.label, sim,
                if is_match { "MATCH" } else { "Different" }
            );
        }
    }

    Ok(())
}