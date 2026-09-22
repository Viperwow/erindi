use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::classify::{parse_response, request};
use crate::commands::Command;

/// Loading a 2 GB model from a slow disk can take this long.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// A local `llama-server` that lives as long as this value.
pub struct LlamaServer {
    child: Child,
    #[cfg(windows)]
    _job: crate::job::KillOnClose,
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
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            command.creation_flags(CREATE_NO_WINDOW.0);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start llama-server: {e}"))?;
        // process-wrap's std job does not kill on close, so a quit without destructors would leave
        // the server running; this job dies with Erindi however it exits.
        #[cfg(windows)]
        let job = crate::job::KillOnClose::new()
            .and_then(|job| job.assign(&child).map(|()| job))
            .map_err(|e| {
                let _ = child.kill();
                format!("Cannot tie llama-server to Erindi: {e}")
            })?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        let mut server = Self {
            child,
            #[cfg(windows)]
            _job: job,
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

    /// `Ok(None)` means the server answered with something that is not a command answer.
    pub fn classify(&self, text: &str) -> Result<Option<(Option<Command>, String)>, String> {
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
        let _ = self.child.kill();
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
    fn classifies_with_a_live_server() {
        let var = std::env::var("ERINDI_LLAMA").unwrap();
        let (exe, model) = var.split_once(';').unwrap();
        let server = LlamaServer::start(Path::new(exe), Path::new(model)).unwrap();
        let (command, _) = server
            .classify("давай с чистого листа, напиши README")
            .unwrap()
            .unwrap();
        assert_eq!(command, Some(Command::NewSession));
    }
}
