use std::sync::mpsc::{self, SyncSender};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::dsp::downmix;

pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return vec![];
    };
    devices
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect()
}

fn find_device(name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .ok_or("no default microphone".into()),
        Some(name) => host
            .input_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.description().is_ok_and(|desc| desc.name() == name))
            .ok_or(format!("microphone not found: {name}")),
    }
}

fn open(device: Option<&str>, tx: SyncSender<Vec<f32>>) -> Result<(cpal::Stream, u32), String> {
    let device = find_device(device)?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate();
    let on_error = |e| eprintln!("microphone stream error: {e}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            supported.config(),
            move |data: &[f32], _: &_| {
                let _ = tx.try_send(downmix(data, channels));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            supported.config(),
            move |data: &[i16], _: &_| {
                let data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                let _ = tx.try_send(downmix(&data, channels));
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

/// A running microphone stream. Mono chunks at the device rate go to the channel given to [`start`].
pub struct Capture {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// Opens `device` (or the default input) and returns the capture with its sample rate.
/// Chunks that do not fit into `tx` are dropped so the audio callback never blocks.
pub fn start(device: Option<&str>, tx: SyncSender<Vec<f32>>) -> Result<(Capture, u32), String> {
    let device = device.map(str::to_string);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    // cpal streams are not Send on every platform, so the stream lives and dies on its own thread.
    let thread = std::thread::spawn(move || match open(device.as_deref(), tx) {
        Ok((stream, rate)) => {
            let _ = ready_tx.send(Ok(rate));
            let _ = stop_rx.recv();
            drop(stream);
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
    });
    let rate = ready_rx
        .recv()
        .map_err(|_| "capture thread died".to_string())??;
    Ok((
        Capture {
            stop: Some(stop_tx),
            thread: Some(thread),
        },
        rate,
    ))
}

impl Drop for Capture {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn listing_devices_does_not_fail() {
        let _ = input_devices();
    }

    #[test]
    fn unknown_device_is_an_error() {
        let (tx, _rx) = mpsc::sync_channel(4);
        assert!(start(Some("no such microphone"), tx).is_err());
    }

    #[test]
    #[ignore = "needs a microphone"]
    fn records_from_default_microphone() {
        let (tx, rx) = mpsc::sync_channel(64);
        let (capture, rate) = start(None, tx).unwrap();
        let started = Instant::now();
        let mut samples = 0;
        while started.elapsed() < Duration::from_millis(500) {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                samples += chunk.len();
            }
        }
        drop(capture);
        assert!(samples as u32 > rate / 4, "{samples} samples at {rate} Hz");
    }
}
