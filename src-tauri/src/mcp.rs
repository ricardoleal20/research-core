// MCP client: real JSON-RPC over stdio for local MCP servers, plus built-in
// research-tool adapters (arXiv / Semantic Scholar REST) exposed as tools.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDef {
    pub id: String,
    pub name: String,
    pub transport: String, // "stdio" | "http"
    pub command: Option<String>,
    pub args: Option<String>,
    pub env: Option<String>,
    pub url: Option<String>,
    pub tags: Option<String>,
}

/// A connected stdio MCP server session.
pub struct StdioSession {
    child: Child,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    stdout: Arc<Mutex<tokio::process::ChildStdout>>,
    next_id: Mutex<u64>,
    tools: Mutex<Vec<McpTool>>,
}

impl StdioSession {
    pub async fn spawn(def: &McpServerDef) -> Result<Self, String> {
        let cmd = def.command.clone().ok_or("missing command")?;
        // shell-arg-split the args string (also expands ~)
        let args = shell_split(def.args.as_deref().unwrap_or(""));
        let mut command = Command::new(&cmd);
        command.args(&args);
        // GUI .app bundles on macOS launch with a minimal PATH
        // (/usr/bin:/bin:/usr/sbin:/sbin) that excludes Homebrew and nvm, so
        // `npx`/`node` aren't found and stdio MCP servers fail to spawn.
        // Augment PATH with the common tool locations before spawning.
        command.env("PATH", augmented_path());
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::null());
        if let Some(env_s) = &def.env {
            for pair in env_s.split(',') {
                if let Some((k, v)) = pair.split_once('=') {
                    command.env(k.trim(), v.trim());
                }
            }
        }
        // Expand ~ in args
        let mut child = command.spawn().map_err(|e| format!("spawn {cmd}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut s = StdioSession {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(stdout)),
            next_id: Mutex::new(1),
            tools: Mutex::new(vec![]),
        };
        s.initialize().await?;
        Ok(s)
    }

    async fn next_id(&self) -> u64 {
        let mut g = self.next_id.lock().await;
        let v = *g; *g += 1; v
    }

    async fn call_raw(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id().await;
        let req = json!({ "jsonrpc":"2.0", "id": id, "method": method, "params": params });
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())? + "\n";
        {
            let mut sin = self.stdin.lock().await;
            sin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
            sin.flush().await.map_err(|e| e.to_string())?;
        }
        // Read one newline-terminated JSON line. Bound the wait so a stalled
        // server (e.g. npx still resolving, or a handshake mismatch) fails fast
        // with a clear reason instead of hanging indefinitely.
        let resp_str = {
            let mut sout = self.stdout.lock().await;
            let read_line = async {
                let mut buf = Vec::new();
                loop {
                    let mut byte = [0u8; 1];
                    let n = sout.read(&mut byte).await.map_err(|e| e.to_string())?;
                    if n == 0 { return Err("MCP server closed stdout".into()); }
                    if byte[0] == b'\n' { break; }
                    buf.push(byte[0]);
                    if buf.len() > 1_000_000 { return Err("MCP response too large".into()); }
                }
                String::from_utf8(buf).map_err(|e| e.to_string())
            };
            tokio::time::timeout(std::time::Duration::from_secs(10), read_line)
                .await
                .map_err(|_| "MCP response timed out (>10s)".to_string())??
        };
        let resp: Value = serde_json::from_str(&resp_str)
            .map_err(|e| format!("MCP parse error: {} :: {}", e, &resp_str[..resp_str.len().min(160)]))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("MCP error: {}", err));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn initialize(&self) -> Result<(), String> {
        let _ = self.call_raw("initialize", json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "research-core", "version": "0.1.0" }
        })).await?;
        // initialized notification (no id)
        let notif = json!({ "jsonrpc":"2.0", "method":"notifications/initialized" });
        let line = serde_json::to_string(&notif).map_err(|e| e.to_string())? + "\n";
        let mut sin = self.stdin.lock().await;
        sin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
        sin.flush().await.map_err(|e| e.to_string())?;
        // list tools
        let res = self.call_raw("tools/list", json!({})).await?;
        let tools: Vec<McpTool> = if let Some(arr) = res.get("tools") {
            serde_json::from_value(arr.clone()).unwrap_or_default()
        } else { vec![] };
        *self.tools.lock().await = tools;
        Ok(())
    }

    pub async fn tools(&self) -> Vec<McpTool> {
        self.tools.lock().await.clone()
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, String> {
        let res = self.call_raw("tools/call", json!({ "name": name, "arguments": args })).await?;
        Ok(res)
    }

    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for c in s.chars() {
        match c {
            '"' => in_q = !in_q,
            ' ' if !in_q => {
                if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    // expand ~
    out.iter().map(|a| expand_tilde(a)).collect()
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), rest);
        }
    }
    s.to_string()
}

/// Build a PATH that includes the directories where node/npx live on macOS,
/// so stdio MCP servers can spawn `npx` even when the app is launched as a
/// GUI .app bundle (which inherits a minimal PATH). Existing PATH entries are
/// preserved and de-duplicated.
pub fn augmented_path() -> String {
    let mut dirs: Vec<String> = vec![
        "/opt/homebrew/bin".into(),
        "/opt/homebrew/sbin".into(),
        "/usr/local/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
    ];
    // Best-effort: add nvm/volta/asdf node shims if present.
    if let Some(home) = dirs::home_dir() {
        let h = home.display().to_string();
        for extra in [
            format!("{h}/.volta/bin"),
            format!("{h}/.asdf/shims"),
        ] {
            if !dirs.contains(&extra) {
                dirs.push(extra);
            }
        }
    }
    if let Ok(cur) = std::env::var("PATH") {
        for part in cur.split(':') {
            if !part.is_empty() && !dirs.contains(&part.to_string()) {
                dirs.push(part.to_string());
            }
        }
    }
    dirs.join(":")
}

/// Registry of live stdio MCP sessions.
pub struct McpRegistry {
    pub sessions: Mutex<HashMap<String, Arc<StdioSession>>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }

    pub async fn connect(&self, def: &McpServerDef) -> Result<Vec<McpTool>, String> {
        if def.transport != "stdio" {
            return Err("only stdio transport is connectable here".into());
        }
        let session = StdioSession::spawn(def).await?;
        let tools = session.tools().await;
        self.sessions.lock().await.insert(def.id.clone(), Arc::new(session));
        Ok(tools)
    }

    pub async fn disconnect(&self, id: &str) {
        if let Some(s) = self.sessions.lock().await.remove(id) {
            // Arc cannot kill directly; drop will close pipes.
            drop(s);
        }
    }

    pub async fn call(&self, server_id: &str, tool: &str, args: Value) -> Result<Value, String> {
        let session = self.sessions.lock().await.get(server_id).cloned()
            .ok_or("server not connected")?;
        session.call_tool(tool, args).await
    }
}

// ---- built-in research adapters (REST, not MCP) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub abstract_text: Option<String>,
}

/// Search arXiv via the public Atom API.
pub async fn arxiv_search(query: &str, max: u32) -> Result<Vec<SearchResult>, String> {
    let client = reqwest::Client::builder()
        .user_agent("research-core/0.1")
        .timeout(std::time::Duration::from_secs(20))
        .build().map_err(|e| e.to_string())?;
    let url = format!("https://export.arxiv.org/api/query?search_query=all:{}&start=0&max_results={}",
        urlencoding::encode(query), max);
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok(parse_arxiv_atom(&text))
}

/// Fetch one paper's metadata by arXiv id via the public export API (the
/// onboarding paste flow, Story 1.9). A data fetch, not an LLM call — so it
/// lives here in the research-adapters module rather than behind the provider
/// layer (AD-9 governs LLM calls only). `Ok(None)` = the id exists on no
/// paper.
pub async fn arxiv_fetch(arxiv_id: &str) -> Result<Option<SearchResult>, String> {
    let client = reqwest::Client::builder()
        .user_agent("research-core/0.1")
        .timeout(std::time::Duration::from_secs(20))
        .build().map_err(|e| e.to_string())?;
    let url = format!(
        "https://export.arxiv.org/api/query?id_list={}",
        urlencoding::encode(arxiv_id)
    );
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok(parse_arxiv_atom(&text).into_iter().next())
}

fn parse_arxiv_atom(xml: &str) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let title = extract(entry, "title").trim().replace('\n', " ");
        let id = extract(entry, "id");
        let arxiv_id = id.rsplit_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| id.to_string());
        let mut authors = Vec::new();
        let mut rest = entry;
        while let Some(start) = rest.find("<author>") {
            rest = &rest[start..];
            if let Some(name_start) = rest.find("<name>") {
                rest = &rest[name_start..];
                if let Some(end) = rest.find("</name>") {
                    let name = rest[6..end].trim().to_string();
                    if !name.is_empty() { authors.push(name); }
                    rest = &rest[end..];
                }
            } else { break; }
        }
        let year = extract(entry, "published").get(0..4).and_then(|s| s.parse().ok());
        let summary = extract(entry, "summary").trim().replace('\n', " ");
        out.push(SearchResult {
            title,
            authors: authors.join(", "),
            year,
            venue: "arXiv".into(),
            doi: format!("10.48550/arXiv.{}", arxiv_id),
            url: format!("https://arxiv.org/abs/{}", arxiv_id),
            abstract_text: if summary.is_empty() { None } else { Some(summary) },
        });
    }
    out
}

fn extract<'a>(s: &'a str, tag: &str) -> &'a str {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(i) = s.find(&open) {
        if let Some(j) = s[i..].find(&close) {
            return &s[i + open.len()..i + j];
        }
    }
    ""
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut out = String::new();
        for &b in s.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    }
}
