#[derive(Clone, Debug)]
pub struct RawDetection {
    /// Normalized bounding box coordinates [0.0..1.0] relative to original image dimensions
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    #[allow(dead_code)]
    pub landmarks: Vec<(f32, f32)>,
}

pub struct ScrfdPostProcessor;

impl ScrfdPostProcessor {
    #[inline]
    fn iou(a: &RawDetection, b: &RawDetection) -> f32 {
        let x1 = a.x.max(b.x);
        let y1 = a.y.max(b.y);
        let x2 = (a.x + a.w).min(b.x + b.w);
        let y2 = (a.y + a.h).min(b.y + b.h);

        let inter_w = (x2 - x1).max(0.0);
        let inter_h = (y2 - y1).max(0.0);
        let inter_area = inter_w * inter_h;

        let area_a = a.w * a.h;
        let area_b = b.w * b.h;
        let union_area = area_a + area_b - inter_area;

        if union_area <= 0.0 {
            0.0
        } else {
            inter_area / union_area
        }
    }

    pub fn nms(mut detections: Vec<RawDetection>, iou_threshold: f32) -> Vec<RawDetection> {
        detections.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let mut picked = Vec::new();
        let mut suppressed = vec![false; detections.len()];

        for i in 0..detections.len() {
            if suppressed[i] {
                continue;
            }
            picked.push(detections[i].clone());

            for j in (i + 1)..detections.len() {
                if !suppressed[j] && Self::iou(&detections[i], &detections[j]) > iou_threshold {
                    suppressed[j] = true;
                }
            }
        }

        picked
    }

    /// Decodes SCRFD 10G tensors for a given stride level (8, 16, or 32).
    /// Uses anchor grid offsets to compute normalized bounding boxes.
    pub fn decode_stride(
        score_slice: &[f32],
        bbox_slice: &[f32],
        _kps_slice: Option<&[f32]>,
        stride: usize,
        score_threshold: f32,
        scale: f32,
        pad_x: f32,
        pad_y: f32,
        orig_w: u32,
        orig_h: u32,
    ) -> Vec<RawDetection> {
        let feat_h = 640 / stride;
        let feat_w = 640 / stride;
        let num_anchors = 2; // SCRFD-10G has 2 anchors per spatial location

        let mut detections = Vec::new();
        let mut idx = 0;

        for y in 0..feat_h {
            for x in 0..feat_w {
                for _a in 0..num_anchors {
                    if idx >= score_slice.len() {
                        break;
                    }

                    let score = score_slice[idx];

                    if score >= score_threshold {
                        let anchor_x = (x as f32 + 0.5) * stride as f32;
                        let anchor_y = (y as f32 + 0.5) * stride as f32;

                        let bbox_idx = idx * 4;
                        if bbox_idx + 3 < bbox_slice.len() {
                            let l = bbox_slice[bbox_idx + 0] * stride as f32;
                            let t = bbox_slice[bbox_idx + 1] * stride as f32;
                            let r = bbox_slice[bbox_idx + 2] * stride as f32;
                            let b = bbox_slice[bbox_idx + 3] * stride as f32;

                            let x1 = ((anchor_x - l - pad_x) / scale).max(0.0).min(orig_w as f32);
                            let y1 = ((anchor_y - t - pad_y) / scale).max(0.0).min(orig_h as f32);
                            let x2 = ((anchor_x + r - pad_x) / scale).max(0.0).min(orig_w as f32);
                            let y2 = ((anchor_y + b - pad_y) / scale).max(0.0).min(orig_h as f32);

                            let box_w = x2 - x1;
                            let box_h = y2 - y1;

                            // Filter out degenerate micro-boxes (< 12px)
                            if box_w >= 12.0 && box_h >= 12.0 {
                                detections.push(RawDetection {
                                    x: x1 / orig_w as f32,
                                    y: y1 / orig_h as f32,
                                    w: box_w / orig_w as f32,
                                    h: box_h / orig_h as f32,
                                    score,
                                    landmarks: Vec::new(),
                                });
                            }
                        }
                    }

                    idx += 1;
                }
            }
        }

        detections
    }
}