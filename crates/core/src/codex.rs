use crate::agent::Target;

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
pub fn exec_args(
    model: Option<&str>,
    sandbox: Option<&str>,
    target: &Target,
    cwd: &str,
) -> Vec<String> {
    match target {
        Target::Resume(id) => ["exec", "resume", id.as_str(), "--json"]
            .map(String::from)
            .into(),
        Target::New(_) => {
            let mut args: Vec<String> = ["exec", "--json", "-C", cwd].map(String::from).into();
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
pub fn terminal_args(
    program: &str,
    cwd: &str,
    model: Option<&str>,
    sandbox: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec!["-d".into(), cwd.into(), program.into()];
    args.extend(options(model, sandbox));
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
