// crates/media_downloader/src/config.rs

#[derive(Clone, Debug, Default)]
pub struct DownloaderConfig {
    pub chrome_ws_url: Option<String>,
    pub ig_cookie: Option<String>,
    pub ig_csrf_token: Option<String>,
}

impl DownloaderConfig {
    pub fn new(
        chrome_ws_url: Option<String>,
        ig_cookie: Option<String>,
        ig_csrf_token: Option<String>,
    ) -> Self {
        Self {
            chrome_ws_url,
            ig_cookie,
            ig_csrf_token,
        }
    }

    pub fn has_instagram_auth(&self) -> bool {
        self.ig_cookie.as_ref().map_or(false, |s| !s.trim().is_empty())
            && self.ig_csrf_token.as_ref().map_or(false, |s| !s.trim().is_empty())
    }
}