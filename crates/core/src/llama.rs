use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use process_wrap::std::*;
use serde_json::Value;

use crate::refine::{Refined, parse_response, request};

/// Loading a 2 GB model from a slow disk can take this long.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// A local `llama-server` that lives as long as this value.
pub struct LlamaServer {
    child: Box<dyn ChildWrapper>,
    url: String,
    agent: ureq::Agent,
}

impl LlamaServer {
    pub fn start(exe: &Path, model: &Path) -> Result<Self, String> {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| format!("No free port for llama-server: {e}"))?
            .port();
        let mut command = std::process::Command::new(exe);
        command
            .arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["-ngl", "99", "-c", "4096", "-np", "1", "--no-webui"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut wrap = CommandWrap::from(command);
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            wrap.wrap(CreationFlags(CREATE_NO_WINDOW)).wrap(JobObject);
        }
        let child = wrap
            .spawn()
            .map_err(|e| format!("Cannot start llama-server: {e}"))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        let mut server = Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            agent,
        };
        server.wait_ready()?;
        Ok(server)
    }

    fn wait_ready(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!("llama-server exited: {status}"));
            }
            if self
                .agent
                .get(format!("{}/health", self.url))
                .call()
                .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("llama-server did not start in time".into())
    }

    /// `Ok(None)` means the server answered with something that is not a refined prompt.
    pub fn refine(&self, text: &str) -> Result<Option<Refined>, String> {
        let body: Value = self
            .agent
            .post(format!("{}/v1/chat/completions", self.url))
            .send_json(request(text))
            .map_err(|e| format!("llama-server request failed: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("llama-server reply is not JSON: {e}"))?;
        Ok(parse_response(&body))
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_server_is_an_error() {
        let err = LlamaServer::start(Path::new("C:/nope/llama-server.exe"), Path::new("m.gguf"))
            .err()
            .unwrap();
        assert!(err.contains("llama-server"), "{err}");
    }

    /// `ERINDI_LLAMA=<llama-server.exe>;<model.gguf> cargo test -p erindi-core llama -- --ignored`
    #[test]
    #[ignore = "needs llama-server and the cleanup model"]
    fn refines_with_a_live_server() {
        let var = std::env::var("ERINDI_LLAMA").unwrap();
        let (exe, model) = var.split_once(';').unwrap();
        let server = LlamaServer::start(Path::new(exe), Path::new(model)).unwrap();
        let r = server
            .refine("эээ ну проверь diff в модуле auth")
            .unwrap()
            .unwrap();
        assert!(r.text.contains("diff"), "{r:?}");
    }
}
