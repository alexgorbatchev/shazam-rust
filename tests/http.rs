use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    response::Response,
};
use reqwest::{Method, Url};
use serde_json::{Value, json};
use shazam_client::{
    ArtistQuery, Client, Config, Endpoints, Error, Pagination, TrackInfo, http::HttpClient,
    signature::Signature,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Server {
    requests: Mutex<Vec<(String, reqwest::header::HeaderMap, Value)>>,
}
async fn handler(State(state): State<Arc<Server>>, request: Request) -> Response {
    let uri = request.uri().to_string();
    if uri.starts_with("/slow") {
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    if uri.starts_with("/invalid-json") {
        return Response::new(Body::from("{"));
    }
    if uri.starts_with("/gzip") {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"{\"compressed\":true}").unwrap();
        return Response::builder()
            .header("content-encoding", "gzip")
            .body(Body::from(encoder.finish().unwrap()))
            .unwrap();
    }
    let headers = request.headers().clone();
    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut requests = state.requests.lock().unwrap();
    requests.push((uri.clone(), headers, body));
    let attempts = requests.iter().filter(|(u, _, _)| u == &uri).count();
    let (status, body) = if uri.starts_with("/retry") && attempts < 3 {
        (503, json!({"error":"retry"}))
    } else if uri.starts_with("/always") {
        (429, json!({"error":"later"}))
    } else if uri.starts_with("/bad") {
        (400, json!({"error":"bad"}))
    } else if uri.starts_with("/services/charts/locations") {
        (
            200,
            json!({"countries":[{"id":"GB","listid":"country","cities":[{"name":"London","listid":"city"}],"genres":[{"name":"Pop","urlName":"pop","listid":"country-genre"}]}],"global":{"top":{"listid":"world"},"genres":[{"name":"Pop","urlName":"pop","listid":"world-genre"}]}}),
        )
    } else if uri.starts_with("/services/count/v2/web/track?") {
        (200, json!([{"total":42}]))
    } else if uri.contains("/albums/99") {
        (200, json!({"data":[]}))
    } else if uri.contains("/albums/") {
        (
            200,
            json!({"data":[{"id":"album","attributes":{"name":"Album"}}]}),
        )
    } else if uri.starts_with("/discovery/v5") && uri.contains("/track/") {
        (200, json!({"key":"42","title":"Title"}))
    } else if uri.starts_with("/recognize") || uri.contains("/tag/") {
        (
            200,
            json!({"tagid":"tag","matches":[{"id":"42"}],"track":{"key":"42","title":"Title"},"newField":true}),
        )
    } else {
        (200, json!({"ok":true}))
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
async fn server() -> (Url, Arc<Server>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Server::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let router = Router::new().fallback(handler).with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, state, task)
}
#[tokio::test]
async fn complete_catalog_and_geo_contracts() {
    let (base, state, task) = server().await;
    let endpoints = Endpoints {
        web: base.clone(),
        discovery: base.clone(),
        cdn: base.clone(),
        recognition: None,
        locations: None,
    };
    let mut client = Client::new(Config {
        endpoints,
        user_agent: Some("test-agent".into()),
        ..Default::default()
    })
    .unwrap();
    let c = CancellationToken::new();
    let p = Pagination::default();
    assert_eq!(client.top_world_tracks(p, &c).await.unwrap()["ok"], true);
    client.top_country_tracks("gb", p, &c).await.unwrap();
    client.top_city_tracks("GB", "london", p, &c).await.unwrap();
    client.top_world_genre_tracks("POP", p, &c).await.unwrap();
    client
        .top_country_genre_tracks("gb", "Pop", p, &c)
        .await
        .unwrap();
    for (country, city, genre) in [
        (Some("xx"), None, None),
        (Some("gb"), Some("missing"), None),
        (None, None, Some("missing")),
    ] {
        assert!(matches!(
            client.chart_playlist(country, city, genre, &c).await,
            Err(Error::NotFound(_))
        ));
    }
    let a = client.locations(&c).await.unwrap();
    let b = client.locations(&c).await.unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    client
        .artist_about(
            42,
            &ArtistQuery {
                views: vec!["top-songs".into(), "similar-artists".into()],
                extend: vec!["artistBio".into()],
            },
            &c,
        )
        .await
        .unwrap();
    client
        .artist_albums(
            42,
            Pagination {
                limit: 3,
                offset: 2,
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(client.search_album(42, &c).await.unwrap()["id"], "album");
    assert!(matches!(
        client.search_album(99, &c).await,
        Err(Error::NotFound(_))
    ));
    assert_eq!(client.track_about(42, &c).await.unwrap().title, "Title");
    client.related_tracks(42, p, &c).await.unwrap();
    client.search_artist("A & B/é", p, &c).await.unwrap();
    client
        .search_track(
            "Track?",
            Pagination {
                limit: 2,
                offset: 1,
            },
            &c,
        )
        .await
        .unwrap();
    client.listening_counter(42, &c).await.unwrap();
    assert_eq!(
        client.listening_counter_many(&[42, 43], &c).await.unwrap()[0]["total"],
        42
    );
    assert!(client.listening_counter_many(&[], &c).await.is_err());
    client
        .youtube_data(base.join("video").unwrap(), &c)
        .await
        .unwrap();
    let signature = Signature::new(16000).unwrap();
    let response = client
        .recognize_signature(Some(&signature), &c)
        .await
        .unwrap();
    assert_eq!(response.track.unwrap().key, "42");
    assert_eq!(response.extra["newField"], true);
    assert!(
        client
            .recognize_signature(None, &c)
            .await
            .unwrap()
            .matches
            .is_empty()
    );
    {
        let requests = state.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(u, _, _)| u.starts_with("/services/charts/locations"))
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .any(|(u, _, _)| u.contains("playlists/world-genre/tracks?limit=100&offset=0"))
        );
        assert!(
            requests
                .iter()
                .any(|(u, _, _)| u.contains("term=A+%26+B%2F%C3%A9"))
        );
        assert!(requests.iter().any(|(u, _, _)| u.contains("id=42&id=43")));
        let (uri, headers, body) = requests
            .iter()
            .find(|(u, _, _)| u.contains("/tag/"))
            .unwrap();
        assert!(uri.contains("/en-US/GB/iphone/-/tag/"));
        assert_eq!(headers["user-agent"], "test-agent");
        assert_eq!(headers["x-shazam-platform"], "IPHONE");
        assert_eq!(body["signature"]["samplems"], 0);
        assert_eq!(body["timezone"], "Europe/Moscow");
        Signature::from_uri(body["signature"]["uri"].as_str().unwrap()).unwrap();
    }
    client
        .set_locations_url(base.join("services/charts/locations").unwrap())
        .await;
    assert!(!Arc::ptr_eq(&a, &client.locations(&c).await.unwrap()));
    c.cancel();
    assert!(matches!(client.locations(&c).await, Err(Error::Cancelled)));
    assert!(matches!(
        client.recognize_signature(None, &c).await,
        Err(Error::Cancelled)
    ));
    task.abort();
}

#[tokio::test]
async fn retries_errors_and_cancellation() {
    let (base, state, task) = server().await;
    let mut http = HttpClient::new(Duration::from_secs(2), None).unwrap();
    http.initial_backoff = Duration::from_millis(1);
    let c = CancellationToken::new();
    let compressed: Value = http
        .request(
            Method::GET,
            base.join("gzip").unwrap(),
            None,
            Default::default(),
            &c,
        )
        .await
        .unwrap();
    assert_eq!(compressed["compressed"], true);
    assert!(matches!(
        http.request::<Value>(
            Method::GET,
            base.join("invalid-json").unwrap(),
            None,
            Default::default(),
            &c
        )
        .await,
        Err(Error::Json(_))
    ));
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-test", "passed".parse().unwrap());
    let body = json!({"exact":"bytes"});
    let result: Value = http
        .request(
            Method::POST,
            base.join("retry").unwrap(),
            Some(&body),
            headers,
            &c,
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], true);
    {
        let requests = state.requests.lock().unwrap();
        let retries: Vec<_> = requests.iter().filter(|(u, _, _)| u == "/retry").collect();
        assert_eq!(retries.len(), 3);
        assert!(
            retries
                .iter()
                .all(|(_, h, b)| h["x-test"] == "passed" && b == &body)
        );
    }
    assert!(matches!(
        http.request::<Value>(
            Method::GET,
            base.join("bad").unwrap(),
            None,
            Default::default(),
            &c
        )
        .await,
        Err(Error::Api { status: 400, .. })
    ));
    assert!(matches!(
        http.request::<Value>(
            Method::GET,
            base.join("always").unwrap(),
            None,
            Default::default(),
            &c
        )
        .await,
        Err(Error::Api { status: 429, .. })
    ));
    assert!(
        http.request::<Value>(Method::DELETE, base.clone(), None, Default::default(), &c)
            .await
            .is_err()
    );
    http.attempts = 0;
    assert!(
        http.request::<Value>(Method::GET, base.clone(), None, Default::default(), &c)
            .await
            .is_err()
    );
    http.attempts = 5;
    http.initial_backoff = Duration::from_secs(10);
    let cancel = c.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });
    assert!(matches!(
        http.request::<Value>(
            Method::GET,
            base.join("always-new").unwrap(),
            None,
            Default::default(),
            &c
        )
        .await,
        Err(Error::Cancelled)
    ));
    assert!(HttpClient::new(Duration::from_secs(1), Some("http://[invalid")).is_err());
    assert!(
        Client::new(Config {
            language: String::new(),
            ..Default::default()
        })
        .is_err()
    );
    task.abort();
}

#[tokio::test]
async fn truncated_responses_retry_and_inflight_requests_cancel() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        for attempt in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let byte = stream.read_u8().await.unwrap();
                request.push(byte);
                assert!(request.len() < 16384);
            }
            let reply: &[u8] = if attempt < 2 {
                b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nConnection: close\r\n\r\n{"
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}"
            };
            stream.write_all(reply).await.unwrap();
        }
    });
    let mut http = HttpClient::new(Duration::from_secs(2), None).unwrap();
    http.initial_backoff = Duration::from_millis(1);
    let result: Value = http
        .request(
            Method::GET,
            url,
            None,
            Default::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], true);
    task.await.unwrap();
    let (base, _, task) = server().await;
    let cancel = CancellationToken::new();
    let cloned = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cloned.cancel()
    });
    assert!(matches!(
        http.request::<Value>(
            Method::GET,
            base.join("slow").unwrap(),
            None,
            Default::default(),
            &cancel
        )
        .await,
        Err(Error::Cancelled)
    ));
    task.abort();
}

#[test]
fn track_links_preserve_metadata_and_correct_source_defects() {
    assert!(serde_json::from_value::<TrackInfo>(json!({"key":42})).is_err());
    let raw = json!({"key":"42","title":"Title","subtitle":"Artist","artists":[{"id":"7"}],"images":{"coverarthq":"hq"},"hub":{"options":[{"actions":[{"uri":"https://music.apple.com/gb/album/a?i=42&tracking=remove"}]}],"actions":[{"uri":"first"},{"uri":"https://open.spotify.com/track/42"}],"providers":[{"actions":[{"uri":"spotify:search:Artist+Title"},{"uri":"https://spotify.com.evil.example/track/42"},{"uri":"https://open.spotify.com/track/42"}]}]},"sections":[{"type":"video","youtubeurl":"video"}],"unknown":{"retained":true}});
    let track: TrackInfo = serde_json::from_value(raw).unwrap();
    assert_eq!(track.artist_id, "7");
    assert_eq!(track.shazam_url, "https://www.shazam.com/track/42");
    assert_eq!(track.photo_url, "hq");
    assert_eq!(
        track.apple_music_url,
        "https://music.apple.com/gb/album/a?i=42"
    );
    assert_eq!(track.spotify_uri_query, "Artist+Title");
    assert_eq!(track.spotify_url, "https://open.spotify.com/track/42");
    assert_eq!(track.youtube_link, "video");
    assert!(track.ringtone.is_empty());
    assert_eq!(
        serde_json::to_value(track).unwrap()["unknown"]["retained"],
        true
    );
    let track:TrackInfo=serde_json::from_value(json!({"images":{"coverart":"cover"},"hub":{"options":[{"actions":[{"uri":"https://music.apple.com/a?tracking=remove"}]}],"actions":[{"type":"ringtone","uri":"ringtone"}]}})).unwrap();
    assert_eq!(track.photo_url, "cover");
    assert_eq!(track.ringtone, "ringtone");
    assert_eq!(track.apple_music_url, "https://music.apple.com/a");
    assert!(serde_json::from_str::<TrackInfo>("[]").is_err());
}
