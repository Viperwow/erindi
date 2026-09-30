//! A plain-text debug log of hotkeys, capture timing and controller traffic, for diagnosing
//! gestures and dropped phrases. It holds transcripts, so it goes only where Settings point it.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use erindi_core::controller::{Effect, Msg};

static PATH: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Writes to `path` from now on; an empty path turns the log off.
pub fn set_path(path: &str) {
    *PATH.write().unwrap() = (!path.trim().is_empty()).then(|| PathBuf::from(path));
}

pub fn line(text: String) {
    static START: OnceLock<Instant> = OnceLock::new();
    let ms = START.get_or_init(Instant::now).elapsed().as_millis();
    let Some(path) = PATH.read().unwrap().clone() else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{ms:>8} {text}");
    }
}

pub fn msg(msg: &Msg) {
    match msg {
        Msg::Audio { .. } | Msg::Live { .. } | Msg::Settings { .. } => {}
        Msg::Transcribed { op, text } => line(format!("msg Transcribed {op} {text:?}")),
        other => line(format!("msg {other:?}")),
    }
}

pub fn effect(effect: &Effect) {
    match effect {
        Effect::LiveDecode { .. } | Effect::Show(_) => {}
        Effect::Transcribe { op, samples } => line(format!(
            "fx Transcribe {op} {} ms of audio",
            samples.len() / 16
        )),
        other => line(format!("fx {other:?}")),
    }
}
