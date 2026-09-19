// src/utils/url.rs

use super::patterns::{
    ANCHOR_HREF_RE, A_CLASS_NEXT_RE, A_REL_NEXT_RE, POST_PATH_RE, REL_NEXT_RE, REL_NEXT_REV_RE,
};
use std::collections::HashSet;

pub fn normalize_scraped_url(raw: &str) -> String {
    raw.replace(r"\/", "/")
        .replace("&amp;", "&")
        .replace("&quot;", "")
        .replace("&#38;", "&")
        .trim_matches(|c| c == '"' || c == '\'' || c == '\\' || c == ' ')
        .to_string()
}

pub fn resolve_relative_url(base_url: &str, target: &str) -> String {
    let clean_target = normalize_scraped_url(target);
    if clean_target.starts_with("http://") || clean_target.starts_with("https://") {
        clean_target
    } else if clean_target.starts_with("//") {
        let scheme = if base_url.starts_with("http://") { "http:" } else { "https:" };
        format!("{scheme}{clean_target}")
    } else if let Ok(base) = reqwest::Url::parse(base_url) {
        base.join(&clean_target).map(|u| u.to_string()).unwrap_or_else(|_| clean_target)
    } else {
        clean_target
    }
}

pub fn discover_post_links(current_url: &str, html: &str) -> Vec<String> {
    let mut discovered = Vec::new();
    let mut seen = HashSet::new();

    let target_host = reqwest::Url::parse(current_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();

    for caps in ANCHOR_HREF_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let full_url = resolve_relative_url(current_url, matched.as_str());

            let is_same_domain = reqwest::Url::parse(&full_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h == target_host))
                .unwrap_or(false);

            if !is_same_domain {
                continue;
            }

            if full_url == current_url || (full_url.ends_with('/') && full_url.trim_end_matches('/') == current_url) {
                continue;
            }

            let lower = full_url.to_lowercase();
            if lower.contains("/login")
                || lower.contains("/signup")
                || lower.contains("/register")
                || lower.contains("/logout")
                || lower.contains("/terms")
                || lower.contains("/privacy")
                || lower.contains("/about")
                || lower.contains("/contact")
                || lower.contains("/search")
                || lower.contains("/tag/")
                || lower.contains("/settings")
            {
                continue;
            }

            if POST_PATH_RE.is_match(&full_url) && seen.insert(full_url.clone()) {
                discovered.push(full_url);
            }
        }
    }

    discovered
}

pub fn discover_next_page(current_url: &str, html: &str) -> Option<String> {
    if let Some(caps) = REL_NEXT_RE.captures(html) {
        if let Some(href) = caps.get(1) {
            return Some(resolve_relative_url(current_url, href.as_str()));
        }
    }

    if let Some(caps) = REL_NEXT_REV_RE.captures(html) {
        if let Some(href) = caps.get(1) {
            return Some(resolve_relative_url(current_url, href.as_str()));
        }
    }

    if let Some(caps) = A_REL_NEXT_RE.captures(html) {
        if let Some(href) = caps.get(1) {
            let next_url = resolve_relative_url(current_url, href.as_str());
            if next_url != current_url {
                return Some(next_url);
            }
        }
    }

    if let Some(caps) = A_CLASS_NEXT_RE.captures(html) {
        if let Some(href) = caps.get(1) {
            let next_url = resolve_relative_url(current_url, href.as_str());
            if next_url != current_url {
                return Some(next_url);
            }
        }
    }

    if let Ok(mut parsed) = reqwest::Url::parse(current_url) {
        let mut target_key = None;
        let mut current_page_num = 0u32;

        for (k, v) in parsed.query_pairs() {
            if k == "page" || k == "p" || k == "pg" {
                if let Ok(num) = v.parse::<u32>() {
                    target_key = Some(k.to_string());
                    current_page_num = num;
                    break;
                }
            }
        }

        if let Some(key) = target_key {
            let next_page_num = current_page_num + 1;
            let current_query_pairs: Vec<(String, String)> = parsed
                .query_pairs()
                .filter(|(k, _)| k != &key)
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            parsed.set_query(None);
            {
                let mut serializer = parsed.query_pairs_mut();
                for (k, v) in current_query_pairs {
                    serializer.append_pair(&k, &v);
                }
                serializer.append_pair(&key, &next_page_num.to_string());
            }

            let page_num_str = format!("page={}", next_page_num);
            let p_num_str = format!("p={}", next_page_num);
            if html.contains(&page_num_str) || html.contains(&p_num_str) {
                return Some(parsed.to_string());
            }
        }
    }

    None
}