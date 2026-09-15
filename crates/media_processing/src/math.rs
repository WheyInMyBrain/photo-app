use image::{DynamicImage, GenericImageView, SubImage};

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

pub struct FaceChipBounds {
    pub px: u32,
    pub py: u32,
    pub pw: u32,
    pub ph: u32,
}

#[inline]
pub fn compute_face_chip_bounds(
    img_w: u32,
    img_h: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> FaceChipBounds {
    let (fw, fh) = (img_w as f32, img_h as f32);
    let bw = w * fw;
    let bh = h * fh;

    let cx = (x * fw) + bw * 0.5;
    let cy = (y * fh) + bh * 0.43;

    let side = (bw.max(bh) * 1.35).round();
    let px = (cx - side * 0.5).round().max(0.0) as u32;
    let py = (cy - side * 0.5).round().max(0.0) as u32;
    let pw = (side as u32).min(img_w.saturating_sub(px)).max(1);
    let ph = (side as u32).min(img_h.saturating_sub(py)).max(1);

    FaceChipBounds { px, py, pw, ph }
}

#[inline]
pub fn view_face_chip<'a>(
    img: &'a DynamicImage,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> SubImage<&'a DynamicImage> {
    let bounds = compute_face_chip_bounds(img.width(), img.height(), x, y, w, h);
    img.view(bounds.px, bounds.py, bounds.pw, bounds.ph)
}