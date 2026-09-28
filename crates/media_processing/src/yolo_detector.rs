// photo-app/crates/media_processing/src/yolo_detector.rs
use crate::models::{DetectedPose, Keypoint};

pub struct YoloPostProcessor;

impl YoloPostProcessor {
    #[inline]
    pub fn iou_box(ax: f32, ay: f32, aw: f32, ah: f32, bx: f32, by: f32, bw: f32, bh: f32) -> f32 {
        let x1 = ax.max(bx);
        let y1 = ay.max(by);
        let x2 = (ax + aw).min(bx + bw);
        let y2 = (ay + ah).min(by + bh);

        let inter_w = (x2 - x1).max(0.0);
        let inter_h = (y2 - y1).max(0.0);
        let inter_area = inter_w * inter_h;

        let area_a = (aw * ah).max(0.0);
        let area_b = (bw * bh).max(0.0);
        let union_area = area_a + area_b - inter_area;

        if union_area <= 1e-6 {
            0.0
        } else {
            (inter_area / union_area).clamp(0.0, 1.0)
        }
    }

    pub fn nms_poses(mut poses: Vec<DetectedPose>, iou_threshold: f32) -> Vec<DetectedPose> {
        poses.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let mut picked = Vec::new();
        let mut suppressed = vec![false; poses.len()];

        for i in 0..poses.len() {
            if suppressed[i] {
                continue;
            }
            picked.push(poses[i].clone());

            for j in (i + 1)..poses.len() {
                if !suppressed[j] {
                    let iou = Self::iou_box(
                        poses[i].x, poses[i].y, poses[i].w, poses[i].h,
                        poses[j].x, poses[j].y, poses[j].w, poses[j].h,
                    );
                    if iou > iou_threshold {
                        suppressed[j] = true;
                    }
                }
            }
        }
        picked
    }

    /// Decodes YOLO Pose estimation from:
    /// - End-to-End: `[1, 300, 57]` -> `[x1, y1, x2, y2, score, class_id, 17*(x,y,conf)]`
    /// - End-to-End (legacy): `[1, 300, 56]` -> `[x1, y1, x2, y2, score, 17*(x,y,conf)]`
    /// - Standard Channel-First: `[1, 56, 8400]` -> `[cx, cy, w, h, score, 17*(x,y,conf)]`
    /// - Standard Anchor-First: `[1, 8400, 56]` -> Transposed
    pub fn decode_pose_output(
        raw_slice: &[f32],
        shape: &[i64],
        conf_threshold: f32,
        scale: f32,
        pad_x: f32,
        pad_y: f32,
        orig_w: u32,
        orig_h: u32,
    ) -> Vec<DetectedPose> {
        let mut poses = Vec::new();
        let orig_wf = orig_w as f32;
        let orig_hf = orig_h as f32;

        if orig_wf <= 0.0 || orig_hf <= 0.0 || scale <= 0.0 || shape.len() < 3 {
            return poses;
        }

        // Layout A: End-to-End Flat Detections [1, num_boxes (<=300), step (56 or 57)]
        if shape[1] <= 300 && (shape[2] == 56 || shape[2] == 57) {
            let num_boxes = shape[1] as usize;
            let step = shape[2] as usize;
            // In [1, 300, 57]: index 4 is score, index 5 is class_id (0.0). Keypoints start at index 6.
            // In [1, 300, 56]: index 4 is score. Keypoints start at index 5.
            let kpt_start = if step == 57 { 6 } else { 5 };

            for i in 0..num_boxes {
                let offset = i * step;
                if offset + 4 >= raw_slice.len() {
                    break;
                }

                let conf = raw_slice[offset + 4];

                if conf >= conf_threshold {
                    let x1 = ((raw_slice[offset + 0] - pad_x) / scale).clamp(0.0, orig_wf);
                    let y1 = ((raw_slice[offset + 1] - pad_y) / scale).clamp(0.0, orig_hf);
                    let x2 = ((raw_slice[offset + 2] - pad_x) / scale).clamp(0.0, orig_wf);
                    let y2 = ((raw_slice[offset + 3] - pad_y) / scale).clamp(0.0, orig_hf);

                    let box_w = (x2 - x1).max(0.0);
                    let box_h = (y2 - y1).max(0.0);

                    let norm_x = (x1 / orig_wf).clamp(0.0, 1.0);
                    let norm_y = (y1 / orig_hf).clamp(0.0, 1.0);
                    let norm_w = (box_w / orig_wf).clamp(0.0, 1.0 - norm_x);
                    let norm_h = (box_h / orig_hf).clamp(0.0, 1.0 - norm_y);

                    let mut keypoints = Vec::with_capacity(17);
                    for k in 0..17 {
                        let kp_offset = offset + kpt_start + k * 3;
                        if kp_offset + 2 < raw_slice.len() {
                            let kx_raw = raw_slice[kp_offset + 0];
                            let ky_raw = raw_slice[kp_offset + 1];
                            let k_score = raw_slice[kp_offset + 2];

                            let kx = (((kx_raw - pad_x) / scale) / orig_wf).clamp(0.0, 1.0);
                            let ky = (((ky_raw - pad_y) / scale) / orig_hf).clamp(0.0, 1.0);

                            keypoints.push(Keypoint {
                                x: kx,
                                y: ky,
                                score: k_score,
                            });
                        }
                    }

                    poses.push(DetectedPose {
                        score: conf,
                        x: norm_x,
                        y: norm_y,
                        w: norm_w,
                        h: norm_h,
                        keypoints,
                    });
                }
            }
            return Self::nms_poses(poses, 0.45);
        }

        // Layout B: Standard Ultralytics Pose (Channel-First [1, 56, 8400] vs Anchor-First [1, 8400, 56])
        let (num_channels, num_anchors, is_channel_first) = if shape[1] < shape[2] {
            (shape[1] as usize, shape[2] as usize, true)
        } else {
            (shape[2] as usize, shape[1] as usize, false)
        };

        let get_val = |c: usize, a: usize| -> f32 {
            let idx = if is_channel_first {
                c * num_anchors + a
            } else {
                a * num_channels + c
            };
            raw_slice.get(idx).copied().unwrap_or(0.0)
        };

        for a in 0..num_anchors {
            let score = get_val(4, a);

            if score >= conf_threshold {
                let cx = get_val(0, a);
                let cy = get_val(1, a);
                let bw = get_val(2, a);
                let bh = get_val(3, a);

                let x1 = ((cx - bw * 0.5 - pad_x) / scale).clamp(0.0, orig_wf);
                let y1 = ((cy - bh * 0.5 - pad_y) / scale).clamp(0.0, orig_hf);
                let x2 = ((cx + bw * 0.5 - pad_x) / scale).clamp(0.0, orig_wf);
                let y2 = ((cy + bh * 0.5 - pad_y) / scale).clamp(0.0, orig_hf);

                let box_w = (x2 - x1).max(0.0);
                let box_h = (y2 - y1).max(0.0);

                if box_w >= 8.0 && box_h >= 8.0 {
                    let norm_x = (x1 / orig_wf).clamp(0.0, 1.0);
                    let norm_y = (y1 / orig_hf).clamp(0.0, 1.0);
                    let norm_w = (box_w / orig_wf).clamp(0.0, 1.0 - norm_x);
                    let norm_h = (box_h / orig_hf).clamp(0.0, 1.0 - norm_y);

                    let mut keypoints = Vec::with_capacity(17);
                    for k in 0..17 {
                        let kx_raw = get_val(5 + k * 3 + 0, a);
                        let ky_raw = get_val(5 + k * 3 + 1, a);
                        let k_score = get_val(5 + k * 3 + 2, a);

                        let kx = (((kx_raw - pad_x) / scale) / orig_wf).clamp(0.0, 1.0);
                        let ky = (((ky_raw - pad_y) / scale) / orig_hf).clamp(0.0, 1.0);

                        keypoints.push(Keypoint {
                            x: kx,
                            y: ky,
                            score: k_score,
                        });
                    }

                    poses.push(DetectedPose {
                        score,
                        x: norm_x,
                        y: norm_y,
                        w: norm_w,
                        h: norm_h,
                        keypoints,
                    });
                }
            }
        }

        Self::nms_poses(poses, 0.45)
    }
}