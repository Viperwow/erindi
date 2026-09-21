use std::path::Path;
use std::time::Duration;

use sherpa_onnx::{VadModelConfig, VoiceActivityDetector};

use crate::dsp::TARGET_RATE;

/// Recording stops if nobody speaks for this long after the hotkey.
pub const NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Continue,
    /// Speech was heard and then silence lasted for the configured time.
    SpeechEnded,
    NoSpeech,
}

/// Decides when a toggle-mode recording is over, using Silero VAD on 16 kHz audio.
pub struct Endpointer {
    vad: VoiceActivityDetector,
    heard: bool,
    samples: usize,
}

impl Endpointer {
    pub fn new(models_dir: &Path, silence: Duration) -> Result<Self, String> {
        let model = models_dir.join("silero_vad.onnx");
        if !model.is_file() {
            return Err(format!("model file missing: {}", model.display()));
        }
        let mut config = VadModelConfig::default();
        config.silero_vad.model = Some(model.to_string_lossy().into_owned());
        config.silero_vad.threshold = 0.5;
        config.silero_vad.min_speech_duration = 0.25;
        config.silero_vad.min_silence_duration = silence.as_secs_f32();
        config.silero_vad.window_size = 512;
        config.silero_vad.max_speech_duration = 600.0;
        config.sample_rate = TARGET_RATE as i32;
        config.num_threads = 1;
        let vad =
            VoiceActivityDetector::create(&config, 30.0).ok_or("failed to load Silero VAD")?;
        Ok(Self {
            vad,
            heard: false,
            samples: 0,
        })
    }

    pub fn heard_speech(&self) -> bool {
        self.heard
    }

    pub fn push(&mut self, samples: &[f32]) -> Endpoint {
        self.vad.accept_waveform(samples);
        self.samples += samples.len();
        // Only the in-speech flag matters; finished segments are dropped to bound memory.
        while !self.vad.is_empty() {
            self.vad.pop();
        }
        let in_speech = self.vad.detected();
        self.heard |= in_speech;
        if self.heard && !in_speech {
            Endpoint::SpeechEnded
        } else if !self.heard
            && self.samples as f32 >= NO_SPEECH_TIMEOUT.as_secs_f32() * TARGET_RATE as f32
        {
            Endpoint::NoSpeech
        } else {
            Endpoint::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::{PARAKEET_DIR, tests::models_dir};
    use crate::dsp::To16k;

    fn speech() -> Vec<f32> {
        let wav = models_dir().join(PARAKEET_DIR).join("test_wavs/en.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).unwrap();
        To16k::new(wave.sample_rate() as u32).push(wave.samples())
    }

    fn feed(e: &mut Endpointer, audio: &[f32]) -> Vec<(usize, Endpoint)> {
        audio
            .chunks(480)
            .enumerate()
            .map(|(i, c)| (i * 480, e.push(c)))
            .filter(|(_, p)| *p != Endpoint::Continue)
            .collect()
    }

    #[test]
    fn missing_model_is_an_error() {
        assert!(Endpointer::new(Path::new("no-such-dir"), Duration::from_secs(1)).is_err());
    }

    #[test]
    #[ignore = "needs models/"]
    fn speech_then_silence_ends_after_silence_limit() {
        let mut e = Endpointer::new(&models_dir(), Duration::from_secs(1)).unwrap();
        let mut audio = speech();
        let speech_len = audio.len();
        audio.extend(vec![0.0; 3 * TARGET_RATE as usize]);

        let events = feed(&mut e, &audio);

        assert!(e.heard_speech());
        let (at, kind) = events[0];
        assert_eq!(kind, Endpoint::SpeechEnded);
        let silence = (at - speech_len) as f32 / TARGET_RATE as f32;
        assert!(
            (0.8..2.0).contains(&silence),
            "ended {silence}s into silence"
        );
    }

    #[test]
    #[ignore = "needs models/"]
    fn speech_without_pause_does_not_end() {
        let mut e = Endpointer::new(&models_dir(), Duration::from_secs(2)).unwrap();
        assert_eq!(feed(&mut e, &speech()), []);
    }

    #[test]
    #[ignore = "needs models/"]
    fn silence_only_times_out() {
        let mut e = Endpointer::new(&models_dir(), Duration::from_secs(1)).unwrap();
        let limit = NO_SPEECH_TIMEOUT.as_secs() as usize * TARGET_RATE as usize;
        let events = feed(&mut e, &vec![0.0; limit + TARGET_RATE as usize]);
        assert!(!e.heard_speech());
        let (at, kind) = events[0];
        assert_eq!(kind, Endpoint::NoSpeech);
        assert!(at >= limit - 480, "{at}");
    }
}
