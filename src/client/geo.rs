use super::Client;
use crate::Error;
use reqwest::Url;
use serde_json::Value;
use std::{sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;
impl Client {
    pub async fn locations(&self, cancel: &CancellationToken) -> Result<Arc<Value>, Error> {
        let mut cached = tokio::select! { biased; _ = cancel.cancelled() => return Err(Error::Cancelled), cache = self.locations.lock() => cache };
        if let Some((at, value)) = cached.as_ref()
            && at.elapsed() < self.config.locations_ttl
        {
            return Ok(value.clone());
        }
        let url = match &self.config.endpoints.locations {
            Some(url) => url.clone(),
            None => self.web_url(&["services", "charts", "locations"], [])?,
        };
        let value: Arc<Value> = Arc::new(self.get(url, cancel).await?);
        *cached = Some((Instant::now(), value.clone()));
        Ok(value)
    }
    pub async fn set_locations_url(&mut self, url: Url) {
        self.config.endpoints.locations = Some(url);
        *self.locations.get_mut() = None;
    }
    pub async fn chart_playlist(
        &self,
        country: Option<&str>,
        city: Option<&str>,
        genre: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<String, Error> {
        let locations = self.locations(cancel).await?;
        let region = if let Some(country) = country {
            locations["countries"]
                .as_array()
                .and_then(|countries| {
                    countries.iter().find(|v| {
                        v["id"]
                            .as_str()
                            .is_some_and(|v| v.eq_ignore_ascii_case(country))
                    })
                })
                .ok_or_else(|| Error::NotFound(format!("country {country}")))?
        } else {
            &locations["global"]
        };
        let chart = if let Some(city) = city {
            region["cities"]
                .as_array()
                .and_then(|cities| {
                    cities.iter().find(|v| {
                        v["name"]
                            .as_str()
                            .is_some_and(|v| v.eq_ignore_ascii_case(city))
                    })
                })
                .ok_or_else(|| Error::NotFound(format!("city {city}")))?
        } else if let Some(genre) = genre {
            region["genres"]
                .as_array()
                .and_then(|genres| {
                    genres.iter().find(|v| {
                        ["name", "urlName"].iter().any(|key| {
                            v[key]
                                .as_str()
                                .is_some_and(|v| v.eq_ignore_ascii_case(genre))
                        })
                    })
                })
                .ok_or_else(|| Error::NotFound(format!("genre {genre}")))?
        } else if country.is_none() {
            &region["top"]
        } else {
            region
        };
        chart["listid"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| Error::NotFound("chart playlist id".into()))
    }
}
