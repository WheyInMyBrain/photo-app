use std::path::PathBuf;
use image::{imageops::FilterType, GenericImageView, Rgb, RgbImage};
use imageproc::drawing::{draw_hollow_rect_mut, draw_text_mut};
use imageproc::rect::Rect;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

#[derive(Clone, Debug)]
struct BoxResult {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    score: f32,
}

fn iou(a: &BoxResult, b: &BoxResult) -> f32 {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = a.x2.min(b.x2);
    let y2 = a.y2.min(b.y2);

    let w = (x2 - x1).max(0.0);
    let h = (y2 - y1).max(0.0);
    let inter = w * h;
    let union = (a.x2 - a.x1) * (a.y2 - a.y1) + (b.x2 - b.x1) * (b.y2 - b.y1) - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn nms(mut boxes: Vec<BoxResult>, threshold: f32) -> Vec<BoxResult> {
    boxes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let mut picked = Vec::new();
    let mut suppressed = vec![false; boxes.len()];

    for i in 0..boxes.len() {
        if suppressed[i] { continue; }
        picked.push(boxes[i].clone());
        for j in (i + 1)..boxes.len() {
            if !suppressed[j] && iou(&boxes[i], &boxes[j]) > threshold {
                suppressed[j] = true;
            }
        }
    }
    picked
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --bin inspect_model <path-to-test-image>");
        std::process::exit(1);
    }
    let image_path = PathBuf::from(&args[1]);

    let model_path = if PathBuf::from("storage/models/10g_bnkps.onnx").exists() {
        PathBuf::from("storage/models/10g_bnkps.onnx")
    } else {
        PathBuf::from("../storage/models/10g_bnkps.onnx")
    };

    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&model_path)?;

    let orig_img = image::open(&image_path)?;
    let (orig_w, orig_h) = (orig_img.width(), orig_img.height());
    println!("Image dimensions: {}x{}", orig_w, orig_h);

    // Letterbox to 640x640
    let target_size = 640.0f32;
    let scale = (target_size / orig_w as f32).min(target_size / orig_h as f32);
    let new_w = (orig_w as f32 * scale).round() as u32;
    let new_h = (orig_h as f32 * scale).round() as u32;

    let resized = orig_img.resize_exact(new_w, new_h, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let pad_x = (target_size - new_w as f32) / 2.0;
    let pad_y = (target_size - new_h as f32) / 2.0;

    let mut tensor = ndarray::Array4::<f32>::zeros((1, 3, 640, 640));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        let tx = pad_x as usize + x as usize;
        let ty = pad_y as usize + y as usize;
        if tx < 640 && ty < 640 {
            // Standard InsightFace BGR normalization
            tensor[[0, 0, ty, tx]] = (pixel[2] as f32 - 127.5) / 128.0;
            tensor[[0, 1, ty, tx]] = (pixel[1] as f32 - 127.5) / 128.0;
            tensor[[0, 2, ty, tx]] = (pixel[0] as f32 - 127.5) / 128.0;
        }
    }

    let ort_tensor = Tensor::from_array(tensor)?;
    let outputs = session.run(ort::inputs![ort_tensor])?;

    let strides = [8, 16, 32];
    let num_anchors = 2;
    let score_threshold = 0.50; // Use clean probability cutoff
    let mut detections = Vec::new();

    for (i, &stride) in strides.iter().enumerate() {
        let (_, score_slice) = outputs[i].try_extract_tensor::<f32>()?;
        let (_, bbox_slice) = outputs[i + 3].try_extract_tensor::<f32>()?;

        let feat_h = 640 / stride;
        let feat_w = 640 / stride;

        let mut idx = 0;
        for y in 0..feat_h {
            for x in 0..feat_w {
                for _a in 0..num_anchors {
                    let score = score_slice[idx];
                    if score >= score_threshold {
                        let anchor_x = (x as f32 + 0.5) * stride as f32;
                        let anchor_y = (y as f32 + 0.5) * stride as f32;

                        let bbox_idx = idx * 4;
                        let l = bbox_slice[bbox_idx + 0] * stride as f32;
                        let t = bbox_slice[bbox_idx + 1] * stride as f32;
                        let r = bbox_slice[bbox_idx + 2] * stride as f32;
                        let b = bbox_slice[bbox_idx + 3] * stride as f32;

                        let x1 = ((anchor_x - l - pad_x) / scale).max(0.0).min(orig_w as f32);
                        let y1 = ((anchor_y - t - pad_y) / scale).max(0.0).min(orig_h as f32);
                        let x2 = ((anchor_x + r - pad_x) / scale).max(0.0).min(orig_w as f32);
                        let y2 = ((anchor_y + b - pad_y) / scale).max(0.0).min(orig_h as f32);

                        if (x2 - x1) > 10.0 && (y2 - y1) > 10.0 {
                            detections.push(BoxResult { x1, y1, x2, y2, score });
                        }
                    }
                    idx += 1;
                }
            }
        }
    }

    println!("Raw detections above threshold: {}", detections.len());
    let filtered = nms(detections, 0.4);
    println!("Detections after NMS: {}", filtered.len());

    // Draw bounding boxes on the original image
    let mut debug_image: RgbImage = orig_img.to_rgb8();
    let green = Rgb([0u8, 255u8, 0u8]);

    for (k, b) in filtered.iter().enumerate() {
        println!("Face #{}: score={:.3}, bbox=[{:.1}, {:.1}, {:.1}, {:.1}]", k + 1, b.score, b.x1, b.y1, b.x2, b.y2);

        let rx = b.x1 as i32;
        let ry = b.y1 as i32;
        let rw = (b.x2 - b.x1) as u32;
        let rh = (b.y2 - b.y1) as u32;

        // Draw multiple outline rings for thickness
        for offset in 0..6 {
            if rx >= offset && ry >= offset {
                draw_hollow_rect_mut(
                    &mut debug_image,
                    Rect::at(rx - offset, ry - offset).of_size(rw + (offset as u32 * 2), rh + (offset as u32 * 2)),
                    green,
                );
            }
        }
    }

    let out_path = PathBuf::from("debug_detection.jpg");
    debug_image.save(&out_path)?;
    println!("\nSaved visual confirmation image to: {}", out_path.display());

    Ok(())
}