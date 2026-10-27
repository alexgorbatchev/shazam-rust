use crate::Error;
use rand::Rng;
use reqwest::{Method, Url, header::HeaderMap};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const USER_AGENTS: &[&str] = &[
    "Dalvik/2.1.0 (Linux; U; Android 5.0.2; VS980 4G Build/LRX22G)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.2; SM-T210 Build/KOT49H)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; SM-P905V Build/LMY47X)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.4; Vodafone Smart Tab 4G Build/KTU84P)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.4; SM-G360H Build/KTU84P)",
    "Dalvik/2.1.0 (Linux; U; Android 5.0.2; SM-S920L Build/LRX22G)",
    "Dalvik/2.1.0 (Linux; U; Android 5.0; Fire Pro Build/LRX21M)",
    "Dalvik/2.1.0 (Linux; U; Android 5.0; SM-N9005 Build/LRX21V)",
    "Dalvik/2.1.0 (Linux; U; Android 6.0.1; SM-G920F Build/MMB29K)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.2; SM-G7102 Build/KOT49H)",
    "Dalvik/2.1.0 (Linux; U; Android 5.0; SM-G900F Build/LRX21T)",
    "Dalvik/2.1.0 (Linux; U; Android 6.0.1; SM-G928F Build/MMB29K)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; SM-J500FN Build/LMY48B)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; Coolpad 3320A Build/LMY47V)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.4; SM-J110F Build/KTU84P)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.2; SAMSUNG-SGH-I747 Build/KOT49H)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.2; SAMSUNG-SM-T337A Build/KOT49H)",
    "Dalvik/1.6.0 (Linux; U; Android 4.3; SGH-T999 Build/JSS15J)",
    "Dalvik/2.1.0 (Linux; U; Android 6.0.1; D6603 Build/23.5.A.0.570)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; SM-J700H Build/LMY48B)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.2; HTC6600LVW Build/KOT49H)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; SM-N910G Build/LMY47X)",
    "Dalvik/2.1.0 (Linux; U; Android 5.1.1; SM-N910T Build/LMY47X)",
    "Dalvik/1.6.0 (Linux; U; Android 4.4.4; C6903 Build/14.4.A.0.157)",
    "Dalvik/2.1.0 (Linux; U; Android 6.0.1; SM-G920F Build/MMB29K)",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
    "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

#[derive(Clone)]
pub struct HttpClient {
    pub client: reqwest::Client,
    pub language: String,
    pub user_agent: Option<String>,
    pub attempts: usize,
    pub initial_backoff: Duration,
}
impl HttpClient {
    pub fn new(timeout: Duration, proxy: Option<&str>) -> Result<Self, Error> {
        let mut builder = reqwest::Client::builder()
            .timeout(timeout)
            .retry(reqwest::retry::never());
        if let Some(proxy) = proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        }
        Ok(Self {
            client: builder.build()?,
            language: "en-US".into(),
            user_agent: None,
            attempts: 5,
            initial_backoff: Duration::from_millis(500),
        })
    }
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
        headers: HeaderMap,
        cancel: &CancellationToken,
    ) -> Result<T, Error> {
        if method != Method::GET && method != Method::POST {
            return Err(Error::InvalidRequest(
                "only GET and POST are supported".into(),
            ));
        }
        if self.attempts == 0 {
            return Err(Error::InvalidRequest("attempts must be positive".into()));
        }
        let mut backoff = self.initial_backoff;
        for attempt in 0..self.attempts {
            let ua = self
                .user_agent
                .as_deref()
                .unwrap_or_else(|| USER_AGENTS[rand::rng().random_range(0..USER_AGENTS.len())]);
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .header("X-Shazam-Platform", "IPHONE")
                .header("X-Shazam-AppVersion", "14.1.0")
                .header("Accept", "*/*")
                .header("Accept-Language", &self.language)
                .header("User-Agent", ua)
                .headers(headers.clone());
            if let Some(body) = body {
                request = request.json(body);
            }
            let operation = async {
                let response = request.send().await?;
                let status = response.status().as_u16();
                let bytes = response.bytes().await?;
                if status >= 400 {
                    return Err(Error::Api {
                        status,
                        body: String::from_utf8_lossy(&bytes).into_owned(),
                    });
                }
                Ok(serde_json::from_slice(&bytes)?)
            };
            let result = tokio::select! { biased; _ = cancel.cancelled() => return Err(Error::Cancelled), result = operation => result };
            let retry = match &result {
                Err(Error::Http(_)) => true,
                Err(Error::Api { status, .. }) => [429, 500, 502, 503, 504].contains(status),
                _ => false,
            };
            if !retry || attempt + 1 == self.attempts {
                return result;
            }
            let duration = backoff
                .mul_f64(rand::rng().random_range(0.8..1.2))
                .min(Duration::from_secs(10));
            tokio::select! { biased; _ = cancel.cancelled() => return Err(Error::Cancelled), _ = tokio::time::sleep(duration) => {} }
            backoff = backoff.saturating_mul(2).min(Duration::from_secs(10));
        }
        unreachable!("positive attempts return on their last iteration")
    }
}
