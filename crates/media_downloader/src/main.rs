// src/main.rs

use anyhow::Result;
use media_downloader::{download_media, extract_media, MediaType};
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_filename("../.env");
    }

    let args: Vec<String> = env::args().collect();
    let target_url = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("https://v.redd.it/ktpbenh0b0qh1"); // Or your test URL

    println!(">>> Extracting metadata for: {target_url}");
    let meta = extract_media(target_url).await?;

    println!("\n=== METADATA SUMMARY ===");
    println!("Platform   : {}", meta.platform);
    println!("Author     : {}", meta.author);
    println!("Caption    : {}", meta.caption);
    println!("Total Items: {}", meta.items.len());

    let output_dir = PathBuf::from("./downloads");
    tokio::fs::create_dir_all(&output_dir).await?;

    println!("\n=== STARTING DOWNLOAD PIPELINE ===");

    for (idx, item) in meta.items.iter().enumerate() {
        // Determine file extension
        let ext = match item.media_type {
            MediaType::Video => "mp4",
            MediaType::Image => {
                if item.high_res_url.contains(".png") {
                    "png"
                } else if item.high_res_url.contains(".webp") {
                    "webp"
                } else if item.high_res_url.contains(".gif") {
                    "gif"
                } else {
                    "jpg"
                }
            }
        };

        let file_name = format!("{}_{}_{}.{}", meta.platform, meta.author, idx + 1, ext);
        let dest_path = output_dir.join(&file_name);

        println!("\n[{}/{}] Downloading asset...", idx + 1, meta.items.len());
        println!("  • Video/Image : {}", item.high_res_url);
        if let Some(ref audio) = item.audio_url {
            println!("  • Audio Track : {audio}");
        }
        println!("  • Saving To   : {}", dest_path.display());

        match download_media(
            &item.high_res_url,
            item.audio_url.as_deref(),
            &dest_path,
            item.referer_required.as_deref(),
        )
        .await
        {
            Ok(saved_path) => {
                let size = tokio::fs::metadata(&saved_path).await?.len();
                println!("  ✓ Download complete! ({} bytes)", size);
            }
            Err(e) => {
                eprintln!("  ✗ Download failed: {e}");
            }
        }
    }

    println!("\nAll operations finished.");
    Ok(())
}