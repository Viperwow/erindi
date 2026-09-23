use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub struct ModelFile {
    /// Relative to the models folder.
    pub path: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub files: &'static [ModelFile],
}

/// Smallest files first, so a broken network fails fast.
pub const SPEECH: Model = Model {
    id: "speech",
    label: "Parakeet TDT 0.6B v3 (~670 MB)",
    files: &[
        ModelFile {
            path: "silero_vad.onnx",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
            size: 643_854,
            sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/tokens.txt",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/tokens.txt",
            size: 93_939,
            sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/joiner.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/joiner.int8.onnx",
            size: 6_355_277,
            sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/decoder.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/decoder.int8.onnx",
            size: 11_845_275,
            sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/encoder.int8.onnx",
            size: 652_184_281,
            sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
        },
    ],
};

pub const CLEANUP_GGUF: &str = "qwen2.5-3b-instruct-q4_k_m.gguf";

pub const CLEANUP: Model = Model {
    id: "cleanup",
    label: "Qwen2.5-3B-Instruct Q4 (~2.1 GB)",
    files: &[ModelFile {
        path: CLEANUP_GGUF,
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/7dabda4d13d513e3e842b20f0d435c732f172cbe/qwen2.5-3b-instruct-q4_k_m.gguf",
        size: 2_104_932_768,
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
    }],
};

pub fn by_id(id: &str) -> Option<&'static Model> {
    [&SPEECH, &CLEANUP].into_iter().find(|m| m.id == id)
}

impl Model {
    /// Size is checked instead of the hash so the check is instant; downloads verify the hash
    /// before a file gets its final name.
    pub fn installed(&self, dir: &Path) -> bool {
        self.files
            .iter()
            .all(|f| std::fs::metadata(dir.join(f.path)).is_ok_and(|m| m.len() == f.size))
    }

    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Downloads the missing files of `model` into `dir`, reporting `(done, total)` bytes.
pub fn download(
    model: &Model,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let total = model.size();
    let mut done = 0;
    for f in model.files {
        let path = dir.join(f.path);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == f.size) {
            done += f.size;
            progress(done, total);
            continue;
        }
        let mut response = ureq::get(f.url)
            .call()
            .map_err(|e| format!("Cannot download {}: {e}", f.path))?;
        let reader = response.body_mut().as_reader();
        save(reader, &path, f.sha256, |n| {
            done += n;
            progress(done, total);
        })?;
    }
    Ok(())
}

/// Writes `reader` to `<path>.partial` and renames it to `path` once the hash matches.
fn save(
    mut reader: impl Read,
    path: &Path,
    sha256: &str,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let partial = std::path::PathBuf::from(format!("{}.partial", path.display()));
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress(n as u64);
    }
    drop(file);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{}: SHA-256 mismatch", path.display()));
    }
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const HELLO_SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn saves_a_file_whose_hash_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/hello.bin");
        let mut seen = 0;
        save(Cursor::new(b"hello"), &path, HELLO_SHA, |n| seen += n).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(seen, 5);
        assert!(!dir.path().join("a/hello.bin.partial").exists());
    }

    #[test]
    fn hash_mismatch_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        let err = save(Cursor::new(b"hellO"), &path, HELLO_SHA, |_| {}).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!path.exists());
        assert!(!dir.path().join("hello.bin.partial").exists());
    }

    #[test]
    fn partial_file_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let model = &SPEECH;
        for f in model.files {
            let path = dir.path().join(f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path)
                .unwrap()
                .set_len(f.size)
                .unwrap();
        }
        assert!(model.installed(dir.path()));
        let last = model.files.last().unwrap();
        std::fs::File::create(dir.path().join(last.path))
            .unwrap()
            .set_len(last.size - 1)
            .unwrap();
        assert!(!model.installed(dir.path()));
    }

    #[test]
    fn catalog_lookup() {
        assert_eq!(by_id("speech").unwrap().id, "speech");
        assert_eq!(by_id("cleanup").unwrap().files[0].path, CLEANUP_GGUF);
        assert!(by_id("other").is_none());
        assert!(SPEECH.size() > 600_000_000);
    }

    #[test]
    fn replaces_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        std::fs::write(&path, b"broken").unwrap();
        save(Cursor::new(b"hello"), &path, HELLO_SHA, |_| {}).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }
}
