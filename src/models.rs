use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// A recognition response, preserving additional fields supplied by the service.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recognition {
    #[serde(default)]
    pub tagid: String,
    #[serde(default)]
    pub matches: Vec<Value>,
    pub retryms: Option<u64>,
    pub track: Option<TrackInfo>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Track metadata and derived service links. All original metadata is retained.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TrackInfo {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub artist_id: String,
    pub shazam_url: String,
    pub photo_url: String,
    pub spotify_uri_query: String,
    pub apple_music_url: String,
    pub ringtone: String,
    pub spotify_url: String,
    pub spotify_uri: String,
    pub youtube_link: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn host(raw: &str, domain: &str) -> bool {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .is_some_and(|h| h == domain || h.ends_with(&format!(".{domain}")))
}
impl<'de> Deserialize<'de> for TrackInfo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut extra = Map::<String, Value>::deserialize(deserializer)?;
        let mut take = |name: &str| -> Result<String, D::Error> {
            match extra.remove(name) {
                None | Some(Value::Null) => Ok(String::new()),
                Some(value) => serde_json::from_value(value).map_err(serde::de::Error::custom),
            }
        };
        let mut track = Self {
            key: take("key")?,
            title: take("title")?,
            subtitle: take("subtitle")?,
            artist_id: take("artist_id")?,
            shazam_url: take("shazam_url")?,
            photo_url: take("photo_url")?,
            spotify_uri_query: take("spotify_uri_query")?,
            apple_music_url: take("apple_music_url")?,
            ringtone: take("ringtone")?,
            spotify_url: take("spotify_url")?,
            spotify_uri: take("spotify_uri")?,
            youtube_link: take("youtube_link")?,
            extra,
        };
        let raw = Value::Object(track.extra.clone());
        if track.artist_id.is_empty() {
            track.artist_id = text(&raw["artists"][0]["id"]);
        }
        if track.shazam_url.is_empty() && !track.key.is_empty() {
            track.shazam_url = format!("https://www.shazam.com/track/{}", track.key);
        }
        if track.photo_url.is_empty() {
            track.photo_url = text(&raw["images"]["coverarthq"]);
            if track.photo_url.is_empty() {
                track.photo_url = text(&raw["images"]["coverart"]);
            }
        }
        for option in array(&raw["hub"]["options"]) {
            for action in array(&option["actions"]) {
                let uri = text(&action["uri"]);
                if track.apple_music_url.is_empty() && host(&uri, "music.apple.com") {
                    let mut url = reqwest::Url::parse(&uri).expect("validated URL");
                    let id = url
                        .query_pairs()
                        .find(|(k, _)| k == "i")
                        .map(|(_, v)| v.into_owned());
                    url.set_query(None);
                    if let Some(id) = id {
                        url.query_pairs_mut().append_pair("i", &id);
                    }
                    track.apple_music_url = url.to_string();
                }
            }
        }
        for action in array(&raw["hub"]["actions"]) {
            let uri = text(&action["uri"]);
            if track.ringtone.is_empty()
                && !uri.is_empty()
                && (text(&action["name"]).eq_ignore_ascii_case("ringtone")
                    || text(&action["type"]).eq_ignore_ascii_case("ringtone")
                    || uri.contains(".m4r"))
            {
                track.ringtone = uri;
            }
        }
        for provider in array(&raw["hub"]["providers"]) {
            for action in array(&provider["actions"]) {
                let uri = text(&action["uri"]);
                if let Some(query) = uri.strip_prefix("spotify:search:") {
                    track.spotify_uri_query = query.to_owned();
                    track.spotify_uri = uri;
                } else if track.spotify_url.is_empty() && host(&uri, "spotify.com") {
                    track.spotify_url = uri;
                }
            }
        }
        for section in array(&raw["sections"]) {
            if track.youtube_link.is_empty() && text(&section["type"]).eq_ignore_ascii_case("VIDEO")
            {
                track.youtube_link = text(&section["youtubeurl"]);
            }
        }
        Ok(track)
    }
}
