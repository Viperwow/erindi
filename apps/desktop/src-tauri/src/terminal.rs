//! Opens an agent in the system terminal: Windows Terminal, or Terminal.app on macOS.

use erindi_core::agent::TerminalCommand;

pub fn open(command: &TerminalCommand) -> Result<(), String> {
    spawn(command).map_err(|e| format!("Cannot open a terminal: {e}"))
}

#[cfg(windows)]
fn spawn(command: &TerminalCommand) -> std::io::Result<()> {
    std::process::Command::new("wt.exe")
        .args(command.wt_args())
        .spawn()
        .map(drop)
}

/// Terminal.app runs a `.command` file without asking for Automation permission. The script
/// removes itself once zsh has it open, however long the user's rc files take.
#[cfg(not(windows))]
fn spawn(command: &TerminalCommand) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!("erindi-{}.command", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&path)?;
    std::io::Write::write_all(&mut file, command.command_script().as_bytes())?;
    drop(file);
    let opened = std::process::Command::new("open")
        .args(["-a", "Terminal"])
        .arg(&path)
        .status();
    let error = match opened {
        Ok(status) if status.success() => return Ok(()),
        Ok(status) => std::io::Error::other(format!("open exited with {status}")),
        Err(e) => e,
    };
    let _ = std::fs::remove_file(&path);
    Err(error)
}
