// Ollama local provider adapter (Story 6.1, FR-24.1): the first-class local
// LLM provider — model discovery through `GET /api/tags`, chat through
// `POST /api/chat` (non-streaming v1), no API key, and a hard localhost-only
// guard (FR-24.3/NFR-14): the adapter refuses to even construct against a
// non-localhost endpoint, so no research content can leave the machine on
// the local path — the guard is the unit-tested zero-egress boundary. A
// stopped local runtime is the honest typed `local_unreachable:` error,
// never a panic and never a simulated stand-in. AD-9: this is one of the
// only places outside `mcp.rs` allowed to speak HTTP to an LLM provider.

use serde_json::{json, Value};

use super::{ChatRequest, ChatResponse, ProviderClient, ProviderError, Usage};

/// The default local endpoint (Ollama's documented default).
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// The localhost guard (FR-24.3/NFR-14, zero egress): the local adapter
/// speaks ONLY to loopback. Any other host — cloud APIs, LAN boxes, wildcard
/// binds — fails `is_localhost`, and `Ollama::new` turns that into the typed
/// `local_not_localhost:` refusal BEFORE any request can exist. Pure: the
/// unit-tested egress boundary.
fn is_localhost(base_url: &str) -> bool {
    let s = base_url.trim();
    // strip the scheme (an accidental "localhost:11434" still parses honestly)
    let rest = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
    // strip path / query / fragment, then userinfo
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    // strip the port — IPv6 literals ride in brackets ([::1]:11434); a bare
    // `::1` keeps its colons and is matched as-is by the candidate fallback
    let bracketed = authority.strip_prefix('[').and_then(|a| a.split_once(']').map(|(h, _)| h));
    let host = bracketed
        .map(str::to_string)
        .unwrap_or_else(|| {
            authority
                .rsplit_once(':')
                .map(|(h, _)| h.to_string())
                .unwrap_or_else(|| authority.to_string())
        });
    // the unbracketed authority is also a candidate (a bare IPv6 literal)
    [host.as_str(), authority]
        .iter()
        .any(|h| {
            matches!(
                h.trim().trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase().as_str(),
                "localhost" | "127.0.0.1" | "::1"
            )
        })
}

/// The local provider adapter: one localhost endpoint, no key, the native
/// Ollama API. Registering like any cloud provider means implementing the
/// same `ProviderClient` interface (FR-24.1) — callers never know a local
/// runtime answered.
pub struct Ollama {
    base_url: String,
}

impl Ollama {
    /// Construct the adapter for one local endpoint. An empty base URL is
    /// the default (`http://localhost:11434`). A non-localhost URL is the
    /// typed `local_not_localhost:` refusal — zero egress is enforced here,
    /// before any request exists (FR-24.3, NFR-14).
    pub fn new(base_url: &str) -> Result<Self, ProviderError> {
        let base = if base_url.trim().is_empty() {
            DEFAULT_BASE_URL
        } else {
            base_url.trim()
        };
        if !is_localhost(base) {
            return Err(ProviderError::NotLocalhost(base.to_string()));
        }
        Ok(Self { base_url: base.trim_end_matches('/').to_string() })
    }

    /// The (normalized) base URL this adapter speaks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn client() -> Result<reqwest::Client, ProviderError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Request { name: "local".into(), source: e })
    }

    /// Map transport failures on the local path to the honest typed
    /// `local_unreachable:` error (NFR-14): a stopped local runtime is the
    /// unconfigured state with the reason — never a panic, never a silent
    /// fallback to simulated.
    fn local_err(base_url: &str, e: reqwest::Error) -> ProviderError {
        if e.is_connect() || e.is_timeout() {
            ProviderError::LocalUnreachable { url: base_url.to_string() }
        } else {
            ProviderError::Request { name: "local".into(), source: e }
        }
    }

    /// Discover the endpoint's installed models: `GET /api/tags`
    /// (FR-24.1). A reachable endpoint that lists nothing is an honestly
    /// empty list — a model is never invented.
    pub async fn tags(&self) -> Result<Vec<String>, ProviderError> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = Self::client()?
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| Self::local_err(&self.base_url, e))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                name: "local".into(),
                status: status.as_u16(),
                body: ProviderError::api_body(body),
            });
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| Self::local_err(&self.base_url, e))?;
        Ok(parse_tags(&v))
    }
}

/// Parse an `/api/tags` body: `{"models": [{"name": "llama3.1:8b"}, …]}`.
/// Pure — the tested surface.
pub fn parse_tags(v: &Value) -> Vec<String> {
    v.get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

impl ProviderClient for Ollama {
    fn name(&self) -> &str {
        "local"
    }

    fn chat(
        &self,
        req: ChatRequest,
    ) -> super::BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let mut messages: Vec<Value> = Vec::with_capacity(req.messages.len());
            for m in &req.messages {
                messages.push(json!({ "role": m.role, "content": m.content }));
            }
            // non-streaming v1 (streaming is the runtime's later story);
            // the temperature rides Ollama's `options` shape
            let body = json!({
                "model": req.model,
                "messages": messages,
                "stream": false,
                "options": { "temperature": req.temperature },
            });
            let url = format!("{}/api/chat", self.base_url);
            let resp = Self::client()?
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| Self::local_err(&self.base_url, e))?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ProviderError::Api {
                    name: "local".into(),
                    status,
                    body: ProviderError::api_body(body),
                });
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| Self::local_err(&self.base_url, e))?;
            let content = v["message"]["content"]
                .as_str()
                .ok_or_else(|| ProviderError::EmptyCompletion { name: "local".into() })?
                .to_string();
            // honest token counts (NFR-14): the local runtime's own counters
            // ride verbatim — the $0 spend event reports real usage, never an
            // invented number.
            let usage = Usage {
                input_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
                output_tokens: v["eval_count"].as_u64().unwrap_or(0),
            };
            Ok(ChatResponse { content, usage })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::providers::Message;
    use std::sync::{Arc, Mutex};

    fn ollama_default() -> &'static str {
        DEFAULT_BASE_URL
    }

    /// A one-shot local "Ollama" on the shared test runtime: captures the
    /// chat request and answers with the native `/api/chat` shape.
    async fn fake_ollama() -> (String, Arc<Mutex<Option<String>>>) {
        let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen = captured.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new()
            .route(
                "/api/chat",
                axum::routing::post(move |body: String| {
                    let seen = seen.clone();
                    async move {
                        *seen.lock().unwrap() = Some(body);
                        axum::Json(serde_json::json!({
                            "model": "llama3.1:8b",
                            "message": { "role": "assistant", "content": "hola desde el modelo local" },
                            "prompt_eval_count": 120,
                            "eval_count": 45
                        }))
                    }
                }),
            )
            .route(
                "/api/tags",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "models": [
                            { "name": "llama3.1:8b" },
                            { "name": "qwen2.5:14b" },
                            { "name": "mistral-nemo" }
                        ]
                    }))
                }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), captured)
    }

    /// FR-24.3/NFR-14 (zero egress): the guard refuses every non-localhost
    /// endpoint at construction — cloud APIs, LAN boxes, wildcard binds,
    /// remote IPv6 — so no request to them can ever exist on the local path.
    #[test]
    fn the_localhost_guard_refuses_every_non_local_endpoint() {
        for remote in [
            "https://api.openai.com/v1",
            "http://10.0.0.5:11434",
            "http://192.168.1.10:11434",
            "http://example.com",
            "http://[2001:db8::1]:11434",
            "http://0.0.0.0:11434",
            "https://localhost.evil.example",
        ] {
            assert!(
                matches!(Ollama::new(remote), Err(ProviderError::NotLocalhost(ref u)) if u == remote),
                "`{remote}` must be refused as non-localhost"
            );
        }
        // localhost shapes are the only ones through — default when empty,
        // trailing slash normalized away
        for (local, normalized) in [
            ("http://localhost:11434", "http://localhost:11434"),
            ("http://127.0.0.1:11434", "http://127.0.0.1:11434"),
            ("http://[::1]:11434", "http://[::1]:11434"),
            ("http://localhost:11434/", "http://localhost:11434"),
            ("", ollama_default()),
        ] {
            let adapter = Ollama::new(local)
                .unwrap_or_else(|e| panic!("`{local}` is localhost and must construct: {e}"));
            assert_eq!(adapter.name(), "local");
            assert_eq!(adapter.base_url(), normalized);
        }
    }

    #[test]
    fn parse_tags_reads_the_ollama_shape() {
        let v: Value = serde_json::from_str(
            r#"{"models":[{"name":"llama3.1:8b"},{"name":"qwen2.5:14b"}]}"#,
        )
        .unwrap();
        assert_eq!(parse_tags(&v), vec!["llama3.1:8b", "qwen2.5:14b"]);
        // unparseable / empty stays empty — never invented
        assert!(parse_tags(&Value::Null).is_empty());
        assert!(parse_tags(&serde_json::json!({ "models": [] })).is_empty());
    }

    /// FR-24.1: discovery goes through `GET /api/tags` and lists the
    /// endpoint's installed models verbatim.
    #[tokio::test]
    async fn tags_list_the_installed_models() {
        let (base, _) = fake_ollama().await;
        let adapter = Ollama::new(&base).unwrap();
        assert_eq!(
            adapter.tags().await.unwrap(),
            vec!["llama3.1:8b", "qwen2.5:14b", "mistral-nemo"]
        );
    }

    /// FR-24.1: chat speaks the NATIVE `/api/chat` shape (not the
    /// OpenAI-compatible shim) — the model rides the call, the reply and the
    /// runtime's own token counters ride back verbatim.
    #[tokio::test]
    async fn chat_speaks_the_native_api_chat_shape() {
        let (base, captured) = fake_ollama().await;
        let adapter = Ollama::new(&base).unwrap();
        let resp = adapter
            .chat(
                ChatRequest::new(vec![
                    Message::system("eres el asistente"),
                    Message::user("hola"),
                ])
                .with_model("qwen2.5:14b")
                .with_temperature(0.4),
            )
            .await
            .unwrap();
        assert_eq!(resp.content, "hola desde el modelo local");
        assert_eq!(resp.usage, Usage { input_tokens: 120, output_tokens: 45 });

        let body = captured.lock().unwrap().clone().expect("the local runtime saw the request");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], "qwen2.5:14b", "the chosen model rides the call");
        assert_eq!(v["stream"], false, "non-streaming v1");
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["content"], "hola");
    }

    /// NFR-14: a stopped local runtime is the typed `local_unreachable:`
    /// error with the endpoint in it — never a panic, never a simulated
    /// stand-in. (Bind an ephemeral port, drop the listener: nothing
    /// listens there.)
    #[tokio::test]
    async fn a_stopped_local_runtime_is_the_typed_local_unreachable_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let base = format!("http://{addr}");
        let adapter = Ollama::new(&base).unwrap();
        let err = adapter
            .chat(ChatRequest::new(vec![Message::user("hola")]).with_model("llama3.1:8b"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProviderError::LocalUnreachable { ref url } if url == &base),
            "expected LocalUnreachable, got {err:?}"
        );
        // and the discovery probe reports the same honest reason
        let err = adapter.tags().await.unwrap_err();
        assert!(matches!(err, ProviderError::LocalUnreachable { ref url } if url == &base));
    }
}
