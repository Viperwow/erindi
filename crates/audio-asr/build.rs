// sherpa-onnx-sys copies its DLLs next to binaries and examples but not into `deps`, where test
// binaries live. There Windows would resolve the older System32 onnxruntime.dll and crash, so the
// DLLs are copied into `deps` too.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile_dir = out_dir.ancestors().nth(3).unwrap();
    for entry in std::fs::read_dir(profile_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "dll") {
            let _ = std::fs::copy(&path, profile_dir.join("deps").join(entry.file_name()));
        }
    }
}
