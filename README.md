# shazam-client

Shazam audio signatures and asynchronous music recognition and catalog access in Rust. This library replaces [goshazam](https://github.com/alexgorbatchev/goshazam).

## What It Does

- **Recognition:** Recognize encoded audio files, bytes, asynchronous readers, PCM audio, or an existing signature.
- **Signatures:** Generate incremental 16 kHz spectral fingerprints and read or write native CRC32-protected Shazam signatures and data URIs.
- **Catalog:** Fetch track details, artist profiles and albums, album details, search results, related songs, listener counts, and YouTube metadata.
- **Charts:** Resolve global, country, city, and genre playlists using cached location metadata.
- **Transport:** Configure language, country, timezone, proxy, timeout, headers, user agent, endpoints, and cancellation.

## How It Works

Native WAV decoding and an FFmpeg fallback produce mono 16 kHz PCM. Overlapping FFT windows identify time and frequency peaks, which are serialized into the native signature format. Recognition submits that signature to Shazam's discovery service; other methods access its catalog and chart endpoints.

## How it Really Works

[RustFFT](https://docs.rs/rustfft/) plans transforms and reuses scratch buffers; [hound](https://docs.rs/hound/) reads integer and float WAV; [crc32fast](https://docs.rs/crc32fast/) and [base64](https://docs.rs/base64/) implement wire primitives. [reqwest](https://docs.rs/reqwest/) supplies HTTP, Rustls, gzip/deflate, and HTTP/SOCKS proxies. Tokio manages subprocesses and cancellation. Catalog responses use lossless `serde_json::Value`; recognition and track responses expose typed convenience fields and preserve additional metadata.

The signature pipeline is implemented directly because the inspected SongRec libraries have GPL licensing and the inspected alternative fingerprint crates implement local indexes rather than Shazam's remote signature protocol. Golden tests compare generated signature URIs against the original Go implementation.

Requests use five attempts for transport failures and statuses 429, 500, 502, 503, and 504. Backoff starts at 500 ms, doubles with 20% jitter, and caps at ten seconds. Bodies and caller headers are retained across attempts. JSON decoding and other status failures return immediately. Each default HTTP attempt has a 30-second timeout; cancellation interrupts requests, backoff, reader operations, and decoder processes. Location metadata caches for one hour with concurrent requests sharing a single fetch. All endpoints can be replaced for local testing or mirrors.

## Prerequisites

Rust 1.88 or later. FFmpeg on `PATH` is required for compressed audio and other formats that hound cannot decode; integer and float WAV require no external decoder. Recognition and catalog methods require network access to Shazam's undocumented endpoints, whose server behavior can change independently of this library.

## Installation

From a sibling project:

```sh
cargo add shazam-client --path ../shazam
cargo add tokio --features macros,rt-multi-thread
cargo add tokio-util
```

## Quick Start

```rust,no_run
use shazam_client::{Client, Config};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), shazam_client::Error> {
    let client = Client::new(Config::default())?;
    let result = client.recognize_file("recording.mp3", &CancellationToken::new()).await?;
    if let Some(track) = result.track {
        println!("{} — {}", track.subtitle, track.title);
    }
    Ok(())
}
```

## API

`Client::new(Config)` configures a client. `recognize_file`, `recognize_bytes`, `recognize_reader`, `recognize_audio`, and `recognize_signature` cover supported inputs. Audio shorter than one 128-sample frame returns an empty recognition response without a request. `audio::Audio::new` validates PCM; `samples()`, `sample_rate()`, and `channels()` inspect it. `audio::Decoder` exposes file, byte, and reader decoding and accepts a custom FFmpeg executable.

`signature::Generator` accepts normalized PCM through `feed` and returns successive snippets through `next_signature`. Default limits are twelve seconds or 255 peaks, checked at frame boundaries; `with_limits` customizes them. `reset` discards pending input. `Signature` supports `encode`, `decode`, `to_uri`, `from_uri`, duration, and peak counts. Long audio uses a centered starting excerpt, matching the source selection policy.

Catalog methods are `track_about`, `artist_about`, `artist_albums`, `search_album`, `related_tracks`, `search_artist`, `search_track`, `listening_counter`, `listening_counter_many`, and `youtube_data`. `ArtistQuery` supplies views and extensions. `Pagination` uses a zero limit to select the method default: 200 for charts, 100 for global genre charts, 20 for related tracks, and 10 for search and artist albums. Chart methods are `top_world_tracks`, `top_country_tracks`, `top_city_tracks`, `top_world_genre_tracks`, and `top_country_genre_tracks`. `locations` and `chart_playlist` expose chart resolution; `set_locations_url` changes its endpoint and invalidates the cache.

`TrackInfo` derives artist, artwork, Apple Music, Spotify, Shazam, ringtone, and video links. Native serde serialization replaces the Go JSON decoding helper. Typed errors distinguish invalid input, signatures, CRC failures, cancellation, decoder failures, missing resources, HTTP status responses, and underlying I/O or JSON failures.

Corrected source defects: reset discards old audio instead of retaining processed input; descending signature peaks return an error instead of being silently dropped; unrelated second hub actions are no longer labeled as ringtones; music service links validate hostname boundaries instead of matching substrings; invalid proxies return configuration errors. PCM parameters are immutable after validated construction. Live service availability has not been verified by local tests.

## License

MIT. Original protocol and algorithm attribution: Alex Gorbatchev and dotX12; see [LICENSE](LICENSE). Dependency licenses are retained by Cargo packages.
