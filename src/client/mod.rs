use crate::{
    Error, Recognition,
    audio::{Audio, Decoder},
    http::HttpClient,
    signature::Signature,
};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncRead;
use tokio_util::sync::CancellationToken;

mod catalog;
mod config;
mod geo;
pub use config::{ArtistQuery, Config, Endpoints, Pagination};
pub struct Client {
    pub http: HttpClient,
    pub decoder: Decoder,
    config: Config,
    locations: tokio::sync::Mutex<Option<(Instant, Arc<Value>)>>,
}
impl Client {
    pub fn new(mut config: Config) -> Result<Self, Error> {
        if config.language.is_empty() || config.country.is_empty() {
            return Err(Error::InvalidRequest(
                "language and country must not be empty".into(),
            ));
        }
        let mut http = match config.http.take() {
            Some(http) => http,
            None => HttpClient::new(config.timeout, config.proxy.as_deref())?,
        };
        http.language = config.language.clone();
        if config.user_agent.is_some() {
            http.user_agent = config.user_agent.clone();
        }
        Ok(Self {
            http,
            decoder: config.decoder.clone(),
            config,
            locations: tokio::sync::Mutex::new(None),
        })
    }
    fn url(
        &self,
        base: &Url,
        parts: &[&str],
        pairs: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Url, Error> {
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::InvalidRequest("endpoint cannot contain path segments".into()))?
            .pop_if_empty()
            .extend(parts);
        url.query_pairs_mut().extend_pairs(pairs);
        Ok(url)
    }
    async fn get<T: DeserializeOwned>(
        &self,
        url: Url,
        cancel: &CancellationToken,
    ) -> Result<T, Error> {
        self.http
            .request(Method::GET, url, None, Default::default(), cancel)
            .await
    }
    fn web_url(
        &self,
        parts: &[&str],
        pairs: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Url, Error> {
        self.url(&self.config.endpoints.web, parts, pairs)
    }
    pub async fn recognize_signature(
        &self,
        signature: Option<&Signature>,
        cancel: &CancellationToken,
    ) -> Result<Recognition, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let Some(signature) = signature else {
            return Ok(Recognition::default());
        };
        let body = json!({"timezone":self.config.timezone,"signature":{"uri":signature.to_uri()?,"samplems": signature.number_samples as u64 * 1000 / signature.sample_rate as u64},"timestamp":SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| Error::InvalidRequest(e.to_string()))?.as_millis() as u64,"context":{},"geolocation":{}});
        let endpoint = match &self.config.endpoints.recognition {
            Some(url) => url.clone(),
            None => self.url(
                &self.config.endpoints.discovery,
                &[
                    "discovery",
                    "v5",
                    &self.config.language,
                    &self.config.country,
                    "iphone",
                    "-",
                    "tag",
                    &uuid::Uuid::new_v4().to_string(),
                    &uuid::Uuid::new_v4().to_string(),
                ],
                [
                    ("sync", "true"),
                    ("webv3", "true"),
                    ("sampling", "true"),
                    ("connected", ""),
                    ("shazamapiversion", "v3"),
                    ("sharehub", "true"),
                    ("hubv5minorversion", "v5.1"),
                    ("hidelb", "true"),
                    ("video", "v3"),
                ]
                .map(|(k, v)| (k.into(), v.into())),
            )?,
        };
        self.http
            .request(
                Method::POST,
                endpoint,
                Some(&body),
                Default::default(),
                cancel,
            )
            .await
    }
    pub async fn recognize_audio(
        &self,
        audio: &Audio,
        cancel: &CancellationToken,
    ) -> Result<Recognition, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.recognize_signature(
            audio.signature_generator().next_signature().as_ref(),
            cancel,
        )
        .await
    }
    pub async fn recognize_bytes(
        &self,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<Recognition, Error> {
        self.recognize_audio(&self.decoder.decode_bytes(bytes, cancel).await?, cancel)
            .await
    }
    pub async fn recognize_file(
        &self,
        path: impl AsRef<Path>,
        cancel: &CancellationToken,
    ) -> Result<Recognition, Error> {
        self.recognize_audio(&self.decoder.decode_file(path, cancel).await?, cancel)
            .await
    }
    pub async fn recognize_reader(
        &self,
        reader: impl AsyncRead + Unpin,
        cancel: &CancellationToken,
    ) -> Result<Recognition, Error> {
        self.recognize_audio(&self.decoder.decode_reader(reader, cancel).await?, cancel)
            .await
    }
}
