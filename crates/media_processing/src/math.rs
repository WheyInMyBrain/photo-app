use image::DynamicImage;

#[inline]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn update_centroid(existing: &[f32], count: i32, new_emb: &[f32]) -> Vec<f32> {
    let dim = existing.len();
    let n = count as f32;
    let mut updated = vec![0.0f32; dim];

    for i in 0..dim {
        updated[i] = (existing[i] * n) + new_emb[i];
    }

    let norm = updated.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    for v in updated.iter_mut() {
        *v /= norm;
    }

    updated
}

pub fn crop_face_chip(img: &DynamicImage, x: f32, y: f32, w: f32, h: f32) -> DynamicImage {
    let (img_w, img_h) = (img.width() as f32, img.height() as f32);
    let bw = w * img_w;
    let bh = h * img_h;

    let cx = (x * img_w) + bw * 0.5;
    let cy = (y * img_h) + bh * 0.43;

    let side = (bw.max(bh) * 1.35).round();
    let px = (cx - side * 0.5).round().max(0.0) as u32;
    let py = (cy - side * 0.5).round().max(0.0) as u32;
    let pw = (side as u32).min(img.width().saturating_sub(px)).max(1);
    let ph = (side as u32).min(img.height().saturating_sub(py)).max(1);

    img.crop_imm(px, py, pw, ph)
}