use std::path::{Path, PathBuf};
use image::{imageops::FilterType, DynamicImage, Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut};
use imageproc::rect::Rect;
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

const ARCFACE_REF_PTS: [(f32, f32); 5] = [
    (38.2946, 51.6963), // Left eye
    (73.5318, 51.5014), // Right eye
    (56.0252, 71.7366), // Nose tip
    (41.5493, 92.3655), // Left mouth corner
    (70.7299, 92.2041), // Right mouth corner
];

#[derive(Clone, Debug)]
struct Detection {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    score: f32,
    kps: [(f32, f32); 5],
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
        let has_kps = (i + 6) < outputs.len();
        let kps_slice = if has_kps {
            outputs[i + 6].try_extract_tensor::<f32>().ok().map(|(_, s)| s)
        } else {
            None
        };

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

                        let mut kps = [(0.0f32, 0.0f32); 5];
                        if let Some(ref ks) = kps_slice {
                            let ki = idx * 10;
                            for k in 0..5 {
                                let kx = ax + ks[ki + k * 2] * stride as f32;
                                let ky = ay + ks[ki + k * 2 + 1] * stride as f32;
                                kps[k] = (
                                    ((kx - pad_x) / scale).max(0.0).min(orig_w as f32),
                                    ((ky - pad_y) / scale).max(0.0).min(orig_h as f32),
                                );
                            }
                        }

                        if (x2 - x1) > 20.0 && (y2 - y1) > 20.0 {
                            raw.push(Detection { x1, y1, x2, y2, score, kps });
                        }
                    }
                    idx += 1;
                }
            }
        }
    }

    nms(raw, 0.40)
}

fn crop_face_tight(img: &DynamicImage, d: &Detection) -> DynamicImage {
    let w = d.x2 - d.x1;
    let h = d.y2 - d.y1;
    let cx = d.x1 + w / 2.0;
    let cy = d.y1 + h / 2.0;

    let size = (w.max(h) * 1.15).round();
    let px = (cx - size / 2.0).max(0.0) as u32;
    let py = (cy - size / 2.0).max(0.0) as u32;
    let pw = (size as u32).min(img.width().saturating_sub(px)).max(1);
    let ph = (size as u32).min(img.height().saturating_sub(py)).max(1);

    img.crop_imm(px, py, pw, ph)
}

fn align_face_chip(img: &DynamicImage, d: &Detection) -> DynamicImage {
    let src_pts = &d.kps;

    if src_pts[0].0 == 0.0 && src_pts[1].0 == 0.0 {
        return crop_face_tight(img, d).resize_exact(112, 112, FilterType::Triangle);
    }

    let mut src_mean = (0.0f32, 0.0f32);
    let mut dst_mean = (0.0f32, 0.0f32);
    for i in 0..5 {
        src_mean.0 += src_pts[i].0;
        src_mean.1 += src_pts[i].1;
        dst_mean.0 += ARCFACE_REF_PTS[i].0;
        dst_mean.1 += ARCFACE_REF_PTS[i].1;
    }
    src_mean.0 /= 5.0; src_mean.1 /= 5.0;
    dst_mean.0 /= 5.0; dst_mean.1 /= 5.0;

    let mut sum_dot = 0.0f32;
    let mut sum_cross = 0.0f32;
    let mut den = 0.0f32;

    for i in 0..5 {
        let sx = src_pts[i].0 - src_mean.0;
        let sy = src_pts[i].1 - src_mean.1;
        let dx = ARCFACE_REF_PTS[i].0 - dst_mean.0;
        let dy = ARCFACE_REF_PTS[i].1 - dst_mean.1;

        sum_dot += sx * dx + sy * dy;
        sum_cross += sx * dy - sy * dx;
        den += sx * sx + sy * sy;
    }

    let a = sum_dot / den.max(1e-6);
    let b = sum_cross / den.max(1e-6);

    let tx = dst_mean.0 - (a * src_mean.0 - b * src_mean.1);
    let ty = dst_mean.1 - (b * src_mean.0 + a * src_mean.1);

    let det = (a * a + b * b).max(1e-6);
    let inv_a = a / det;
    let inv_b = b / det;

    let (w, h) = (img.width(), img.height());
    let rgb = img.to_rgb8();
    let mut out_chip = RgbImage::new(112, 112);

    for y in 0..112 {
        for x in 0..112 {
            let x_shift = x as f32 - tx;
            let y_shift = y as f32 - ty;

            let sx = inv_a * x_shift + inv_b * y_shift;
            let sy = -inv_b * x_shift + inv_a * y_shift;

            // Clamp coordinates to boundary pixels to avoid black gaps
            let sx_clamped = sx.clamp(0.0, (w - 2) as f32);
            let sy_clamped = sy.clamp(0.0, (h - 2) as f32);

            let x0 = sx_clamped.floor() as u32;
            let y0 = sy_clamped.floor() as u32;
            let x1 = x0 + 1;
            let y1 = y0 + 1;

            let fx = sx_clamped - x0 as f32;
            let fy = sy_clamped - y0 as f32;

            let p00 = rgb.get_pixel(x0, y0);
            let p10 = rgb.get_pixel(x1, y0);
            let p01 = rgb.get_pixel(x0, y1);
            let p11 = rgb.get_pixel(x1, y1);

            let mut pixel = [0u8; 3];
            for c in 0..3 {
                let top = (1.0 - fx) * (p00[c] as f32) + fx * (p10[c] as f32);
                let bot = (1.0 - fx) * (p01[c] as f32) + fx * (p11[c] as f32);
                pixel[c] = ((1.0 - fy) * top + fy * bot).clamp(0.0, 255.0) as u8;
            }
            out_chip.put_pixel(x, y, Rgb(pixel));
        }
    }

    DynamicImage::ImageRgb8(out_chip)
}

/// w600k_r50 expects BGR format normalized with (x - 127.5) / 127.5 -> [-1.0, 1.0]
fn get_embedding_w600k(session: &mut Session, chip: &DynamicImage) -> Vec<f32> {
    let rgb = chip.to_rgb8();
    let mut tensor = Array4::<f32>::zeros((1, 3, 112, 112));

    for (x, y, pixel) in rgb.enumerate_pixels() {
        // Raw RGB floats [0.0, 255.0]
        tensor[[0, 0, y as usize, x as usize]] = pixel[0] as f32;
        tensor[[0, 1, y as usize, x as usize]] = pixel[1] as f32;
        tensor[[0, 2, y as usize, x as usize]] = pixel[2] as f32;
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

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn resolve_path(rel: &str) -> PathBuf {
    if Path::new(rel).exists() {
        PathBuf::from(rel)
    } else {
        PathBuf::from(format!("../{}", rel))
    }
}

struct ImageItem {
    label: String,
    chips: Vec<DynamicImage>,
    embeddings: Vec<Vec<f32>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scrfd_path = resolve_path("storage/models/10g_bnkps.onnx");
    let arcface_path = resolve_path("storage/models/w600k_r50.onnx");

    println!("Loading SCRFD: {:?}", scrfd_path);
    let mut det_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&scrfd_path)?;

    println!("Loading InsightFace w600k_r50: {:?}", arcface_path);
    let mut rec_sess = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .commit_from_file(&arcface_path)?;

    let paths = [
        ("IMG1 (56)", resolve_path("storage/originals/public/college/sem 1/IMG-20240216-WA0056.jpg")),
        ("IMG2 (58)", resolve_path("storage/originals/public/college/sem 1/IMG-20240216-WA0058.jpg")),
        ("IMG3 (05)", resolve_path("storage/originals/public/college/sem 1/IMG-20240225-WA0005.jpg")),
    ];

    std::fs::create_dir_all("diag_output")?;

    let mut items = Vec::new();

    for (label, p) in &paths {
        println!("\nReading {:?}", p);
        let img = image::open(p)?;
        let detections = detect_faces(&mut det_sess, &img);
        println!("  -> Found {} face(s)", detections.len());

        let mut chips = Vec::new();
        let mut embs = Vec::new();

        for (i, d) in detections.iter().enumerate() {
            let chip = align_face_chip(&img, d);
            let filename = format!("diag_output/{}_face_{}.jpg", label.replace([' ', '(', ')'], "_"), i + 1);
            chip.save(&filename)?;
            
            let emb = get_embedding_w600k(&mut rec_sess, &chip);
            chips.push(chip);
            embs.push(emb);
        }

        items.push(ImageItem {
            label: label.to_string(),
            chips,
            embeddings: embs,
        });
    }

    println!("\n================== PAIRWISE SIMILARITY MATRIX (w600k_r50) ==================");
    println!("Threshold: 0.55 (Expected: Same person >= 0.60, Different <= 0.30)\n");

    let mut comparisons = Vec::new();

    for a_idx in 0..items.len() {
        for b_idx in (a_idx + 1)..items.len() {
            let item_a = &items[a_idx];
            let item_b = &items[b_idx];

            println!("--- {} vs {} ---", item_a.label, item_b.label);

            for (i, ea) in item_a.embeddings.iter().enumerate() {
                for (j, eb) in item_b.embeddings.iter().enumerate() {
                    let sim = cosine(ea, eb);
                    let is_match = sim >= 0.55;

                    println!(
                        "{} Face #{} vs {} Face #{} -> Similarity: {:.4}  [{}]",
                        item_a.label, i + 1, item_b.label, j + 1, sim,
                        if is_match { "MATCH (SAME)" } else { "DIFFERENT" }
                    );

                    comparisons.push((
                        item_a.chips[i].clone(),
                        item_b.chips[j].clone(),
                        sim,
                        is_match,
                    ));
                }
            }
            println!();
        }
    }

    // ================== CREATE COMPOSITE VISUAL GRID ==================
    println!("Generating visual comparison matrix...");

    let thumb_size: u32 = 180;
    let row_height: u32 = 220;
    let canvas_width: u32 = 750;
    let canvas_height: u32 = (comparisons.len() as u32 * row_height).max(220);

    let mut canvas = RgbImage::from_pixel(canvas_width, canvas_height, Rgb([25, 27, 31]));

    for (idx, (chip_a, chip_b, sim, is_match)) in comparisons.iter().enumerate() {
        let base_y = (idx as u32 * row_height + 20) as i32;

        let t1 = chip_a.resize_exact(thumb_size, thumb_size, FilterType::Triangle).to_rgb8();
        let t2 = chip_b.resize_exact(thumb_size, thumb_size, FilterType::Triangle).to_rgb8();

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
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(480, base_y + 40).of_size(230, 45),
            badge_color,
        );

        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(480, base_y + 105).of_size(230, 20),
            Rgb([50, 55, 65]),
        );
        let bar_width = ((sim.clamp(0.0, 1.0)) * 230.0).round().max(1.0) as u32;
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(480, base_y + 105).of_size(bar_width, 20),
            badge_color,
        );
    }

    let out_file = PathBuf::from("diag_output/comparison_matrix.jpg");
    canvas.save(&out_file)?;
    println!("Saved composite comparison grid to: {}", out_file.display());

    Ok(())
}