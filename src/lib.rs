//! Shazam audio signatures and async music catalog client.
#![doc = include_str!("../README.md")]

pub mod audio;
mod client;
pub mod http;
pub mod models;
pub mod signature;
pub use client::{ArtistQuery, Client, Config, Endpoints, Pagination};
pub use models::{Recognition, TrackInfo};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid or corrupted Shazam signature")]
    InvalidSignature,
    #[error("signature CRC32 mismatch")]
    Checksum,
    #[error("unsupported sample rate or invalid audio parameters")]
    InvalidAudio,
    #[error("invalid signature generator limits")]
    InvalidLimits,
    #[error("operation cancelled")]
    Cancelled,
    #[error("audio decoder failed: {0}")]
    Decoder(String),
    #[error("HTTP {status}: {body}")]
    Api { status: u16, body: String },
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Wav(#[from] hound::Error),
}
