// Provider model listing + connection testing (Story 5.7, FR-17.2): the
// Ajustes → IA "test connection" path — one GET against the provider's
// models endpoint with the keychain-stored key (AD-9: it goes through the
// provider layer's own HTTP surface, never a bespoke fetch in the UI). A
// successful test doubles as the live model list for the chat header's
// picker (FR-17.4: "the provider's models or a curated per-provider list").

use serde_json::Value;

use super::{default_base_url, ProviderError, ProviderSettings};

/// The models endpoint + auth headers for one provider (pure — the tested
/// surface). Google keys ride the query string (its documented shape);
/// anthropic uses `x-api-key` + its version header; everything else is
/// OpenAI-shaped Bearer auth on `{base}/models`.
fn models_request(s: &ProviderSettings) -> (String, reqwest::header::HeaderMap) {
    let name = s.name.trim();
    let mut headers = reqwest::header::HeaderMap::new();
    let url = match name {
        "anthropic" => {
            let base = if s.base_url.trim().is_empty() {
                default_base_url("anthropic").unwrap_or_default().to_string()
            } else {
                s.base_url.trim().trim_end_matches('/').to_string()
            };
            let _ = headers.insert("x-api-key", parse_header(&s.api_key));
            let _ = headers.insert("anthropic-version", parse_header("2023-06-01"));
            format!("{base}/v1/models?limit=100")
        }
        "google" => {
            let base = if s.base_url.trim().is_empty() {
                default_base_url("google").unwrap_or_default().to_string()
            } else {
                s.base_url.trim().trim_end_matches('/').to_string()
            };
            format!("{base}/v1beta/models?key={}", s.api_key.trim())
        }
        _ => {
            let base = if s.base_url.trim().is_empty() {
                default_base_url(name).unwrap_or_default().to_string()
            } else {
                s.base_url.trim().trim_end_matches('/').to_string()
            };
            let _ = headers.insert(
                reqwest::header::AUTHORIZATION,
                parse_header(&format!("Bearer {}", s.api_key.trim())),
            );
            format!("{base}/models")
        }
    };
    (url, headers)
}

fn parse_header(v: &str) -> reqwest::header::HeaderValue {
    reqwest::header::HeaderValue::from_str(v)
        .unwrap_or(reqwest::header::HeaderValue::from_static(""))
}

/// Parse a models-listing response body: OpenAI-shaped `{data: [{id}]}`
/// and Google-shaped `{models: [{name: "models/<id>"}]}` both resolve to
/// the plain model id list (pure).
pub fn parse_models(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    if let Some(data) = v.get("data").and_then(Value::as_array) {
        let ids: Vec<String> = data
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        if !ids.is_empty() {
            return ids;
        }
    }
    if let Some(models) = v.get("models").and_then(Value::as_array) {
        return models
            .iter()
            .filter_map(|m| m.get("name").and_then(Value::as_str))
            .map(|n| n.trim_start_matches("models/").to_string())
            .collect();
    }
    Vec::new()
}

/// The result of one test-connection run: ok + the live model list (the
/// picker refreshes from it), or the honest error string.
#[derive(Debug, Clone)]
pub struct ConnectionTest {
    pub ok: bool,
    pub models: Vec<String>,
    pub error: Option<String>,
}

/// Test the LOCAL provider's endpoint (Story 6.1, FR-24.1): `GET /api/tags`
/// through the local adapter's own HTTP surface (AD-9 — no bespoke fetch
/// anywhere else). No API key is required; a non-localhost URL is the typed
/// zero-egress refusal (FR-24.3) and a stopped runtime is the honest
/// unreachable reason (NFR-14) — never a dead spawn, never an invented list.
pub async fn test_local_connection(base_url: &str) -> ConnectionTest {
    let adapter = match super::ollama::Ollama::new(base_url) {
        Ok(a) => a,
        Err(e) => {
            return ConnectionTest { ok: false, models: Vec::new(), error: Some(e.to_string()) }
        }
    };
    match adapter.tags().await {
        Ok(models) => {
            if models.is_empty() {
                ConnectionTest {
                    ok: false,
                    models,
                    error: Some(
                        "the local endpoint answered but lists no models — pull one first \
                         (e.g. `ollama pull llama3.1`) / el endpoint local respondió sin \
                         modelos — descarga uno primero (p. ej. `ollama pull llama3.1`)"
                            .into(),
                    ),
                }
            } else {
                ConnectionTest { ok: true, models, error: None }
            }
        }
        Err(e) => ConnectionTest { ok: false, models: Vec::new(), error: Some(e.to_string()) },
    }
}

/// Test the configured API provider: GET its models endpoint with the
/// keychain-stored key. Never invents a result — a provider that answers
/// anything other than a parseable 200 is honestly reported.
pub async fn test_connection(s: &ProviderSettings) -> ConnectionTest {
    if s.api_key.trim().is_empty() {
        return ConnectionTest {
            ok: false,
            models: Vec::new(),
            error: Some(format!(
                "provider `{}` has no API key stored — save one first / no hay clave guardada",
                s.name.trim()
            )),
        };
    }
    let (url, headers) = models_request(s);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ConnectionTest {
                ok: false,
                models: Vec::new(),
                error: Some(e.to_string()),
            }
        }
    };
    match client.get(&url).headers(headers).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) if status.is_success() => {
                    let models = parse_models(&body);
                    if models.is_empty() {
                        ConnectionTest {
                            ok: false,
                            models,
                            error: Some(
                                "the provider answered but listed no models / el proveedor \
                                 respondió sin modelos"
                                    .into(),
                            ),
                        }
                    } else {
                        ConnectionTest { ok: true, models, error: None }
                    }
                }
                Ok(body) => ConnectionTest {
                    ok: false,
                    models: Vec::new(),
                    error: Some(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>())),
                },
                Err(e) => ConnectionTest {
                    ok: false,
                    models: Vec::new(),
                    error: Some(e.to_string()),
                },
            }
        }
        Err(e) => ConnectionTest {
            ok: false,
            models: Vec::new(),
            error: Some(ProviderError::Request { name: s.name.trim().into(), source: e }.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(name: &str, base_url: &str) -> ProviderSettings {
        ProviderSettings {
            mode: "provider".into(),
            name: name.into(),
            base_url: base_url.into(),
            api_key: "sk-test".into(),
            model: String::new(),
            cli: String::new(),
            cli_model: String::new(),
            local_base_url: String::new(),
        }
    }

    #[test]
    fn models_endpoints_follow_the_provider_shape() {
        let (url, headers) = models_request(&settings("openai", ""));
        assert_eq!(url, "https://api.openai.com/v1/models");
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer sk-test"
        );
        let (url, _) = models_request(&settings("openrouter", ""));
        assert_eq!(url, "https://openrouter.ai/api/v1/models");
        // custom base URLs are honored verbatim
        let (url, _) = models_request(&settings("custom", "https://example.internal/v1/"));
        assert_eq!(url, "https://example.internal/v1/models");
        // anthropic: x-api-key + version header
        let (url, headers) = models_request(&settings("anthropic", ""));
        assert_eq!(url, "https://api.anthropic.com/v1/models?limit=100");
        assert_eq!(headers.get("x-api-key").unwrap(), "sk-test");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        // google: the key rides the documented query-string shape
        let (url, headers) = models_request(&settings("google", ""));
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models?key=sk-test"
        );
        assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());
    }

    #[test]
    fn parse_models_reads_both_provider_shapes() {
        // OpenAI-shaped
        let openai = r#"{"object":"list","data":[{"id":"gpt-5.2"},{"id":"gpt-5-mini"}]}"#;
        assert_eq!(
            parse_models(openai),
            vec!["gpt-5.2".to_string(), "gpt-5-mini".to_string()]
        );
        // Google-shaped ("models/" prefix stripped)
        let google = r#"{"models":[{"name":"models/gemini-3-pro"},{"name":"models/gemini-2-5-flash"}]}"#;
        assert_eq!(
            parse_models(google),
            vec!["gemini-3-pro".to_string(), "gemini-2-5-flash".to_string()]
        );
        // unparseable / empty stays empty — never invented
        assert!(parse_models("not json").is_empty());
        assert!(parse_models(r#"{"data":[]}"#).is_empty());
    }

    #[tokio::test]
    async fn a_missing_key_is_an_honest_refusal_not_a_request() {
        let mut s = settings("openai", "");
        s.api_key = "  ".into();
        let t = test_connection(&s).await;
        assert!(!t.ok);
        assert!(t.error.unwrap().contains("no API key"));
    }

    /// Story 6.1 (FR-24.1/NFR-14): the local connection test speaks
    /// `GET /api/tags` through the adapter — a reachable endpoint lists its
    /// installed models, a stopped runtime reports the honest
    /// `local_unreachable:` reason, and a non-localhost URL is the typed
    /// zero-egress refusal.
    #[tokio::test]
    async fn the_local_connection_test_is_honest_both_ways() {
        // a one-shot fake Ollama listing two models
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/tags",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "models": [{ "name": "llama3.1:8b" }, { "name": "qwen2.5:14b" }]
                }))
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let t = test_local_connection(&format!("http://{addr}")).await;
        assert!(t.ok);
        assert_eq!(t.models, vec!["llama3.1:8b", "qwen2.5:14b"]);
        assert!(t.error.is_none());

        // a stopped runtime (nothing listens) is honestly unreachable
        let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gone_addr = gone.local_addr().unwrap();
        drop(gone);
        let t = test_local_connection(&format!("http://{gone_addr}")).await;
        assert!(!t.ok);
        assert!(t.models.is_empty());
        assert!(
            t.error.unwrap().contains("local_unreachable:"),
            "the honest reason, never a dead spawn"
        );

        // a non-localhost URL never even constructs (zero egress, FR-24.3)
        let t = test_local_connection("http://10.0.0.5:11434").await;
        assert!(!t.ok);
        assert!(t.error.unwrap().contains("local_not_localhost:"));
    }
}
