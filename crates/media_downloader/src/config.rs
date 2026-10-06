// crates/media_downloader/src/config.rs

use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct DownloaderConfig {
    pub chrome_ws_url: Option<String>,
    pub ig_cookie: Option<String>,
    pub ig_csrf_token: Option<String>,
    pub ig_lsd: Option<String>,
    pub ig_fb_dtsg: Option<String>,
    pub known_post_ids: Option<HashSet<String>>,
}

impl DownloaderConfig {
    pub fn new(
        chrome_ws_url: Option<String>,
        ig_cookie: Option<String>,
        ig_csrf_token: Option<String>,
        ig_lsd: Option<String>,
        ig_fb_dtsg: Option<String>,
    ) -> Self {
        Self {
            chrome_ws_url,
            ig_cookie,
            ig_csrf_token,
            ig_lsd,
            ig_fb_dtsg,
            known_post_ids: None,
        }
    }

    /// Builder pattern helper to attach a set of previously downloaded post IDs
    pub fn with_known_post_ids(mut self, known_ids: HashSet<String>) -> Self {
        self.known_post_ids = Some(known_ids);
        self
    }

    /// Fast lookup helper: returns true if the post shortcode is already saved
    pub fn is_known_post(&self, post_id: &str) -> bool {
        self.known_post_ids
            .as_ref()
            .map_or(false, |set| set.contains(post_id))
    }

    pub fn has_instagram_auth(&self) -> bool {
        self.ig_cookie.as_ref().map_or(false, |s| !s.trim().is_empty())
            && self.ig_csrf_token.as_ref().map_or(false, |s| !s.trim().is_empty())
    }

    pub fn has_instagram_graphql_auth(&self) -> bool {
        self.has_instagram_auth()
            && self.ig_lsd.as_ref().map_or(false, |s| !s.trim().is_empty())
            && self.ig_fb_dtsg.as_ref().map_or(false, |s| !s.trim().is_empty())
    }
}