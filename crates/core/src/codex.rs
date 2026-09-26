use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::agent::{ModelOption, Target};
use crate::stream::RunEvent;

/// Windows basics shared with Claude, plus what Codex and its Node launcher read.
const ENV_ALLOW_PREFIX: &[&str] = &["CODEX_", "OPENAI_"];

pub fn codex_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| {
            let k = k.to_uppercase();
            crate::claude::base_env_allowed(&k) || ENV_ALLOW_PREFIX.iter().any(|p| k.starts_with(p))
        })
        .collect()
}

/// A native ID Codex could read as an option is refused by the caller before this runs.
/// The user picked the folder in Settings, so Codex's own git-repo guard is skipped.
/// The flag does not make Codex load the folder's `.codex/` config or hooks.
pub fn exec_args(
    model: Option<&str>,
    sandbox: Option<&str>,
    target: &Target,
    cwd: &str,
) -> Vec<String> {
    match target {
        Target::Resume(id) => [
            "exec",
            "resume",
            id.as_str(),
            "--json",
            "--skip-git-repo-check",
        ]
        .map(String::from)
        .into(),
        Target::New(_) => {
            let mut args: Vec<String> = ["exec", "--json", "--skip-git-repo-check", "-C", cwd]
                .map(String::from)
                .into();
            args.extend(options(model, sandbox));
            args
        }
    }
}

fn options(model: Option<&str>, sandbox: Option<&str>) -> Vec<String> {
    let mut args = vec![];
    if let Some(model) = model {
        args.extend(["-m".into(), model.to_string()]);
    }
    if let Some(sandbox) = sandbox {
        args.extend(["-s".into(), sandbox.to_string()]);
    }
    args
}

/// The prompt follows `--`, and its `;` is escaped because Windows Terminal splits commands there.
/// A resumed session keeps its own model and sandbox, so only a new one gets the options.
pub fn terminal_args(
    program: &str,
    cwd: &str,
    model: Option<&str>,
    sandbox: Option<&str>,
    target: &Target,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec!["-d".into(), cwd.into(), program.into()];
    match target {
        Target::Resume(id) => args.extend(["resume".into(), id.clone()]),
        Target::New(_) => args.extend(options(model, sandbox)),
    }
    if !prompt.is_empty() {
        args.extend(["--".into(), prompt.replace(';', r"\;")]);
    }
    args
}

pub fn resume_in_terminal(program: &str, cwd: &str, native_id: &str) -> Vec<String> {
    ["-d", cwd, program, "resume", native_id]
        .map(String::from)
        .into()
}

/// Unknown, malformed or irrelevant lines yield no events.
pub fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let item = &v["item"];
    match (v["type"].as_str(), item["type"].as_str()) {
        (Some("thread.started"), _) => match v["thread_id"].as_str() {
            Some(id) => vec![RunEvent::SessionStarted {
                native_id: id.into(),
            }],
            None => vec![],
        },
        (Some("item.started"), Some("command_execution")) => vec![RunEvent::ToolUse {
            name: text(&item["command"]),
        }],
        (Some("item.started"), Some("file_change")) => {
            let path = text(&item["changes"][0]["path"]);
            vec![RunEvent::ToolUse {
                name: format!("Edit {path}"),
            }]
        }
        (Some("item.completed"), Some("agent_message")) => vec![RunEvent::Reply {
            text: text(&item["text"]),
        }],
        (Some("turn.completed"), _) => vec![RunEvent::Result {
            ok: true,
            text: String::new(),
        }],
        (Some("turn.failed"), _) => vec![RunEvent::Result {
            ok: false,
            text: text(&v["error"]["message"]),
        }],
        (Some("error"), _) => vec![RunEvent::Result {
            ok: false,
            text: text(&v["message"]),
        }],
        _ => vec![],
    }
}

/// Models from `codex debug models` that Codex shows in its own picker.
pub fn parse_models(json: &str) -> Result<Vec<ModelOption>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let list = v["models"].as_array().ok_or("no \"models\" list")?;
    Ok(list
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?;
            let label = m["display_name"].as_str().unwrap_or(id);
            Some(ModelOption {
                id: id.into(),
                label: label.into(),
            })
        })
        .collect())
}

/// In a folder its own `config.toml` does not trust, Codex defaults to a read-only sandbox and
/// skips the folder's `.codex/` hooks, MCP servers and config. The folder or its git root must be
/// listed; trusting a parent does not count.
pub fn limited(folder: &Path, codex_config: &str) -> bool {
    let root = folder
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(folder);
    !trusted(&[folder, root], codex_config)
}

fn trusted(dirs: &[&Path], codex_config: &str) -> bool {
    let Ok(config) = codex_config.parse::<toml::Table>() else {
        return false;
    };
    let Some(projects) = config.get("projects").and_then(|p| p.as_table()) else {
        return false;
    };
    let wanted: Vec<String> = dirs
        .iter()
        .map(|d| same_path(&d.to_string_lossy()))
        .collect();
    projects.iter().any(|(path, project)| {
        project.get("trust_level").and_then(|t| t.as_str()) == Some("trusted")
            && wanted.contains(&same_path(path))
    })
}

/// Windows paths compare without case, separator style or a trailing separator.
fn same_path(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

pub fn config_path(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map_or(home.join(".codex"), PathBuf::from)
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_until_codex_trusts_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let key = folder.to_string_lossy().to_uppercase().replace('\\', "/");
        let trusted = format!("[projects.'{key}/']\ntrust_level = \"trusted\"\n");
        let parent = format!(
            "[projects.'{}']\ntrust_level = \"trusted\"\n",
            folder.parent().unwrap().display()
        );

        assert!(limited(folder, ""));
        assert!(limited(folder, "not toml ["));
        assert!(limited(folder, &parent));
        assert!(!limited(folder, &trusted));
        assert!(limited(
            folder,
            &trusted.replace("\"trusted\"", "\"untrusted\"")
        ));
    }

    #[test]
    fn a_subfolder_uses_the_trust_of_its_git_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        let sub = root.join("app");
        std::fs::create_dir(&sub).unwrap();
        let trusted = format!(
            "[projects.'{}']\ntrust_level = \"trusted\"\n",
            root.display()
        );
        assert!(limited(&sub, ""));
        assert!(!limited(&sub, &trusted));
    }

    const FIXTURE: &str = include_str!("../tests/fixtures/codex-exec.jsonl");

    #[test]
    fn events_from_a_real_run() {
        let events: Vec<_> = FIXTURE.lines().flat_map(parse_line).collect();
        assert_eq!(
            events,
            [
                RunEvent::SessionStarted {
                    native_id: "01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2".into()
                },
                RunEvent::ToolUse {
                    name: "git diff".into()
                },
                RunEvent::ToolUse {
                    name: "Edit src/a.rs".into()
                },
                RunEvent::Reply {
                    text: "pong".into()
                },
                RunEvent::Result {
                    ok: true,
                    text: String::new()
                },
            ]
        );
    }

    #[test]
    fn failures() {
        let line = r#"{"type":"turn.failed","error":{"message":"stream disconnected"}}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "stream disconnected".into()
            }]
        );
        let line = r#"{"type":"error","message":"Not logged in"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "Not logged in".into()
            }]
        );
    }

    #[test]
    fn ignores_noise_and_garbage() {
        for line in [
            "",
            "x",
            "{\"type\":",
            "[1]",
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.started","item":"oops"}"#,
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"…"}}"#,
        ] {
            assert_eq!(parse_line(line), [], "{line}");
        }
    }

    #[test]
    fn listed_models_only() {
        let models = parse_models(include_str!("../tests/fixtures/codex-models.json")).unwrap();
        let ids: Vec<_> = models
            .iter()
            .map(|m| (m.id.as_str(), m.label.as_str()))
            .collect();
        assert_eq!(ids, [("gpt-6-sol", "GPT-6-Sol"), ("gpt-5.5", "GPT-5.5")]);
    }

    #[test]
    fn broken_catalog_is_an_error() {
        for bad in ["", "{", r#"{"models":"x"}"#, r#"{"other":[]}"#] {
            assert!(parse_models(bad).is_err(), "{bad}");
        }
    }
}
