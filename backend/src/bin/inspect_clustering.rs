use sqlx::{sqlite::SqlitePoolOptions, Row};

#[inline]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite://../storage/db/app.db")
        .await?;

    let rows = sqlx::query("SELECT id, face_thumb_path, embedding FROM asset_faces")
        .fetch_all(&pool)
        .await?;

    let mut faces = Vec::new();
    for r in &rows {
        let id: String = r.get("id");
        let path: String = r.get("face_thumb_path");
        let blob: Vec<u8> = r.get("embedding");
        let vec_f32: Vec<f32> = bytemuck::cast_slice(&blob).to_vec();
        faces.push((id, path, vec_f32));
    }

    println!("\n=== Face Similarity Matrix ===");
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            let sim = cosine_similarity(&faces[i].2, &faces[j].2);
            println!(
                "Face [{}] vs Face [{}] -> Cosine Sim: {:.4}",
                &faces[i].0[0..8],
                &faces[j].0[0..8],
                sim
            );
        }
    }
    Ok(())
}