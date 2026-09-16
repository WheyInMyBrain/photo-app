use image::DynamicImage;
use super::tag_engine::TagEngine;

#[derive(Debug, Clone)]
pub struct TagPrediction {
    pub name: String,
    pub confidence: f32,
}

pub struct TagPipeline;

impl TagPipeline {
    pub const CONFIDENCE_THRESHOLD: f32 = 0.35;

    /// 1. Synchronous CPU/ONNX computation — safe for `spawn_blocking`
    pub fn compute_tags_sync(
        engine: &TagEngine,
        img: &DynamicImage,
    ) -> Result<Vec<TagPrediction>, Box<dyn std::error::Error + Send + Sync>> {
        let ranked_tags = engine.tag_image(img, Self::CONFIDENCE_THRESHOLD)?;
        let predictions = ranked_tags
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();
        Ok(predictions)
    }
}