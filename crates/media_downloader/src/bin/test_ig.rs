// crates/media_downloader/src/bin/test_ig.rs

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, REFERER, USER_AGENT};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const TARGET_USERNAME: &str = "nilavikrishnan";
const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const APP_ID: &str = "936619743392459";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("TESTING PUBLIC (COOKIELESS) INSTAGRAM APIS");
    println!("Target: @{}", TARGET_USERNAME);
    println!("==================================================");

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    headers.insert("X-IG-App-ID", HeaderValue::from_static(APP_ID));
    headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));

    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .timeout(Duration::from_secs(15))
        .build()?;

    // -------------------------------------------------------------------------
    // METHOD 1: Public Web Profile Info (Guest Mode)
    // -------------------------------------------------------------------------
    println!("\n--- Method 1: Public web_profile_info (no cookies) ---");
    let info_url = format!(
        "https://www.instagram.com/api/v1/users/web_profile_info/?username={}",
        TARGET_USERNAME
    );
    let resp = client
        .get(&info_url)
        .header(
            REFERER,
            format!("https://www.instagram.com/{}/", TARGET_USERNAME),
        )
        .send()
        .await?;

    let status = resp.status();
    println!("Status: {}", status);
    let body = resp.text().await.unwrap_or_default();
    println!("Body length: {} bytes", body.len());

    if status.is_success() && body.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            if let Some(edges) = v
                .pointer("/data/user/edge_owner_to_timeline_media/edges")
                .and_then(|e| e.as_array())
            {
                println!("[+] SUCCESS via Method 1! Retrieved {} posts:", edges.len());
                for (i, edge) in edges.iter().enumerate() {
                    let code = edge
                        .pointer("/node/shortcode")
                        .and_then(|c| c.as_str())
                        .unwrap_or("???");
                    let is_video = edge
                        .pointer("/node/is_video")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    println!(
                        "    #{:02} [{}] https://www.instagram.com/p/{}/",
                        i + 1,
                        if is_video { "Video" } else { "Photo" },
                        code
                    );
                }
                return Ok(());
            }
        }
    } else {
        println!("Method 1 response preview: {}", &body[..body.len().min(250)]);
    }

    // -------------------------------------------------------------------------
    // METHOD 2: Public Profile Page Scrape & SSR Extraction
    // -------------------------------------------------------------------------
    println!("\n--- Method 2: Public Profile HTML Extraction ---");
    let page_resp = client
        .get(format!("https://www.instagram.com/{}/", TARGET_USERNAME))
        .send()
        .await?;

    let page_status = page_resp.status();
    println!("Page status: {}", page_status);
    let page_html = page_resp.text().await.unwrap_or_default();
    println!("Page HTML size: {} bytes", page_html.len());

    // Search for post shortcodes embedded in public HTML
    let mut codes = Vec::new();
    let pattern = "/p/";
    let mut offset = 0;
    while let Some(pos) = page_html[offset..].find(pattern) {
        let abs_pos = offset + pos + pattern.len();
        let remaining = &page_html[abs_pos..];
        if let Some(end) = remaining.find('/') {
            let code = &remaining[..end];
            if !code.is_empty()
                && code.len() <= 15
                && !code.contains('<')
                && !code.contains('"')
                && !codes.contains(&code.to_string())
            {
                codes.push(code.to_string());
            }
        }
        offset = abs_pos;
    }

    if !codes.is_empty() {
        println!(
            "[+] SUCCESS via Method 2! Found {} post shortcodes in public page HTML:",
            codes.len()
        );
        for (i, code) in codes.iter().enumerate() {
            println!("    #{:02} https://www.instagram.com/p/{}/", i + 1, code);
        }
        return Ok(());
    } else {
        println!("No shortcodes found in HTML.");
    }

    // -------------------------------------------------------------------------
    // METHOD 3: Public Shared Data Query (?__a=1&__d=dis)
    // -------------------------------------------------------------------------
    println!("\n--- Method 3: Public ?__a=1&__d=dis Query ---");
    let a_url = format!("https://www.instagram.com/{}/?__a=1&__d=dis", TARGET_USERNAME);
    let a_resp = client
        .get(&a_url)
        .header(
            REFERER,
            format!("https://www.instagram.com/{}/", TARGET_USERNAME),
        )
        .send()
        .await?;

    let a_status = a_resp.status();
    println!("Status: {}", a_status);
    let a_body = a_resp.text().await.unwrap_or_default();
    println!("Length: {} bytes", a_body.len());
    if a_body.starts_with('{') {
        println!("Returned JSON: {}", &a_body[..a_body.len().min(300)]);
    } else {
        println!("Preview: {}", &a_body[..a_body.len().min(250)]);
    }

    Ok(())
}