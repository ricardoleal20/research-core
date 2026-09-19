// Local-CLI adapter (AD-9): spawns a local coding CLI (claude / codex /
// opencode) to generate completions. Local-first: no HTTP, no API key, no
// measurable token usage — CLI calls record no spend.

use super::{ChatRequest, ChatResponse, Message, ProviderClient, ProviderError, Usage};

pub struct Cli {
    command: String,
}

impl Cli {
    /// `command` is the CLI binary; empty falls back to "claude".
    pub fn new(command: &str) -> Self {
        Self { command: command.trim().to_string() }
    }

    fn binary(&self) -> &str {
        if self.command.is_empty() { "claude" } else { &self.command }
    }
}

/// Build the prompt string the CLI receives (system + history collapsed).
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
            Ok(ChatResponse { content: out, usage: Usage::ZERO })
        })
    }
}

/// Run a CLI: `claude -p "<prompt>"` (with optional `--model`). For codex /
/// opencode we use the same shape (`codex exec`, `opencode run`), since all
/// three accept a prompt and print the result to stdout.
async fn run_cli(cmd: &str, model: &str, prompt: &str) -> Result<String, ProviderError> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let e = |msg: String| ProviderError::Cli(msg);
    // Resolve the binary via the augmented PATH (finds Homebrew/nvm installs
    // even when launched as a GUI .app bundle).
    let resolved = crate::mcp::augmented_path();
    let mut command = match cmd {
        "codex" => {
            let mut c = tokio::process::Command::new("codex");
            c.arg("exec");
            c
        }
        "opencode" => {
            let mut c = tokio::process::Command::new("opencode");
            c.arg("run");
            c
        }
        _ => {
            // claude (default): `claude -p "<prompt>" [--model X]`
            let mut c = tokio::process::Command::new("claude");
            c.arg("-p");
            if !model.trim().is_empty() {
                c.arg("--model").arg(model);
            }
            c
        }
    };
    command.env("PATH", resolved);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    // For claude/opencode the prompt is a CLI arg; for codex it's on stdin.
    let mut child = command
        .spawn()
        .map_err(|err| e(format!("spawn {cmd}: {err} (¿instalado y en PATH?)")))?;

    if cmd == "codex" {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|err| e(err.to_string()))?;
            stdin.flush().await.map_err(|err| e(err.to_string()))?;
            // dropping stdin closes it
        }
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| e(format!("{cmd} timed out (>120s)")))?
    .map_err(|err| e(format!("{cmd}: {err}")))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(e(format!(
            "{cmd} failed: {}",
            err.chars().take(300).collect::<String>()
        )));
    }
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    if out.trim().is_empty() {
        return Err(e(format!("{cmd} produced no output")));
    }
    Ok(out)
}
