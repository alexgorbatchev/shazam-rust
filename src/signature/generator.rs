use super::{Peak, Signature};
use crate::Error;
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::{collections::VecDeque, sync::Arc};

/// Incremental signature generator for mono signed 16-bit PCM at 16000 Hz.
pub struct Generator {
    pending: VecDeque<i16>,
    samples: [i16; 2048],
    position: usize,
    spectra: Vec<[f64; 1025]>,
    spread: Vec<[f64; 1025]>,
    passes: usize,
    fft: Arc<dyn Fft<f64>>,
    window: Vec<f64>,
    buffer: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    max_seconds: f64,
    max_peaks: usize,
}

impl Default for Generator {
    fn default() -> Self {
        Self::with_limits(12.0, 255).expect("valid default limits")
    }
}

impl Generator {
    pub fn with_limits(max_seconds: f64, max_peaks: usize) -> Result<Self, Error> {
        if !max_seconds.is_finite()
            || max_seconds <= 0.0
            || max_seconds > u32::MAX as f64 / 16000.0
            || max_peaks == 0
        {
            return Err(Error::InvalidLimits);
        }
        let fft = FftPlanner::new().plan_fft_forward(2048);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Ok(Self {
            pending: VecDeque::new(),
            samples: [0; 2048],
            position: 0,
            spectra: vec![[0.0; 1025]; 256],
            spread: vec![[0.0; 1025]; 256],
            passes: 0,
            fft,
            window: (0..2048)
                .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * (i + 1) as f64 / 2049.0).cos()))
                .collect(),
            buffer: vec![Complex::default(); 2048],
            scratch,
            max_seconds,
            max_peaks,
        })
    }
    pub fn feed(&mut self, samples: &[i16]) {
        self.pending.extend(samples);
    }
    pub fn reset(&mut self) {
        self.pending.clear();
        self.reset_pipeline();
    }
    fn reset_pipeline(&mut self) {
        self.samples.fill(0);
        self.position = 0;
        self.passes = 0;
        self.spectra.fill([0.0; 1025]);
        self.spread.fill([0.0; 1025]);
    }
    fn relative(&self, offset: isize) -> usize {
        (self.passes as isize + offset).rem_euclid(256) as usize
    }

    pub fn next_signature(&mut self) -> Option<Signature> {
        if self.pending.len() < 128 {
            return None;
        }
        let mut signature = Signature::new(16000).expect("supported sample rate");
        while self.pending.len() >= 128
            && signature.duration_seconds() < self.max_seconds
            && signature.total_peaks() < self.max_peaks
        {
            for _ in 0..128 {
                self.samples[self.position] = self.pending.pop_front().unwrap();
                self.position = (self.position + 1) % 2048;
            }
            signature.number_samples += 128;
            self.process(&mut signature);
        }
        self.reset_pipeline();
        Some(signature)
    }

    fn process(&mut self, signature: &mut Signature) {
        for i in 0..2048 {
            self.buffer[i] = Complex::new(
                self.samples[(self.position + i) % 2048] as f64 * self.window[i],
                0.0,
            );
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let index = self.passes % 256;
        for i in 0..1025 {
            self.spectra[index][i] = (self.buffer[i].norm_sqr() / 131072.0).max(1e-10);
        }
        self.passes += 1;
        let mut current = self.spectra[index];
        for (i, value) in current.iter_mut().enumerate().take(1022) {
            *value = self.spectra[index][i]
                .max(self.spectra[index][i + 1])
                .max(self.spectra[index][i + 2]);
        }
        let r1 = self.relative(-1);
        let r2 = self.relative(-3);
        let r3 = self.relative(-6);
        // Spreading uses the ring position before appending its current row,
        // whereas the FFT ring has already advanced by one row.
        let r1 = (r1 + 255) % 256;
        let r2 = (r2 + 255) % 256;
        let r3 = (r3 + 255) % 256;
        for (i, value) in current.iter().enumerate() {
            self.spread[r1][i] = self.spread[r1][i].max(*value);
            self.spread[r2][i] = self.spread[r2][i].max(self.spread[r1][i]);
            self.spread[r3][i] = self.spread[r3][i].max(self.spread[r2][i]);
        }
        self.spread[index] = current;
        if self.passes < 46 {
            return;
        }
        let fft = &self.spectra[self.relative(-46)];
        let spread = &self.spread[self.relative(-49)];
        for i in 10..1015 {
            let value = fft[i];
            if value < 1.0 / 64.0 || value < spread[i - 1] {
                continue;
            }
            let neighbor = [-10, -7, -4, -3, 1, 2, 5, 8]
                .into_iter()
                .map(|offset| spread[(i as isize + offset) as usize])
                .fold(0.0, f64::max);
            if value <= neighbor {
                continue;
            }
            let neighbor = [
                -53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
            ]
            .into_iter()
            .map(|offset| self.spread[self.relative(offset)][i - 1])
            .fold(neighbor, f64::max);
            if value <= neighbor {
                continue;
            }
            let magnitude = value.max(1.0 / 64.0).ln() * 1477.3 + 6144.0;
            let before = fft[i - 1].max(1.0 / 64.0).ln() * 1477.3 + 6144.0;
            let after = fft[i + 1].max(1.0 / 64.0).ln() * 1477.3 + 6144.0;
            let denominator = magnitude * 2.0 - before - after;
            let correction = if denominator > 0.0 {
                (after - before) * 32.0 / denominator
            } else {
                0.0
            };
            let bin = i as f64 * 64.0 + correction;
            let hz = bin * 16000.0 / 131072.0;
            let band = if hz > 250.0 && hz < 520.0 {
                0
            } else if (520.0..1450.0).contains(&hz) {
                1
            } else if (1450.0..3500.0).contains(&hz) {
                2
            } else if (3500.0..=5500.0).contains(&hz) {
                3
            } else {
                continue;
            };
            signature.peaks.entry(band).or_default().push(Peak {
                pass: (self.passes - 46) as u32,
                magnitude: magnitude as u16,
                frequency_bin: bin as u16,
            });
        }
    }
}
