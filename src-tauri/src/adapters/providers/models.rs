// Provider model listing + connection testing (Story 5.7, FR-17.2): the
// Ajustes → IA "test connection" path — one GET against the provider's
// models endpoint with the keychain-stored key (AD-9: it goes through the
// provider layer's own HTTP surface, never a bespoke fetch in the UI). A
// successful test doubles as the live model list for the chat header's
// picker (FR-17.4: "the provider's models or a curated per-provider list").
//
// The explore path (restoring the bible wizard's EXPLORE action) runs the
// same listing on demand per provider: `list_models` — one GET against the
// provider's real models endpoint, sorted + deduped model ids back, every
// refusal typed. The local (Ollama) provider reuses its own `/api/tags`
// discovery — the prototype's shape — with no key.

use serde_json::Value;
use thiserror::Error;

use super::{default_base_url, ProviderError, ProviderSettings};

/// Everything the explore path can refuse with — typed, never a bare
/// string, so the UI can render the honest error chip and tests can match.
#[derive(Debug, Error)]
pub enum ModelsError {
    #[error("provider_not_configured: {0}")]
    NotConfigured(String),
    #[error("models_fetch_failed: provider `{name}` — {reason}")]
    FetchFailed { name: String, reason: String },
}

/// The models endpoint + auth headers for one provider (pure — the tested
/// surface). Anthropic uses `x-api-key` + its version header; Google keys
/// ride the documented `x-goog-api-key` HEADER (never the query string —
/// secrets stay out of the URI); everything else is OpenAI-shaped Bearer
/// auth on `{base}/models`.
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
            let _ = headers.insert("x-goog-api-key", parse_header(&s.api_key));
            format!("{base}/v1beta/models")
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

/// Is this provider name the local (Ollama) shape? The local server is
/// discovered through its own `/api/tags` endpoint — the prototype's
/// EXPLORE shape — not `/models`, and with no key (it is the user's own).
fn is_local(name: &str) -> bool {
    matches!(name.trim(), "local" | "ollama")
}

/// The local discovery base URL (pure): the configured base URL with any
/// trailing `/` and OpenAI-compatible `/v1` suffix stripped, defaulting to
/// the Ollama endpoint — the adapter itself appends `/api/tags`.
fn local_base_url(s: &ProviderSettings) -> String {
    let base = if s.base_url.trim().is_empty() {
        default_base_url("local").unwrap_or_default().to_string()
    } else {
        s.base_url.trim().trim_end_matches('/').to_string()
    };
    base.strip_suffix("/v1").unwrap_or(&base).to_string()
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
/// anything other than a parseable 200 is honestly reported. Thin: the
/// listing itself is `list_models` (the explore path shares it).
pub async fn test_connection(s: &ProviderSettings) -> ConnectionTest {
    match list_models(s).await {
        Ok(models) if models.is_empty() => ConnectionTest {
            ok: false,
            models,
            error: Some(
                "the provider answered but listed no models / el proveedor \
                 respondió sin modelos"
                    .into(),
            ),
        },
        Ok(models) => ConnectionTest { ok: true, models, error: None },
        Err(e) => ConnectionTest { ok: false, models: Vec::new(), error: Some(e.to_string()) },
    }
}

/// List one provider's LIVE models (the explore path): one GET against the
/// provider's real models endpoint — OpenAI / OpenRouter / custom
/// `{base}/models` (Bearer), Anthropic `/v1/models` (`x-api-key`), Google
/// `/v1beta/models` (`x-goog-api-key`, out of the URI), the local server
/// `/api/tags` (no key). Model ids come back sorted + deduped; an honest
/// 200 with no models is an EMPTY list (the picker falls back to the
/// curated list — never invented entries). Never a bare-string refusal:
/// `provider_not_configured:` / `models_fetch_failed:`.
pub async fn list_models(s: &ProviderSettings) -> Result<Vec<String>, ModelsError> {
    let name = s.name.trim().to_string();
    if is_local(&name) {
        return list_local(&name, s).await;
    }
    if s.api_key.trim().is_empty() {
        return Err(ModelsError::NotConfigured(format!(
            "provider `{name}` has no API key stored — save one first / no hay clave guardada"
        )));
    }
    if s.base_url.trim().is_empty() && default_base_url(&name).is_none() {
        return Err(ModelsError::NotConfigured(format!(
            "provider `{name}` requires a base URL / necesita una URL base"
        )));
    }
    let (url, headers) = models_request(s);
    list_via(&name, &url, headers).await
}

/// The local (Ollama) explore listing rides the v0.2 local adapter's own
/// `/api/tags` path (Story 6.1's adapter — AD-9, no duplicated ollama
/// code): the same localhost-only guard (zero egress, FR-24.3) and the
/// same honest `local_unreachable:` mapping. No key — it is the user's
/// own runtime. Names come back as-is, sorted + deduped.
async fn list_local(name: &str, s: &ProviderSettings) -> Result<Vec<String>, ModelsError> {
    let fail = |reason: String| ModelsError::FetchFailed { name: name.to_string(), reason };
    let base = local_base_url(s);
    let adapter = super::ollama::Ollama::new(&base).map_err(|e| fail(e.to_string()))?;
    let mut models: Vec<String> = adapter
        .tags()
        .await
        .map_err(|e| fail(e.to_string()))?
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

/// One GET + parse: sorted, deduped, non-empty model ids — or the typed
/// fetch failure. The response body never carries the key, so neither does
/// the returned list.
async fn list_via(
    name: &str,
    url: &str,
    headers: reqwest::header::HeaderMap,
) -> Result<Vec<String>, ModelsError> {
    let fail = |reason: String| ModelsError::FetchFailed { name: name.to_string(), reason };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| fail(e.to_string()))?;
    let resp = client.get(url).headers(headers).send().await.map_err(|e| {
        fail(ProviderError::Request { name: name.to_string(), source: e }.to_string())
    })?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| fail(e.to_string()))?;
    if !status.is_success() {
        return Err(fail(format!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    let mut models: Vec<String> = parse_models(&body)
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
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
        // google: the key rides the documented header — NEVER the query
        // string (secrets stay out of the URI)
        let (url, headers) = models_request(&settings("google", ""));
        assert_eq!(url, "https://generativelanguage.googleapis.com/v1beta/models");
        assert_eq!(headers.get("x-goog-api-key").unwrap(), "sk-test");
        assert!(!url.contains("sk-test"), "the key must not ride the URI: {url}");
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
        // Google-shaped ("models/" prefix stripped) — also the Ollama
        // /api/tags shape ({models: [{name}]}, no prefix to strip)
        let google = r#"{"models":[{"name":"models/gemini-3-pro"},{"name":"models/gemini-2-5-flash"}]}"#;
        assert_eq!(
            parse_models(google),
            vec!["gemini-3-pro".to_string(), "gemini-2-5-flash".to_string()]
        );
        let ollama = r#"{"models":[{"name":"llama3.1:8b"},{"name":"qwen2.5:14b"}]}"#;
        assert_eq!(
            parse_models(ollama),
            vec!["llama3.1:8b".to_string(), "qwen2.5:14b".to_string()]
        );
        // unparseable / empty stays empty — never invented
        assert!(parse_models("not json").is_empty());
        assert!(parse_models(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn local_discovery_base_strips_v1_and_defaults_to_ollama() {
        // the default local endpoint: the adapter appends /api/tags itself
        assert_eq!(local_base_url(&settings("local", "")), "http://localhost:11434");
        // a configured base URL is honored, trailing / and /v1 stripped
        assert_eq!(
            local_base_url(&settings("local", "http://127.0.0.1:11434/v1/")),
            "http://127.0.0.1:11434"
        );
        // the ollama alias resolves the same discovery shape
        assert_eq!(local_base_url(&settings("ollama", "")), "http://localhost:11434");
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

    /// The explore path's typed refusals: no key and no base URL are
    /// `provider_not_configured:` — never a request, never a bare string.
    #[tokio::test]
    async fn unconfigured_providers_are_typed_refusals_not_requests() {
        let mut s = settings("openai", "");
        s.api_key = "  ".into();
        let err = list_models(&s).await.unwrap_err();
        assert!(err.to_string().starts_with("provider_not_configured:"), "{err}");
        assert!(err.to_string().contains("no API key"));
        // custom with no base URL: not configured either
        let s = settings("custom", "");
        let err = list_models(&s).await.unwrap_err();
        assert!(err.to_string().starts_with("provider_not_configured:"), "{err}");
        assert!(err.to_string().contains("base URL"));
        // and the test-connection wrapper reports the same refusal
        let t = test_connection(&settings("custom", "")).await;
        assert!(!t.ok);
        assert!(t.error.unwrap().starts_with("provider_not_configured:"));
    }

    /// The explore path's BYOK gate: a configured provider's listing rides
    /// the keychain-stored key in the Authorization header — captured here
    /// by a one-shot local axum stand-in — and the returned list (sorted,
    /// deduped) never carries the key.
    #[tokio::test]
    async fn the_listing_carries_the_key_and_comes_back_sorted_and_deduped() {
        use axum::extract::Request;
        use std::sync::{Arc, Mutex};
        const SENTINEL: &str = "sk-explore-BYOK-SENTINEL-8d4a";

        let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen = captured.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(move |req: Request<axum::body::Body>| {
                let seen = seen.clone();
                async move {
                    let (parts, _) = req.into_parts();
                    let auth = parts
                        .headers
                        .get(reqwest::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    *seen.lock().unwrap() = Some(auth);
                    axum::Json(serde_json::json!({
                        "data": [{ "id": "zzz-model" }, { "id": "aaa-model" }, { "id": "aaa-model" }, { "id": "  " }]
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let mut s = settings("custom", &format!("http://{addr}/v1"));
        s.api_key = SENTINEL.into();
        let models = list_models(&s).await.unwrap();
        assert_eq!(
            models,
            vec!["aaa-model".to_string(), "zzz-model".to_string()],
            "sorted, deduped, blanks dropped"
        );
        assert_eq!(
            captured.lock().unwrap().clone().expect("the provider saw the request"),
            format!("Bearer {SENTINEL}"),
            "the listing carries the keychain-stored key"
        );
        // the key never rides the payload
        assert!(!serde_json::to_string(&models).unwrap().contains(SENTINEL));
    }

    /// Failure paths are the typed `models_fetch_failed:` — an HTTP error
    /// status carries its code + body excerpt; a transport that is down
    /// (nothing listening) carries the transport error. Never invented.
    #[tokio::test]
    async fn failed_listings_are_the_typed_fetch_failure() {
        // an HTTP 500 from the provider
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|| async {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom from the provider")
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let err = list_models(&settings("custom", &format!("http://{addr}/v1"))).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("models_fetch_failed:"), "{msg}");
        assert!(msg.contains("HTTP 500"), "{msg}");
        assert!(msg.contains("boom from the provider"), "{msg}");

        // transport down: a port with nothing listening
        let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gone_addr = gone.local_addr().unwrap();
        drop(gone);
        let err = list_models(&settings("custom", &format!("http://{gone_addr}/v1")))
            .await
            .unwrap_err();
        assert!(
            err.to_string().starts_with("models_fetch_failed:"),
            "{}",
            err
        );
    }

    /// The local provider's explore run reuses the /api/tags discovery —
    /// no Authorization header, no key, ollama-shaped names as-is.
    #[tokio::test]
    async fn the_local_listing_reuses_the_tags_discovery_without_a_key() {
        use std::sync::{Arc, Mutex};
        let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen = captured.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/tags",
            axum::routing::get(move |req: axum::extract::Request<axum::body::Body>| {
                let seen = seen.clone();
                async move {
                    let (parts, _) = req.into_parts();
                    let auth = parts
                        .headers
                        .get(reqwest::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    *seen.lock().unwrap() = auth;
                    axum::Json(serde_json::json!({
                        "models": [
                            { "name": "qwen2.5:14b" },
                            { "name": "llama3.1:8b" },
                            { "name": "qwen2.5:14b" }
                        ]
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        // the base URL carries the OpenAI-compatible /v1 — the discovery
        // strips it and hits /api/tags
        let s = settings("local", &format!("http://{addr}/v1"));
        let models = list_models(&s).await.unwrap();
        assert_eq!(
            models,
            vec!["llama3.1:8b".to_string(), "qwen2.5:14b".to_string()],
            "local names as-is, sorted + deduped"
        );
        assert!(
            captured.lock().unwrap().is_none(),
            "the local discovery sends no Authorization header — no key exists"
        );
    }
}
