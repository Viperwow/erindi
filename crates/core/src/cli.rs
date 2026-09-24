use std::path::PathBuf;

use crate::agent::Agent;

/// The first `name` + extension from `pathext` in the directories of `path`.
pub fn find(name: &str, path: &str, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    path.split(';')
        .map(|d| d.trim().trim_matches('"'))
        .filter(|d| !d.is_empty())
        .find_map(|dir| {
            exts.iter()
                .map(|ext| PathBuf::from(dir).join(format!("{name}{}", ext.to_lowercase())))
                .find(|p| p.is_file())
        })
}

/// Windows builds a process PATH from the system value followed by the user value.
fn merge(system: &str, user: &str) -> String {
    [system, user]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

/// The PATH a newly started program would get, so a CLI installed while Erindi runs is found.
#[cfg(windows)]
pub fn current_path() -> String {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let system = registry_path(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    );
    let user = registry_path(HKEY_CURRENT_USER, "Environment");
    match (system, user) {
        (None, None) => std::env::var("PATH").unwrap_or_default(),
        (s, u) => merge(&s.unwrap_or_default(), &u.unwrap_or_default()),
    }
}

#[cfg(not(windows))]
pub fn current_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// `Path` under `subkey`, with `%VARS%` expanded by the registry API.
#[cfg(windows)]
fn registry_path(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::HSTRING;
    let (key, value) = (HSTRING::from(subkey), HSTRING::from("Path"));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    // SAFETY: a size query with no buffer.
    unsafe { RegGetValueW(root, &key, &value, flags, None, None, Some(&mut size)) }
        .ok()
        .ok()?;
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `buf` holds `size` bytes, as the registry just reported.
    unsafe {
        RegGetValueW(
            root,
            &key,
            &value,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn locate(agent: Agent) -> Option<PathBuf> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    find(agent.cli(), &current_path(), &pathext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "").unwrap();
        p
    }

    #[test]
    fn cmd_files_are_found_through_pathext() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let cmd = touch(b.path(), "codex.cmd");
        let path = format!("{};{}", a.path().display(), b.path().display());
        assert_eq!(find("codex", &path, ".COM;.EXE;.BAT;.CMD"), Some(cmd));
    }

    #[test]
    fn earlier_directories_win_and_exe_beats_cmd_in_one_directory() {
        let a = tempfile::tempdir().unwrap();
        let exe = touch(a.path(), "claude.exe");
        touch(a.path(), "claude.cmd");
        let path = format!("{};C:\\nowhere", a.path().display());
        assert_eq!(find("claude", &path, ".EXE;.CMD"), Some(exe));
    }

    #[test]
    fn missing_or_empty_entries_find_nothing() {
        assert_eq!(find("codex", r";;C:\nowhere", ".EXE;.CMD"), None);
        assert_eq!(find("codex", "", ".EXE"), None);
    }

    #[test]
    fn quoted_entries_are_unquoted() {
        let a = tempfile::tempdir().unwrap();
        let exe = touch(a.path(), "codex.exe");
        let path = format!("\"{}\"", a.path().display());
        assert_eq!(find("codex", &path, ".EXE"), Some(exe));
    }

    #[test]
    fn merge_puts_system_entries_first() {
        assert_eq!(
            merge(r"C:\sys;C:\win", r"C:\user"),
            r"C:\sys;C:\win;C:\user"
        );
        assert_eq!(merge(r"C:\sys", ""), r"C:\sys");
    }
}
