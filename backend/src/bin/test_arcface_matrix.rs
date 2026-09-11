use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage, Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut};
use imageproc::rect::Rect;
use ndarray::{Array4, Ix4};
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

fn face_crop(img: &DynamicImage, d: &Detection) -> DynamicImage {
    let bw = d.x2 - d.x1;
    let bh = d.y2 - d.y1;
    let cx = d.x1 + bw * 0.5;
    let cy = d.y1 + bh * 0.45;

    let side = (bw.max(bh) * 1.35).round();
    let x0 = (cx - side * 0.5).round().max(0.0) as u32;
    let y0 = (cy - side * 0.5).round().max(0.0) as u32;

    let w = (side as u32).min(img.width().saturating_sub(x0)).max(1);
    let h = (side as u32).min(img.height().saturating_sub(y0)).max(1);

    let cropped = img.crop_imm(x0, y0, w, h);
    cropped.resize_exact(112, 112, FilterType::Triangle)
}

fn get_emb(session: &mut Session, img: &DynamicImage, is_nhwc: bool) -> Vec<f32> {
    let resized = img.resize_exact(112, 112, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let input_tensor = if is_nhwc {
        let mut tensor = Array4::<f32>::zeros((1, 112, 112, 3));
        for (x, y, p) in rgb.enumerate_pixels() {
            tensor[[0, y as usize, x as usize, 0]] = (p[0] as f32 - 127.5) / 128.0;
            tensor[[0, y as usize, x as usize, 1]] = (p[1] as f32 - 127.5) / 128.0;
            tensor[[0, y as usize, x as usize, 2]] = (p[2] as f32 - 127.5) / 128.0;
        }
        Tensor::from_array(tensor).unwrap()
    } else {
        let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));
        for (x, y, p) in rgb.enumerate_pixels() {
            tensor[[0, 0, y as usize, x as usize]] = (p[0] as f32 - 127.5) / 128.0;
            tensor[[0, 1, y as usize, x as usize]] = (p[1] as f32 - 127.5) / 128.0;
            tensor[[0, 2, y as usize, x as usize]] = (p[2] as f32 - 127.5) / 128.0;
        }
        Tensor::from_array(tensor).unwrap()
    };

    let outputs = session.run(ort::inputs![input_tensor]).unwrap();
    let (_, slice) = outputs[0].try_extract_tensor::<f32>().unwrap();

    let mut emb = slice.to_vec();
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in emb.iter_mut() {
        *v /= norm;
    }
    emb
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn resolve(rel: &str) -> PathBuf {
    if Path::new(rel).exists() { PathBuf::from(rel) } else { PathBuf::from(format!("../{}", rel)) }
}

struct PersonFace {
    label: String,
    chip: DynamicImage,
    embedding: Vec<f32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scrfd_path = resolve("storage/models/10g_bnkps.onnx");
    let arcface_path = resolve("storage/models/arcface.onnx");

    let mut det_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&scrfd_path)?;

    let mut rec_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&arcface_path)?;

    // Inspect input dimensions to automatically handle NCHW vs NHWC
    let input_shape = format!("{:?}", rec_sess.inputs()[0].dtype());
    println!("ArcFace Model Loaded: {:?}", arcface_path);
    println!("Input Info: name='{}', type={}", rec_sess.inputs()[0].name(), input_shape);

    // Check if the 4th dimension or 2nd dimension is 3
    let is_nhwc = input_shape.contains("112, 112, 3");
    println!("Detected Tensor Layout: {}", if is_nhwc { "NHWC [1, 112, 112, 3]" } else { "NCHW [1, 3, 112, 112]" });

    let photos = [
        ("IMG1", resolve("storage/originals/public/college/sem 1/IMG-20240216-WA0056.jpg")),
        ("IMG2", resolve("storage/originals/public/college/sem 1/IMG-20240216-WA0058.jpg")),
        ("IMG3", resolve("storage/originals/public/college/sem 1/IMG-20240225-WA0005.jpg")),
        ("IMG4", resolve("storage/originals/public/college/sem 1/IMG-20240225-WA0002.jpg")),
    ];

    std::fs::create_dir_all("diag_output")?;
    let mut all_faces = Vec::new();

    for (photo_name, path) in &photos {
        if !path.exists() { continue; }
        let img = image::open(path)?;
        let detections = detect_faces(&mut det_sess, &img);
        for (i, det) in detections.iter().enumerate() {
            let chip = face_crop(&img, det);
            let emb = get_emb(&mut rec_sess, &chip, is_nhwc);
            all_faces.push(PersonFace {
                label: format!("{}_f{}", photo_name, i + 1),
                chip,
                embedding: emb,
            });
        }
    }

    println!("\n=== Pairwise Matrix with downloaded arcface.onnx ===");
    println!("Threshold: 0.35\n");

    let thresh = 0.35f32;
    let mut comparisons = Vec::new();

    for i in 0..all_faces.len() {
        for j in (i + 1)..all_faces.len() {
            let f1 = &all_faces[i];
            let f2 = &all_faces[j];
            let sim = cosine(&f1.embedding, &f2.embedding);
            let is_match = sim >= thresh;
            let match_str = if is_match { "== MATCH ==" } else { "Different" };

            println!("{:<12} vs {:<12} -> {:<6.4}  {}", f1.label, f2.label, sim, match_str);
            comparisons.push((f1.chip.clone(), f2.chip.clone(), sim, is_match));
        }
    }

    // ================== COMPOSITE VISUAL GRID ==================
    if !comparisons.is_empty() {
        let thumb_size: u32 = 180;
        let row_height: u32 = 220;
        let canvas_width: u32 = 750;
        let canvas_height: u32 = comparisons.len() as u32 * row_height;

        let mut canvas = RgbImage::from_pixel(canvas_width, canvas_height, Rgb([25, 27, 31]));

        for (idx, (chip1, chip2, sim, is_match)) in comparisons.iter().enumerate() {
            let base_y = (idx as u32 * row_height + 20) as i32;

            let t1 = chip1.resize_exact(thumb_size, thumb_size, FilterType::Triangle).to_rgb8();
            let t2 = chip2.resize_exact(thumb_size, thumb_size, FilterType::Triangle).to_rgb8();

            image::imageops::overlay(&mut canvas, &t1, 40, base_y as i64);
            image::imageops::overlay(&mut canvas, &t2, 260, base_y as i64);

            let border_color = if *is_match { Rgb([40, 210, 80]) } else { Rgb([190, 45, 45]) };

            for b in 0i32..5i32 {
                draw_hollow_rect_mut(
                    &mut canvas,
                    Rect::at(40 - b, base_y - b).of_size(thumb_size + (b as u32 * 2), thumb_size + (b as u32 * 2)),
                    border_color,
                );
                draw_hollow_rect_mut(
                    &mut canvas,
                    Rect::at(260 - b, base_y - b).of_size(thumb_size + (b as u32 * 2), thumb_size + (b as u32 * 2)),
                    border_color,
                );
            }

            let badge_color = if *is_match { Rgb([40, 210, 80]) } else { Rgb([190, 45, 45]) };
            draw_filled_rect_mut(&mut canvas, Rect::at(480, base_y + 40).of_size(230, 45), badge_color);

            draw_filled_rect_mut(&mut canvas, Rect::at(480, base_y + 105).of_size(230, 20), Rgb([50, 55, 65]));
            let bar_width = ((sim.clamp(0.0, 1.0)) * 230.0).round().max(1.0) as u32;
            draw_filled_rect_mut(&mut canvas, Rect::at(480, base_y + 105).of_size(bar_width, 20), badge_color);
        }

        let out_file = PathBuf::from("diag_output/arcface_matrix.jpg");
        canvas.save(&out_file)?;
        println!("\nSaved visual matrix to: {}", out_file.display());
    }

    Ok(())
}