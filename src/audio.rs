use crate::{Error, signature::Generator};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audio {
    samples: Vec<i16>,
    sample_rate: u32,
    channels: usize,
}

impl Audio {
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn new(samples: Vec<i16>, sample_rate: u32, channels: usize) -> Result<Self, Error> {
        if sample_rate == 0 || channels == 0 || !samples.len().is_multiple_of(channels) {
            return Err(Error::InvalidAudio);
        }
        Ok(Self {
            samples,
            sample_rate,
            channels,
        })
    }
    pub fn from_pcm16le(bytes: &[u8], sample_rate: u32, channels: usize) -> Result<Self, Error> {
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::InvalidAudio);
        }
        Self::new(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|x| i16::from_le_bytes([x[0], x[1]]))
                .collect(),
            sample_rate,
            channels,
        )
    }
    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64 / self.channels as f64
    }
    pub fn to_mono(&self) -> Self {
        Self {
            samples: self
                .samples
                .chunks_exact(self.channels)
                .map(|frame| {
                    (frame.iter().map(|x| *x as i64).sum::<i64>() / self.channels as i64) as i16
                })
                .collect(),
            sample_rate: self.sample_rate,
            channels: 1,
        }
    }
    pub fn resample(&self, target: u32) -> Result<Self, Error> {
        if target == 0 {
            return Err(Error::InvalidAudio);
        }
        if self.sample_rate == target {
            return Ok(self.clone());
        }
        let mono = self.to_mono();
        let ratio = mono.sample_rate as f64 / target as f64;
        let length = (mono.samples.len() as f64 / ratio).floor() as usize;
        let samples = (0..length)
            .map(|i| {
                let position = i as f64 * ratio;
                let first = position as usize;
                let second = (first + 1).min(mono.samples.len() - 1);
                (mono.samples[first] as f64
                    + (position - first as f64)
                        * (mono.samples[second] as f64 - mono.samples[first] as f64))
                    as i16
            })
            .collect();
        Self::new(samples, target, 1)
    }
    pub fn normalize(&self) -> Self {
        self.to_mono()
            .resample(16000)
            .expect("positive normalization rate")
    }
    pub fn signature_generator(&self) -> Generator {
        let normalized = self.normalize();
        let duration = normalized.duration_seconds();
        let skip = if duration > 36.0 {
            (duration / 2.0) as usize - 6
        } else {
            0
        };
        let mut generator = Generator::default();
        generator.feed(&normalized.samples[skip * 16000..]);
        generator
    }
}

/// Decode native integer/float WAV without external processes.
pub fn decode_wav(bytes: &[u8]) -> Result<Audio, Error> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes))?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| Ok((sample?.clamp(-1.0, 1.0) * 32767.0) as i16))
            .collect::<Result<_, hound::Error>>()?,
        hound::SampleFormat::Int => {
            let shift = spec.bits_per_sample as i32 - 16;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample.map(|sample| {
                        if shift >= 0 {
                            (sample >> shift) as i16
                        } else {
                            (sample << -shift) as i16
                        }
                    })
                })
                .collect::<Result<_, _>>()?
        }
    };
    Audio::new(samples, spec.sample_rate, spec.channels as usize)
}

/// WAV decoding with an FFmpeg fallback for compressed and other audio formats.
#[derive(Debug, Clone)]
pub struct Decoder {
    pub ffmpeg: PathBuf,
}
impl Default for Decoder {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
        }
    }
}

impl Decoder {
    pub async fn decode_file(
        &self,
        path: impl AsRef<Path>,
        cancel: &CancellationToken,
    ) -> Result<Audio, Error> {
        let bytes = tokio::select! { biased; _ = cancel.cancelled() => return Err(Error::Cancelled), bytes = tokio::fs::read(path) => bytes? };
        self.decode_bytes(&bytes, cancel).await
    }
    pub async fn decode_reader(
        &self,
        mut reader: impl AsyncRead + Unpin,
        cancel: &CancellationToken,
    ) -> Result<Audio, Error> {
        let mut bytes = Vec::new();
        tokio::select! { biased; _ = cancel.cancelled() => return Err(Error::Cancelled), result = reader.read_to_end(&mut bytes) => { result?; } }
        self.decode_bytes(&bytes, cancel).await
    }
    pub async fn decode_bytes(
        &self,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<Audio, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if bytes.len() >= 12
            && &bytes[..4] == b"RIFF"
            && &bytes[8..12] == b"WAVE"
            && let Ok(audio) = decode_wav(bytes)
        {
            return Ok(audio.normalize());
        }
        let mut command = tokio::process::Command::new(&self.ffmpeg);
        command
            .args([
                "-v",
                "error",
                "-i",
                "pipe:0",
                "-f",
                "s16le",
                "-acodec",
                "pcm_s16le",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let operation = async {
            let write = async {
                let result = stdin.write_all(bytes).await;
                drop(stdin);
                result
            };
            let (written, output) = tokio::join!(write, child.wait_with_output());
            let output = output?;
            if !output.status.success() {
                return Err(Error::Decoder(
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                ));
            }
            written?;
            Audio::from_pcm16le(&output.stdout, 16000, 1)
        };
        tokio::select! { biased; _ = cancel.cancelled() => Err(Error::Cancelled), result = operation => result }
    }
}
