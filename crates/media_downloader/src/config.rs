// crates/media_downloader/src/config.rs

#[derive(Clone, Debug, Default)]
pub struct DownloaderConfig {
    pub chrome_ws_url: Option<String>,
    pub ig_cookie: Option<String>,
    pub ig_csrf_token: Option<String>,
    pub ig_lsd: Option<String>,
    pub ig_fb_dtsg: Option<String>,
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
        }
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