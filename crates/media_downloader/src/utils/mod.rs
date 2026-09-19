// src/utils/mod.rs

pub mod media;
pub mod page;
pub mod patterns;
pub mod url;

pub use media::*;
pub use page::*;
pub use patterns::*;
pub use url::*;

pub const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";