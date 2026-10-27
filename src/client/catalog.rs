use super::{ArtistQuery, Client, Pagination};
use crate::{Error, TrackInfo};
use reqwest::Url;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
impl Client {
    pub async fn playlist_tracks(
        &self,
        id: &str,
        pagination: Pagination,
        default_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        let mut pairs = pagination.pairs(default_limit).to_vec();
        pairs.extend([
            ("l".into(), self.config.language.clone()),
            ("relate[songs]".into(), "artists,music-videos".into()),
        ]);
        self.get(
            self.web_url(
                &[
                    "services",
                    "amapi",
                    "v1",
                    "catalog",
                    &self.config.country,
                    "playlists",
                    id,
                    "tracks",
                ],
                pairs,
            )?,
            cancel,
        )
        .await
    }
    pub async fn top_world_tracks(
        &self,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.playlist_tracks(&self.chart_playlist(None, None, None, c).await?, p, 200, c)
            .await
    }
    pub async fn top_country_tracks(
        &self,
        country: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.playlist_tracks(
            &self.chart_playlist(Some(country), None, None, c).await?,
            p,
            200,
            c,
        )
        .await
    }
    pub async fn top_city_tracks(
        &self,
        country: &str,
        city: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.playlist_tracks(
            &self
                .chart_playlist(Some(country), Some(city), None, c)
                .await?,
            p,
            200,
            c,
        )
        .await
    }
    pub async fn top_world_genre_tracks(
        &self,
        genre: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.playlist_tracks(
            &self.chart_playlist(None, None, Some(genre), c).await?,
            p,
            100,
            c,
        )
        .await
    }
    pub async fn top_country_genre_tracks(
        &self,
        country: &str,
        genre: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.playlist_tracks(
            &self
                .chart_playlist(Some(country), None, Some(genre), c)
                .await?,
            p,
            200,
            c,
        )
        .await
    }
    pub async fn artist_about(
        &self,
        id: u64,
        query: &ArtistQuery,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        let pairs = [("views", &query.views), ("extend", &query.extend)]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.into(), v.join(",")));
        self.get(
            self.web_url(
                &[
                    "services",
                    "amapi",
                    "v1",
                    "catalog",
                    &self.config.country,
                    "artists",
                    &id.to_string(),
                ],
                pairs,
            )?,
            c,
        )
        .await
    }
    pub async fn artist_albums(
        &self,
        id: u64,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.get(
            self.web_url(
                &[
                    "services",
                    "amapi",
                    "v1",
                    "catalog",
                    &self.config.country,
                    "artists",
                    &id.to_string(),
                    "albums",
                ],
                p.pairs(10),
            )?,
            c,
        )
        .await
    }
    pub async fn search_album(&self, id: u64, c: &CancellationToken) -> Result<Value, Error> {
        let result: Value = self
            .get(
                self.web_url(
                    &[
                        "services",
                        "amapi",
                        "v1",
                        "catalog",
                        &self.config.country,
                        "albums",
                        &id.to_string(),
                    ],
                    [],
                )?,
                c,
            )
            .await?;
        result["data"]
            .as_array()
            .and_then(|v| v.first())
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("album {id}")))
    }
    pub async fn track_about(&self, id: u64, c: &CancellationToken) -> Result<TrackInfo, Error> {
        self.get(
            self.web_url(
                &[
                    "discovery",
                    "v5",
                    &self.config.language,
                    &self.config.country,
                    "web",
                    "-",
                    "track",
                    &id.to_string(),
                ],
                [
                    ("shazamapiversion".into(), "v3".into()),
                    ("video".into(), "v3".into()),
                ],
            )?,
            c,
        )
        .await
    }
    pub async fn related_tracks(
        &self,
        id: u64,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        let limit = if p.limit == 0 { 20 } else { p.limit };
        self.get(
            self.url(
                &self.config.endpoints.cdn,
                &[
                    "shazam",
                    "v3",
                    &self.config.language,
                    &self.config.country,
                    "web",
                    "-",
                    "tracks",
                    &format!("track-similarities-id-{id}"),
                ],
                [
                    ("startFrom".into(), p.offset.to_string()),
                    ("pageSize".into(), limit.to_string()),
                    ("connected".into(), "".into()),
                    ("channel".into(), "".into()),
                ],
            )?,
            c,
        )
        .await
    }
    async fn search(
        &self,
        query: &str,
        p: Pagination,
        artist: bool,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        let pairs = [
            (if artist { "term" } else { "query" }.into(), query.into()),
            (
                if artist { "limit" } else { "numResults" }.into(),
                if p.limit == 0 { 10 } else { p.limit }.to_string(),
            ),
            ("offset".into(), p.offset.to_string()),
            (
                "types".into(),
                if artist { "artists" } else { "songs" }.into(),
            ),
        ];
        self.get(
            self.web_url(
                &[
                    "services",
                    "search",
                    if artist { "v4" } else { "v3" },
                    &self.config.language,
                    &self.config.country,
                    "web",
                    "search",
                ],
                pairs,
            )?,
            c,
        )
        .await
    }
    pub async fn search_artist(
        &self,
        query: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.search(query, p, true, c).await
    }
    pub async fn search_track(
        &self,
        query: &str,
        p: Pagination,
        c: &CancellationToken,
    ) -> Result<Value, Error> {
        self.search(query, p, false, c).await
    }
    pub async fn listening_counter(&self, id: u64, c: &CancellationToken) -> Result<Value, Error> {
        self.get(
            self.web_url(
                &["services", "count", "v2", "web", "track", &id.to_string()],
                [],
            )?,
            c,
        )
        .await
    }
    pub async fn listening_counter_many(
        &self,
        ids: &[u64],
        c: &CancellationToken,
    ) -> Result<Vec<Value>, Error> {
        if ids.is_empty() {
            return Err(Error::InvalidRequest("track IDs must not be empty".into()));
        }
        self.get(
            self.web_url(
                &["services", "count", "v2", "web", "track"],
                ids.iter().map(|id| ("id".into(), id.to_string())),
            )?,
            c,
        )
        .await
    }
    pub async fn youtube_data(&self, link: Url, c: &CancellationToken) -> Result<Value, Error> {
        self.get(link, c).await
    }
}
