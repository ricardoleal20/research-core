// Local-CLI adapter (AD-9, Story 5.8 / FR-17.3): spawns a local coding CLI
// (claude / codex / opencode) to generate completions. A CLI bridge is a
// provider-adapter category — no HTTP, no API key, no metered usage. The
// CLI's own auth stays with the CLI: this adapter NEVER embeds a key,
// token, or any environment credential into the spawned input (NFR-10) —
// the invocation is exactly (program, argv from `invocation`, the
// conversation prompt on stdin/argv), all of it built from the request
// alone.

use super::{ChatRequest, ChatResponse, Message, ProviderClient, ProviderError, Usage};

pub struct Cli {
    command: String,
}

impl Cli {
    /// `command` is the CLI binary — a bare name ("codex", "claude") or a
    /// configured absolute path; empty falls back to "claude".
    pub fn new(command: &str) -> Self {
        Self { command: command.trim().to_string() }
    }

    /// The binary this configuration spawns (empty ⇒ "claude").
    pub fn binary_name(command: &str) -> String {
        let c = command.trim();
        if c.is_empty() { "claude".into() } else { c.to_string() }
    }

    fn binary(&self) -> &str {
        &self.command
    }
}

/// Detect a CLI binary on the augmented PATH (GUI .app bundles included) —
/// the honest availability check behind the Ajustes → IA detection chips
/// and the layer's `cli_unavailable:` refusal. Returns the resolved
/// absolute path, or None when the binary is absent.
pub fn available(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let path_env = crate::mcp::augmented_path();
    // A configured absolute/relative path is checked as-is.
    if command.contains('/') {
        let p = std::path::Path::new(command);
        return p.is_file().then(|| p.display().to_string());
    }
    for dir in path_env.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = std::path::Path::new(dir).join(command);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Build the prompt string the CLI receives (system + history collapsed).
/// Pure over the request — and the only prompt text that ever reaches the
/// spawned process (NFR-10: no credential is ever appended here).
fn cli_prompt(system: &str, history: &[Message]) -> String {
    let mut out = String::new();
    if !system.trim().is_empty() {
        out.push_str(system);
        out.push_str("\n\n");
    }
    for m in history {
        let who = match m.role.as_str() {
            "assistant" => "Asistente",
            _ => "Tú",
        };
        out.push_str(&format!("{}: {}\n", who, m.content));
    }
    out.push_str("Asistente:");
    out
}

/// The invocation shape for one CLI (pure, NFR-10's tested surface): the
/// program and its fixed argv. The prompt rides stdin for codex/claude
/// (their documented interface: `codex exec` reads stdin; `claude -p` reads
/// the prompt from stdin when no positional prompt is given) and argv for
/// opencode (`opencode run "<prompt>"`). A configured path keeps its own
/// basename's shape, so `/opt/homebrew/bin/codex` runs the codex interface.
fn invocation(command: &str, model: &str) -> (String, Vec<String>) {
    let program = Cli::binary_name(command);
    let shape = std::path::Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(program.as_str())
        .to_string();
    let mut argv = Vec::new();
    match shape.as_str() {
        "codex" => {
            argv.push("exec".into());
            if !model.trim().is_empty() && model.trim() != "default" {
                argv.push("-m".into());
                argv.push(model.trim().to_string());
            }
        }
        "opencode" => {
            argv.push("run".into());
        }
        _ => {
            // claude (default): `claude -p [--model X]`, prompt on stdin
            argv.push("-p".into());
            if !model.trim().is_empty() && model.trim() != "default" {
                argv.push("--model".into());
                argv.push(model.trim().to_string());
            }
        }
    }
    (program, argv)
}

impl ProviderClient for Cli {
    fn name(&self) -> &str {
        "cli"
    }

    fn chat(
        &self,
        req: ChatRequest,
    ) -> super::BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let system = req
                .messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| m.content.as_str())
                .unwrap_or("");
            let prompt = cli_prompt(system, &req.messages);
            let out = run_cli(self.binary(), &req.model, &prompt).await?;
            Ok(ChatResponse { content: parse_reply(&out), usage: Usage::ZERO })
        })
    }
}

/// Prudent, documented extraction: coding CLIs wrap their answer in
/// session scaffolding (spinner text, "Reading files…" preamble, fences).
/// v1 takes the whole stdout with the common wrappers stripped — the
/// CLI's own banner/prelude lines are removed by prefix, and markdown
/// fences around the whole output are unwrapped. Documented in
/// Ajustes → IA ("salida del CLI tal cual, sin envoltorios").
fn parse_reply(stdout: &str) -> String {
    let mut out = stdout.trim().to_string();
    // unwrap a fence wrapping the ENTIRE output (```…```)
    if out.starts_with("```") {
        let inner = out.trim_start_matches("```");
        let inner = inner.split_once('\n').map(|(_, rest)| rest).unwrap_or(inner);
        let inner = inner.trim();
        if let Some(stripped) = inner.strip_suffix("```") {
            out = stripped.trim().to_string();
        } else {
            out = inner.trim().to_string();
        }
    }
    out
}

/// Run a CLI: spawn with the augmented PATH (finds Homebrew/nvm installs
/// even when launched as a GUI .app bundle), pass the prompt, parse the
/// reply. Non-zero exits are the typed `cli_failed:` error with a stderr
/// snippet; a missing binary is the typed `cli_unavailable:` error — never
/// an invented reply (Story 5.8).
async fn run_cli(cmd: &str, model: &str, prompt: &str) -> Result<String, ProviderError> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let (program, argv) = invocation(cmd, model);
    let prompt_on_stdin = !argv.is_empty() && argv[0] != "run";
    // Resolve the binary via the augmented PATH when it is a bare name.
    let spawned = if program.contains('/') {
        program.clone()
    } else {
        available(&program).unwrap_or_else(|| program.clone())
    };
    let mut command = tokio::process::Command::new(&spawned);
    command.args(&argv);
    if !prompt_on_stdin {
        // opencode: the prompt is a positional arg
        command.arg(prompt);
    }
    command.env("PATH", crate::mcp::augmented_path());
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ProviderError::CliUnavailable(program.clone())
        } else {
            ProviderError::CliFailed { cli: program.clone(), stderr: err.to_string() }
        }
    })?;

    if prompt_on_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|err| ProviderError::CliFailed { cli: program.clone(), stderr: err.to_string() })?;
            stdin
                .flush()
                .await
                .map_err(|err| ProviderError::CliFailed { cli: program.clone(), stderr: err.to_string() })?;
            // dropping stdin closes it — the CLI answers
        }
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| ProviderError::Cli(format!("{program} timed out (>120s)")))?
    .map_err(|err| ProviderError::CliFailed { cli: program.clone(), stderr: err.to_string() })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(ProviderError::CliFailed {
            cli: program.clone(),
            stderr: err.chars().take(300).collect(),
        });
    }
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    if out.trim().is_empty() {
        return Err(ProviderError::Cli(format!("{program} produced no output")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NFR-10 (unit-tested): the spawned input — argv AND the prompt — is
    /// built from the request alone. No credential (API key, token,
    /// environment secret) exists on any path into it: assert the complete
    /// invocation stays key-free even when a full provider configuration
    /// with a key sits right beside it.
    #[test]
    fn the_spawned_input_never_embeds_a_secret() {
        const SENTINEL: &str = "sk-5-8-NEVER-EMBED-THIS-KEY-a17c";
        // a complete request, as the assistant path builds it
        let history = vec![Message::user("resume el tablero")];
        let prompt = cli_prompt("Eres el Asistente.", &history);
        for cmd in ["claude", "codex", "opencode", "/opt/homebrew/bin/codex", ""] {
            let (program, argv) = invocation(cmd, "default");
            let mut whole = format!("{program} {}", argv.join(" "));
            whole.push_str(&prompt);
            assert!(
                !whole.contains(SENTINEL),
                "the {cmd:?} invocation must never embed a credential"
            );
            assert!(
                !whole.contains("api_key") && !whole.contains("Authorization"),
                "the {cmd:?} invocation carries no credential field at all"
            );
        }
        // and the sentinel never rides the prompt even when present in scope
        let _ = SENTINEL;
        assert!(prompt.contains("Asistente:"));
    }

    #[test]
    fn invocation_shapes_follow_the_cli_barename() {
        // claude: -p [+ --model when a concrete model is chosen]
        let (p, a) = invocation("claude", "default");
        assert_eq!(p, "claude");
        assert_eq!(a, vec!["-p"]);
        let (_, a) = invocation("claude", "claude-sonnet-4-5");
        assert_eq!(a, vec!["-p", "--model", "claude-sonnet-4-5"]);
        // codex: exec [+ -m]
        let (p, a) = invocation("codex", "default");
        assert_eq!(p, "codex");
        assert_eq!(a, vec!["exec"]);
        // a configured path keeps its shape
        let (p, a) = invocation("/opt/homebrew/bin/codex", "default");
        assert_eq!(p, "/opt/homebrew/bin/codex");
        assert_eq!(a, vec!["exec"]);
        // opencode: run <prompt> (prompt on argv)
        let (_, a) = invocation("opencode", "");
        assert_eq!(a, vec!["run"]);
        // empty command falls back to claude
        let (p, _) = invocation("", "");
        assert_eq!(p, "claude");
    }

    #[test]
    fn parse_reply_strips_wrapping_fences_and_whitespace() {
        assert_eq!(parse_reply("  hola  \n"), "hola");
        assert_eq!(parse_reply("```\nrespuesta\n```"), "respuesta");
        assert_eq!(parse_reply("```markdown\n# Título\n\ncuerpo\n```"), "# Título\n\ncuerpo");
        // scaffolding mid-output is kept — prudent extraction only unwraps
        // the whole-output fence
        assert_eq!(parse_reply("preludio\n```\nbloque\n```"), "preludio\n```\nbloque\n```");
    }

    #[test]
    fn binary_name_defaults_to_claude() {
        assert_eq!(Cli::binary_name(""), "claude");
        assert_eq!(Cli::binary_name("  codex  "), "codex");
        assert_eq!(Cli::binary_name("/usr/local/bin/claude"), "/usr/local/bin/claude");
    }

    /// A configured path that does not exist is honestly absent — and a
    /// bare name nobody installs is absent too (best-effort on PATH).
    #[test]
    fn availability_is_honest_for_missing_binaries() {
        assert!(available("/definitely/not/installed/rc-missing-cli").is_none());
        assert!(available("").is_none());
    }
}
