use crate::Error;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

mod generator;
pub use generator::Generator;

pub const URI_PREFIX: &str = "data:audio/vnd.shazam.sig;base64,";
const RATES: [u32; 6] = [8000, 11025, 16000, 32000, 44100, 48000];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peak {
    pub pass: u32,
    pub magnitude: u16,
    pub frequency_bin: u16,
}

impl Peak {
    pub fn frequency_hz(&self, sample_rate: u32) -> f64 {
        self.frequency_bin as f64 * sample_rate as f64 / 131072.0
    }
    pub fn amplitude_pcm(&self) -> f64 {
        (((self.magnitude as f64 - 6144.0) / 1477.3).exp() * 65536.0).sqrt() / 1024.0
    }
    pub fn seconds(&self, sample_rate: u32) -> f64 {
        self.pass as f64 * 128.0 / sample_rate as f64
    }
}

/// Decoded signature with frequency bands zero through three in ascending order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub sample_rate: u32,
    pub number_samples: u32,
    pub peaks: BTreeMap<u8, Vec<Peak>>,
}

impl Signature {
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        if !RATES.contains(&sample_rate) {
            return Err(Error::InvalidAudio);
        }
        Ok(Self {
            sample_rate,
            number_samples: 0,
            peaks: BTreeMap::new(),
        })
    }
    pub fn duration_seconds(&self) -> f64 {
        self.number_samples as f64 / self.sample_rate as f64
    }
    pub fn total_peaks(&self) -> usize {
        self.peaks.values().map(Vec::len).sum()
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let rate_id = RATES
            .iter()
            .position(|rate| *rate == self.sample_rate)
            .ok_or(Error::InvalidAudio)? as u32
            + 1;
        let mut body = Vec::new();
        for (band, peaks) in &self.peaks {
            if *band > 3 {
                return Err(Error::InvalidSignature);
            }
            let mut encoded = Vec::new();
            let mut previous = 0;
            for peak in peaks {
                let mut delta = peak
                    .pass
                    .checked_sub(previous)
                    .ok_or(Error::InvalidSignature)?;
                if delta >= 255 {
                    encoded.push(255);
                    encoded.extend_from_slice(&peak.pass.to_le_bytes());
                    delta = 0;
                }
                encoded.push(delta as u8);
                encoded.extend_from_slice(&peak.magnitude.to_le_bytes());
                encoded.extend_from_slice(&peak.frequency_bin.to_le_bytes());
                previous = peak.pass;
            }
            body.extend_from_slice(&(0x60030040 + *band as u32).to_le_bytes());
            body.extend_from_slice(
                &u32::try_from(encoded.len())
                    .map_err(|_| Error::InvalidSignature)?
                    .to_le_bytes(),
            );
            body.extend_from_slice(&encoded);
            body.resize(body.len().next_multiple_of(4), 0);
        }
        let size = u32::try_from(body.len() + 8).map_err(|_| Error::InvalidSignature)?;
        let count = self
            .number_samples
            .checked_add(self.sample_rate * 24 / 100)
            .ok_or(Error::InvalidSignature)?;
        let words = [
            0xcafe2580,
            0,
            size,
            0x94119c00,
            0,
            0,
            0,
            rate_id << 27,
            0,
            0,
            count,
            0x7c0000,
            0x40000000,
            size,
        ];
        let mut binary: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        binary.extend_from_slice(&body);
        let crc = crc32fast::hash(&binary[8..]);
        binary[4..8].copy_from_slice(&crc.to_le_bytes());
        Ok(binary)
    }

    pub fn decode(binary: &[u8]) -> Result<Self, Error> {
        if binary.len() < 56 {
            return Err(Error::InvalidSignature);
        }
        let word =
            |offset: usize| u32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap());
        if word(4) != crc32fast::hash(&binary[8..]) {
            return Err(Error::Checksum);
        }
        if word(0) != 0xcafe2580
            || word(12) != 0x94119c00
            || word(44) != 0x7c0000
            || word(48) != 0x40000000
            || word(8) as usize != binary.len() - 48
            || word(52) != word(8)
        {
            return Err(Error::InvalidSignature);
        }
        let rate_id = word(28) >> 27;
        let rate = *RATES
            .get(rate_id.wrapping_sub(1) as usize)
            .ok_or(Error::InvalidAudio)?;
        let mut signature = Self::new(rate)?;
        signature.number_samples = word(40)
            .checked_sub(rate * 24 / 100)
            .ok_or(Error::InvalidSignature)?;
        let mut position = 56;
        while position < binary.len() {
            if binary.len() - position < 8 {
                return Err(Error::InvalidSignature);
            }
            let tag = word(position)
                .checked_sub(0x60030040)
                .filter(|tag| *tag <= 3)
                .ok_or(Error::InvalidSignature)? as u8;
            let size = word(position + 4) as usize;
            position += 8;
            let end = position
                .checked_add(size)
                .filter(|end| *end <= binary.len())
                .ok_or(Error::InvalidSignature)?;
            let next = end.next_multiple_of(4);
            if next > binary.len()
                || binary[end..next].iter().any(|byte| *byte != 0)
                || signature.peaks.contains_key(&tag)
            {
                return Err(Error::InvalidSignature);
            }
            let mut peaks = Vec::new();
            let mut pass = 0u32;
            while position < end {
                let delta = binary[position];
                position += 1;
                if delta == 255 {
                    if end - position < 4 {
                        return Err(Error::InvalidSignature);
                    }
                    let absolute = word(position);
                    position += 4;
                    if absolute < pass {
                        return Err(Error::InvalidSignature);
                    }
                    pass = absolute;
                    if position == end {
                        return Err(Error::InvalidSignature);
                    }
                    continue;
                }
                pass = pass
                    .checked_add(delta as u32)
                    .ok_or(Error::InvalidSignature)?;
                if end - position < 4 {
                    return Err(Error::InvalidSignature);
                }
                peaks.push(Peak {
                    pass,
                    magnitude: u16::from_le_bytes(
                        binary[position..position + 2].try_into().unwrap(),
                    ),
                    frequency_bin: u16::from_le_bytes(
                        binary[position + 2..position + 4].try_into().unwrap(),
                    ),
                });
                position += 4;
            }
            signature.peaks.insert(tag, peaks);
            position = next;
        }
        Ok(signature)
    }
    pub fn to_uri(&self) -> Result<String, Error> {
        Ok(format!("{URI_PREFIX}{}", STANDARD.encode(self.encode()?)))
    }
    pub fn from_uri(uri: &str) -> Result<Self, Error> {
        Self::decode(&STANDARD.decode(uri.strip_prefix(URI_PREFIX).unwrap_or(uri))?)
    }
}
