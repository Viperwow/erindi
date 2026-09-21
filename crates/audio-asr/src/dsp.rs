use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

pub const TARGET_RATE: u32 = 16_000;

pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Streaming mono resampler to [`TARGET_RATE`].
pub struct To16k {
    inner: Option<Async<f32>>,
    pending: Vec<f32>,
}

impl To16k {
    pub fn new(input_rate: u32) -> Self {
        let inner = (input_rate != TARGET_RATE).then(|| {
            let params = SincInterpolationParameters::new(64, WindowFunction::BlackmanHarris2)
                .oversampling_factor(128)
                .interpolation(SincInterpolationType::Linear);
            let ratio = TARGET_RATE as f64 / input_rate as f64;
            Async::new_sinc(ratio, 1.0, &params, 480, 1, FixedAsync::Input)
                .expect("valid resampler parameters")
        });
        Self {
            inner,
            pending: Vec::new(),
        }
    }

    /// Feeds mono samples and returns whatever 16 kHz output is ready.
    pub fn push(&mut self, mono: &[f32]) -> Vec<f32> {
        let Some(inner) = &mut self.inner else {
            return mono.to_vec();
        };
        self.pending.extend_from_slice(mono);
        let mut out = Vec::new();
        let mut used = 0;
        loop {
            let need = inner.input_frames_next();
            let rest = &self.pending[used..];
            if rest.len() < need {
                break;
            }
            let input = InterleavedSlice::new(&rest[..need], 1, need).expect("mono slice");
            let chunk = inner.process(&input, None).expect("sized input");
            out.extend(chunk.take_data());
            used += need;
        }
        self.pending.drain(..used);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(rate: u32, freq: f32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| 0.5 * (2.0 * PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn zero_crossings(s: &[f32]) -> usize {
        s.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(
            downmix(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2),
            [0.5, 0.5, 0.0]
        );
        assert_eq!(downmix(&[0.1, 0.2], 1), [0.1, 0.2]);
    }

    #[test]
    fn rms_of_sine_and_silence() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 320]), 0.0);
        let r = rms(&sine(16_000, 440.0, 1.0));
        assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.01, "{r}");
    }

    #[test]
    fn resamples_common_rates_in_chunks() {
        for rate in [48_000, 44_100, 16_000] {
            let input = sine(rate, 440.0, 2.0);
            let mut r = To16k::new(rate);
            let out: Vec<f32> = input.chunks(441).flat_map(|c| r.push(c)).collect();

            let expected = 2 * TARGET_RATE as usize;
            assert!(
                out.len() + 2048 >= expected && out.len() <= expected,
                "{rate}: {} samples",
                out.len()
            );
            // 440 Hz keeps ~880 zero crossings per second after resampling.
            let steady = &out[4000..20000];
            let per_sec = zero_crossings(steady) as f32 / 1.0;
            assert!((per_sec - 880.0).abs() < 10.0, "{rate}: {per_sec}");
        }
    }
}
