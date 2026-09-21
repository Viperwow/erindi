use std::path::Path;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};

use crate::dsp::TARGET_RATE;

pub const PARAKEET_DIR: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";

/// Parakeet TDT recognizer; loading takes seconds, so keep one instance for the app lifetime.
pub struct Asr {
    recognizer: OfflineRecognizer,
}

impl Asr {
    pub fn load(models_dir: &Path) -> Result<Self, String> {
        let dir = models_dir.join(PARAKEET_DIR);
        let file = |name: &str| -> Result<Option<String>, String> {
            let path = dir.join(name);
            if !path.is_file() {
                return Err(format!("model file missing: {}", path.display()));
            }
            Ok(Some(path.to_string_lossy().into_owned()))
        };
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer.encoder = file("encoder.int8.onnx")?;
        config.model_config.transducer.decoder = file("decoder.int8.onnx")?;
        config.model_config.transducer.joiner = file("joiner.int8.onnx")?;
        config.model_config.tokens = file("tokens.txt")?;
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads =
            std::thread::available_parallelism().map_or(2, |n| n.get().min(4) as i32);
        let recognizer = OfflineRecognizer::create(&config).ok_or("failed to load Parakeet")?;
        Ok(Self { recognizer })
    }

    /// Transcribes mono 16 kHz audio.
    pub fn transcribe(&self, samples: &[f32]) -> String {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(TARGET_RATE as i32, samples);
        self.recognizer.decode(&stream);
        stream.get_result().map(|r| r.text).unwrap_or_default()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    pub fn models_dir() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models"))
    }

    pub fn asr() -> &'static Asr {
        static ASR: OnceLock<Asr> = OnceLock::new();
        ASR.get_or_init(|| Asr::load(&models_dir()).expect("run scripts/fetch-models.ps1"))
    }

    #[test]
    fn missing_model_is_an_error() {
        assert!(Asr::load(Path::new("no-such-dir")).is_err());
    }

    #[test]
    #[ignore = "needs models/"]
    fn transcribes_english_sample() {
        let wav = models_dir().join(PARAKEET_DIR).join("test_wavs/en.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).unwrap();
        let samples = crate::dsp::To16k::new(wave.sample_rate() as u32).push(wave.samples());
        let text = asr().transcribe(&samples).to_lowercase();
        assert!(text.contains("what your country can do"), "{text}");
    }

    #[test]
    #[ignore = "needs models/"]
    fn silence_gives_empty_text() {
        assert_eq!(
            asr()
                .transcribe(&vec![0.0; 2 * TARGET_RATE as usize])
                .trim(),
            ""
        );
    }
}
