use crate::{audio::Decoder, http::HttpClient};
use reqwest::Url;
use std::time::Duration;
#[derive(Clone)]
pub struct Endpoints {
    pub web: Url,
    pub discovery: Url,
    pub cdn: Url,
    pub recognition: Option<Url>,
    pub locations: Option<Url>,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            web: Url::parse("https://www.shazam.com/").unwrap(),
            discovery: Url::parse("https://amp.shazam.com/").unwrap(),
            cdn: Url::parse("https://cdn.shazam.com/").unwrap(),
            recognition: None,
            locations: None,
        }
    }
}
pub struct Config {
    pub language: String,
    pub country: String,
    pub timezone: String,
    pub timeout: Duration,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
    pub http: Option<HttpClient>,
    pub decoder: Decoder,
    pub endpoints: Endpoints,
    pub locations_ttl: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            language: "en-US".into(),
            country: "GB".into(),
            timezone: "Europe/Moscow".into(),
            timeout: Duration::from_secs(30),
            proxy: None,
            user_agent: None,
            http: None,
            decoder: Decoder::default(),
            endpoints: Endpoints::default(),
            locations_ttl: Duration::from_secs(3600),
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Pagination {
    pub limit: usize,
    pub offset: usize,
}
impl Pagination {
    pub(super) fn pairs(self, default: usize) -> [(String, String); 2] {
        [
            (
                "limit".into(),
                if self.limit == 0 { default } else { self.limit }.to_string(),
            ),
            ("offset".into(), self.offset.to_string()),
        ]
    }
}
#[derive(Debug, Clone, Default)]
pub struct ArtistQuery {
    pub views: Vec<String>,
    pub extend: Vec<String>,
}
