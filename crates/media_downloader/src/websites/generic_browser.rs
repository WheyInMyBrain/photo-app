// src/websites/generic_browser.rs

use crate::models::{ExtractedMediaMetadata, MediaItem, MediaType};
use crate::utils::{
    infer_image_mime, infer_video_mime, is_image_candidate, is_video_url, probe_image_metadata,
    BROWSER_UA,
};
use anyhow::{bail, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::EventRequestWillBeSent;
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use futures::future::join_all;
use futures::StreamExt;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::{sleep, Instant};

use crate::config::DownloaderConfig;

pub async fn sniff(url: &str, config: Option<&DownloaderConfig>) -> Result<ExtractedMediaMetadata> {
    let ws_url = config.and_then(|c| c.chrome_ws_url.as_deref());

    // 1. Check if sidecar Chrome container URL is set, otherwise launch locally
    let (mut browser, mut handler) = if let Some(endpoint) = ws_url {
        Browser::connect(endpoint).await?
    } else {
        let browser_config = BrowserConfig::builder()
            .arg("--headless=new")
            .viewport(None)
            .arg("--disable-blink-features=AutomationControlled")
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--no-sandbox")
            .arg("--disable-extensions")
            .arg("--mute-audio")
            .arg(format!("--user-agent={BROWSER_UA}"))
            .build()
            .map_err(|e| anyhow::anyhow!(e))?;

        Browser::launch(browser_config).await?
    };

    let handle = tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let page = browser.new_page("about:blank").await?;

    let stealth_js = r#"
        Object.defineProperty(navigator, 'webdriver', { get: () => undefined });
        Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'] });
        Object.defineProperty(navigator, 'plugins', { get: () => [1, 2, 3, 4, 5] });
        window.chrome = { runtime: {} };
    "#;
    let _ = page
        .execute(AddScriptToEvaluateOnNewDocumentParams::new(stealth_js.to_string()))
        .await;

    let mut network_events = page.event_listener::<EventRequestWillBeSent>().await?;
    page.goto(url).await?;

    let player_page = page.clone();
    tokio::spawn(async move {
        for _ in 0..12 {
            sleep(Duration::from_millis(800)).await;
            let _ = player_page
                .evaluate(
                    r#"
                    (() => {
                        const triggerInDoc = (doc) => {
                            if (!doc) return false;
                            const selectors = [
                                'button.plyr__control--overlaid',
                                '.vjs-big-play-button',
                                'video',
                                '.player',
                                '.btn-play',
                                '#play',
                                '.play-button',
                                '.jw-display-icon-container'
                            ];
                            for (const s of selectors) {
                                const el = doc.querySelector(s);
                                if (el && typeof el.click === 'function') {
                                    el.click();
                                    return true;
                                }
                            }
                            return false;
                        };

                        triggerInDoc(document);

                        const iframes = document.querySelectorAll('iframe');
                        for (const iframe of iframes) {
                            try {
                                const iDoc = iframe.contentDocument || iframe.contentWindow.document;
                                triggerInDoc(iDoc);
                            } catch (e) {}
                        }
                    })();
                    "#,
                )
                .await;
        }
    });

    let mut video_candidates = Vec::new();
    let mut image_candidates = Vec::new();
    let mut seen = HashSet::new();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut settle_deadline: Option<Instant> = None;

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if let Some(settle) = settle_deadline {
            if now >= settle {
                break;
            }
        }

        let next_timeout = match settle_deadline {
            Some(s) => s.saturating_duration_since(now).min(Duration::from_millis(500)),
            None => deadline.saturating_duration_since(now).min(Duration::from_millis(500)),
        };

        tokio::select! {
            event_opt = network_events.next() => {
                if let Some(event) = event_opt {
                    let req_url = &event.request.url;

                    if is_video_url(req_url) && seen.insert(req_url.clone()) {
                        video_candidates.push(req_url.clone());

                        if req_url.contains("playlist.m3u8") || req_url.contains("master.m3u8") {
                            settle_deadline = Some(Instant::now() + Duration::from_millis(1500));
                        } else if settle_deadline.is_none() {
                            settle_deadline = Some(Instant::now() + Duration::from_millis(2500));
                        }
                    } else if is_image_candidate(req_url) && seen.insert(req_url.clone()) {
                        image_candidates.push(req_url.clone());
                    }
                }
            }
            _ = tokio::time::sleep(next_timeout) => {}
        }
    }

    let title = page
        .get_title()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "web_stream".to_string());

    // Extract any active video poster from DOM
    let poster_from_dom: Option<String> = page
        .evaluate("document.querySelector('video')?.getAttribute('poster') || document.querySelector('meta[property=\"og:image\"]')?.getAttribute('content')")
        .await
        .ok()
        .and_then(|v| v.into_value().ok());

    let _ = browser.close().await;
    handle.abort();

    if video_candidates.is_empty() && image_candidates.is_empty() {
        bail!("Headless browser timed out without capturing any media on: {url}");
    }

    let mut items = Vec::new();

    let master_stream = video_candidates
        .iter()
        .find(|u| u.contains("playlist.m3u8") || u.contains("master.m3u8"))
        .cloned();

    let final_videos = if let Some(master) = master_stream {
        vec![master]
    } else {
        video_candidates
    };

    for v_url in final_videos {
        let mime = infer_video_mime(&v_url);
        items.push(MediaItem::new(
            MediaType::Video,
            mime,
            None,
            None,
            v_url.clone(),
            poster_from_dom.clone(),
            None,
            None,
            Some(url.to_string()),
            v_url,
        ));
    }

    let page_ref = url.to_string();
    let img_tasks = image_candidates.into_iter().map(|img_url| {
        let referer = page_ref.clone();
        async move {
            let probed = probe_image_metadata(&img_url, Some(&referer)).await;
            (img_url, probed)
        }
    });

    let resolved_images = join_all(img_tasks).await;

    // Filter out 0-byte assets or tiny UI icons / tracking beacons (< 60px)
    let mut valid_images: Vec<_> = resolved_images
        .into_iter()
        .filter(|(_, probed)| {
            probed.file_size_bytes.unwrap_or(1) > 0
                && probed
                    .dimensions
                    .as_ref()
                    .map(|d| d.width >= 60 && d.height >= 60)
                    .unwrap_or(true)
        })
        .collect();

    // Sort descending by area (w * h) or file size:
    // First = Highest quality (Master)
    // Last  = Lowest quality (Thumbnail)
    valid_images.sort_by(|(_, a), (_, b)| {
        let area_a = a.dimensions.as_ref().map(|d| d.width * d.height).unwrap_or(0);
        let area_b = b.dimensions.as_ref().map(|d| d.width * d.height).unwrap_or(0);
        area_b
            .cmp(&area_a)
            .then_with(|| b.file_size_bytes.unwrap_or(0).cmp(&a.file_size_bytes.unwrap_or(0)))
    });

    if !valid_images.is_empty() {
        // 1. Highest quality variant
        let (best_url, best_probed) = valid_images.first().cloned().unwrap();

        // 2. Lowest quality variant (or same if only 1 exists)
        let thumb_url = valid_images.last().map(|(u, _)| u.clone()).unwrap();

        let mime = best_probed
            .content_type
            .clone()
            .unwrap_or_else(|| infer_image_mime(&best_url).to_string());

        let variants: Vec<crate::models::MediaVariant> = valid_images
            .iter()
            .map(|(u, p)| crate::models::MediaVariant {
                url: u.clone(),
                dimensions: p.dimensions.clone(),
                file_size_bytes: p.file_size_bytes,
                label: p.dimensions.as_ref().map(|d| format!("{}x{}", d.width, d.height)),
            })
            .collect();

        let mut master_item = MediaItem::new(
            MediaType::Image,
            mime,
            best_probed.dimensions.clone(),
            best_probed.file_size_bytes,
            best_url.clone(),
            Some(thumb_url),
            None,
            None,
            Some(url.to_string()),
            best_url,
        );

        master_item.variants = variants;
        items.push(master_item);
    }

    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "web".to_string());

    Ok(ExtractedMediaMetadata {
        platform: host.clone(),
        author: host,
        caption: title,
        post_text: None,
        published_at: None,
        tags: Vec::new(),
        items,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}