use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct AssetTagItem {
    pub tag_id: i64,
    pub name: String,
    pub confidence: f64,
    pub source: String,
}