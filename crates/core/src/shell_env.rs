//! The environment of the user's login shell. A macOS app started from Finder, the Dock or at
//! login gets only launchd's short PATH, so agents and their `node` would not be found.

use std::sync::Mutex;
#[cfg(unix)]
use std::time::Duration;

const BEGIN: &str = "ERINDI-ENV-BEGIN";
const END: &str = "ERINDI-ENV-END";

static SNAPSHOT: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

/// Prints this process's environment between markers; Erindi runs itself this way inside the
/// login shell, so rc-file output around the block is ignored.
pub fn print_env() {
    let vars: serde_json::Map<String, serde_json::Value> = std::env::vars_os()
        .map(|(k, v)| {
            let v = v.to_string_lossy().into_owned();
            (k.to_string_lossy().into_owned(), v.into())
        })
        .collect();
    println!("{BEGIN}\n{}\n{END}", serde_json::Value::Object(vars));
}

/// fish and Nushell cannot run the POSIX command line below, so they fall back to zsh.
pub fn login_shell(shell: Option<&str>) -> &str {
    let posix = |s: &&str| {
        let name = std::path::Path::new(s).file_name().and_then(|n| n.to_str());
        matches!(name, Some("zsh" | "bash" | "sh"))
    };
    shell.filter(posix).unwrap_or("/bin/zsh")
}

/// The variables in the last marked block of `output`.
pub fn parse(output: &str) -> Option<Vec<(String, String)>> {
    let start = output.rfind(BEGIN)? + BEGIN.len();
    let end = start + output[start..].find(END)?;
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(output[start..end].trim()).ok()?;
    Some(
        map.into_iter()
            .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string()))
            .collect(),
    )
}

/// Runs `shell` as an interactive login shell that prints Erindi's environment, and gives up
/// after `timeout` so a slow or prompting rc file cannot hold Erindi.
#[cfg(unix)]
pub fn read_env(shell: &str, timeout: Duration) -> Option<Vec<(String, String)>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().ok()?.display().to_string();
    let quoted = format!("'{}'", exe.replace('\'', r"'\''"));
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("{quoted} --print-env")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(timeout);
    let _ = child.kill();
    let _ = child.wait();
    parse(&out.ok()?)
}

/// Takes a fresh snapshot of the login-shell environment; keeps the previous one on failure.
pub fn refresh() {
    #[cfg(target_os = "macos")]
    {
        let shell = std::env::var("SHELL").ok();
        match read_env(login_shell(shell.as_deref()), Duration::from_secs(5)) {
            Some(vars) => *SNAPSHOT.lock().unwrap() = Some(vars),
            None => eprintln!("The login shell gave no environment; using Erindi's own"),
        }
    }
}

/// The login-shell environment, or this process's own when there is no snapshot.
pub fn vars() -> Vec<(String, String)> {
    SNAPSHOT
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| std::env::vars().collect())
}

/// `PATH` from [`vars`].
pub fn path() -> String {
    vars()
        .into_iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc_noise_around_the_block_is_ignored() {
        let out = "Welcome!\nERINDI-ENV-BEGIN\n{\"PATH\":\"/a:/b\",\"X\":\"1=2\\nline\"}\nERINDI-ENV-END\nbye\n";
        let vars = parse(out).unwrap();
        assert!(vars.contains(&("PATH".into(), "/a:/b".into())));
        assert!(vars.contains(&("X".into(), "1=2\nline".into())));
    }

    #[test]
    fn no_block_is_none() {
        assert_eq!(parse("oops"), None);
        assert_eq!(parse("ERINDI-ENV-BEGIN\nnot json\nERINDI-ENV-END"), None);
    }

    #[test]
    fn only_posix_shells_are_trusted() {
        assert_eq!(
            login_shell(Some("/opt/homebrew/bin/zsh")),
            "/opt/homebrew/bin/zsh"
        );
        assert_eq!(login_shell(Some("/bin/bash")), "/bin/bash");
        assert_eq!(login_shell(Some("/bin/sh")), "/bin/sh");
        assert_eq!(login_shell(Some("/opt/homebrew/bin/fish")), "/bin/zsh");
        assert_eq!(login_shell(Some("/usr/local/bin/nu")), "/bin/zsh");
        assert_eq!(login_shell(None), "/bin/zsh");
    }

    #[cfg(unix)]
    fn fake_shell(body: &str) -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sh = dir.path().join("zsh");
        std::fs::write(&sh, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = sh.display().to_string();
        (dir, path)
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_prints_the_block_gives_its_env() {
        let (_dir, sh) = fake_shell(
            "echo hi; echo ERINDI-ENV-BEGIN; echo '{\"PATH\":\"/x\"}'; echo ERINDI-ENV-END",
        );
        let vars = read_env(&sh, std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(vars, [("PATH".to_string(), "/x".to_string())]);
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_shell_gives_up_after_the_timeout() {
        let (_dir, sh) = fake_shell("sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(read_env(&sh, std::time::Duration::from_secs(1)), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}
