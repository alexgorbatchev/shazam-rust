use shazam_client::{
    audio::Audio,
    signature::{Generator, Peak, Signature},
};

fn samples() -> Vec<i16> {
    let mut state = 0x12345678u32;
    (0..16000 * 15)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as i16
        })
        .collect()
}

#[test]
fn signatures_round_trip_and_corruption() {
    for rate in [8000, 11025, 16000, 32000, 44100, 48000] {
        let mut signature = Signature::new(rate).unwrap();
        signature.number_samples = rate * 12;
        signature.peaks.insert(
            0,
            vec![
                Peak {
                    pass: 10,
                    magnitude: 7000,
                    frequency_bin: 300,
                },
                Peak {
                    pass: 280,
                    magnitude: 8500,
                    frequency_bin: 450,
                },
            ],
        );
        signature.peaks.insert(
            1,
            vec![Peak {
                pass: 5,
                magnitude: 6500,
                frequency_bin: 600,
            }],
        );
        let binary = signature.encode().unwrap();
        assert_eq!(Signature::decode(&binary).unwrap(), signature);
        assert_eq!(
            Signature::from_uri(&signature.to_uri().unwrap()).unwrap(),
            signature
        );
        assert_eq!(signature.duration_seconds(), 12.0);
        assert_eq!(signature.total_peaks(), 3);
        let peak = &signature.peaks[&0][0];
        assert!(peak.frequency_hz(rate) > 0.0);
        assert!(peak.amplitude_pcm() > 0.0);
        assert!(peak.seconds(rate) > 0.0);
        let mut corrupt = binary.clone();
        corrupt[12] ^= 1;
        assert!(Signature::decode(&corrupt).is_err());
        for end in 0..binary.len() {
            assert!(Signature::decode(&binary[..end]).is_err());
        }
    }
    assert!(Signature::new(9999).is_err());
    assert!(Signature::from_uri("invalid").is_err());
    let mut invalid = Signature::new(16000).unwrap();
    invalid.peaks.insert(4, vec![]);
    assert!(invalid.encode().is_err());
    invalid.peaks.clear();
    invalid.peaks.insert(
        0,
        vec![
            Peak {
                pass: 9,
                magnitude: 1,
                frequency_bin: 1,
            },
            Peak {
                pass: 8,
                magnitude: 1,
                frequency_bin: 1,
            },
        ],
    );
    assert!(
        invalid.encode().is_err(),
        "out-of-order peaks must be rejected, not silently dropped"
    );
}

#[test]
fn generator_snippets_and_audio_normalization() {
    let input = samples();
    let mut generator = Generator::default();
    generator.feed(&input[..60]);
    assert!(generator.next_signature().is_none());
    generator.feed(&input[60..]);
    let signature = generator.next_signature().unwrap();
    assert_eq!(
        signature.to_uri().unwrap(),
        include_str!("fixtures/generator-uri.txt").trim()
    );
    assert!(signature.total_peaks() > 0);
    assert!(signature.number_samples <= 16000 * 12);
    assert_eq!(
        Signature::decode(&signature.encode().unwrap()).unwrap(),
        signature
    );
    let mut consumed = signature.number_samples;
    while let Some(next) = generator.next_signature() {
        consumed += next.number_samples;
    }
    assert_eq!(consumed as usize, input.len() / 128 * 128);
    generator.reset();
    generator.feed(&[0; 128]);
    assert_eq!(generator.next_signature().unwrap().number_samples, 128);
    let audio = Audio::new(vec![10000, 10000, -10000, -10000], 8000, 2).unwrap();
    assert_eq!(audio.to_mono().samples(), [10000, -10000]);
    assert_eq!(audio.normalize().samples(), [10000, 0, -10000, -10000]);
    assert_eq!(audio.duration_seconds(), 2.0 / 8000.0);
    assert!(Audio::new(vec![], 0, 1).is_err());
    assert!(Audio::new(vec![], 16000, 0).is_err());
    assert!(Audio::from_pcm16le(&[1], 16000, 1).is_err());
    assert_eq!(
        Audio::from_pcm16le(&[0xff, 0x7f], 16000, 1)
            .unwrap()
            .samples(),
        [32767]
    );
    let long = Audio::new(vec![0; 16000 * 40], 16000, 1).unwrap();
    assert_eq!(
        long.signature_generator()
            .next_signature()
            .unwrap()
            .number_samples,
        16000 * 12
    );
}
