use shazam_client::{
    Client, Config, Endpoints, Error,
    audio::{Audio, Decoder, decode_wav},
};
use std::{io::Cursor, time::Duration};
use tokio_util::sync::CancellationToken;

fn wav(bits: u16, float: bool) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8000,
        bits_per_sample: bits,
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let mut writer = hound::WavWriter::new(&mut buffer, spec).unwrap();
    if float {
        for x in [0.5f32, 0.5, -0.5, -0.5] {
            writer.write_sample(x).unwrap();
        }
    } else {
        for x in [
            1i32 << (bits - 2),
            1 << (bits - 2),
            -(1 << (bits - 2)),
            -(1 << (bits - 2)),
        ] {
            writer.write_sample(x).unwrap();
        }
    }
    writer.finalize().unwrap();
    buffer.into_inner()
}
#[tokio::test]
async fn wav_formats_native_decode_and_recognition_empty_input() {
    let c = CancellationToken::new();
    let decoder = Decoder::default();
    for bits in [8, 16, 24, 32] {
        let bytes = wav(bits, false);
        let audio = decode_wav(&bytes).unwrap();
        assert_eq!(audio.samples(), [16384, 16384, -16384, -16384]);
        assert_eq!(audio.sample_rate(), 8000);
        assert_eq!(audio.channels(), 2);
        assert_eq!(
            decoder.decode_bytes(&bytes, &c).await.unwrap().samples(),
            [16384, 0, -16384, -16384]
        );
    }
    let bytes = wav(32, true);
    assert_eq!(
        decode_wav(&bytes).unwrap().samples(),
        [16383, 16383, -16383, -16383]
    );
    let path = tempfile::NamedTempFile::new_in(".tmp").unwrap();
    std::fs::write(path.path(), &bytes).unwrap();
    let audio = decoder.decode_file(path.path(), &c).await.unwrap();
    assert_eq!(
        decoder
            .decode_reader(Cursor::new(&bytes), &c)
            .await
            .unwrap(),
        audio
    );
    assert!(decode_wav(b"invalid").is_err());
    assert!(Audio::new(vec![1], 16000, 2).is_err());
    assert!(audio.resample(0).is_err());
    assert_eq!(audio.resample(16000).unwrap(), audio);
    assert!(
        Audio::new(vec![], 8000, 1)
            .unwrap()
            .resample(16000)
            .unwrap()
            .samples()
            .is_empty()
    );
    let client = Client::new(Config::default()).unwrap();
    assert!(
        client
            .recognize_bytes(&bytes, &c)
            .await
            .unwrap()
            .matches
            .is_empty()
    );
    assert!(
        client
            .recognize_file(path.path(), &c)
            .await
            .unwrap()
            .matches
            .is_empty()
    );
    assert!(
        client
            .recognize_reader(Cursor::new(&bytes), &c)
            .await
            .unwrap()
            .matches
            .is_empty()
    );
    c.cancel();
    assert!(matches!(
        decoder.decode_file(path.path(), &c).await,
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        decoder.decode_reader(Cursor::new(bytes.clone()), &c).await,
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        decoder.decode_bytes(&bytes, &c).await,
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        client.recognize_audio(&audio, &c).await,
        Err(Error::Cancelled)
    ));
}

#[tokio::test]
async fn ffmpeg_fallback_failure_and_cancellation() {
    let c = CancellationToken::new();
    let decoder = Decoder::default();
    let mut command = tokio::process::Command::new("ffmpeg");
    let output = command
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.15",
            "-f",
            "flac",
            "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let audio = decoder.decode_bytes(&output.stdout, &c).await.unwrap();
    assert_eq!(audio.sample_rate(), 16000);
    assert_eq!(audio.channels(), 1);
    assert!(audio.samples().len() > 2000);
    assert!(matches!(
        decoder.decode_bytes(b"invalid", &c).await,
        Err(Error::Decoder(_))
    ));
    let missing = Decoder {
        ffmpeg: ".tmp/missing-decoder".into(),
    };
    assert!(matches!(
        missing.decode_bytes(b"invalid", &c).await,
        Err(Error::Io(_))
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = tempfile::NamedTempFile::new_in(".tmp").unwrap();
        std::fs::write(executable.path(), "#!/bin/sh\nexec sleep 20\n").unwrap();
        std::fs::set_permissions(executable.path(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let path = executable.into_temp_path();
        let decoder = Decoder {
            ffmpeg: path.to_path_buf(),
        };
        let cancel = c.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel.cancel()
        });
        assert!(matches!(
            decoder.decode_bytes(b"audio", &c).await,
            Err(Error::Cancelled)
        ));
    }
    let endpoints = Endpoints {
        web: reqwest::Url::parse("mailto:invalid").unwrap(),
        ..Default::default()
    };
    let client = Client::new(Config {
        endpoints,
        ..Default::default()
    })
    .unwrap();
    assert!(
        client
            .listening_counter(42, &CancellationToken::new())
            .await
            .is_err()
    );
}
