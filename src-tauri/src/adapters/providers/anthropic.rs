// Anthropic Messages API adapter (BYOK registry, AD-9). Keys are set via the
// `x-api-key` header; the system prompt travels as the top-level `system`
// parameter per the Messages API shape.

use serde_json::{json, Value};

use super::{ChatRequest, ChatResponse, ProviderClient, ProviderError, Usage};

pub struct Anthropic {
    api_key: String,
}

impl Anthropic {
    pub fn new(api_key: &str) -> Self {
        Self { api_key: api_key.to_string() }
    }
}

impl ProviderClient for Anthropic {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn chat(
        &self,
        req: ChatRequest,
    ) -> super::BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            // system messages become the `system` parameter; the rest is the
            // conversation (Anthropic accepts user/assistant turns only).
            let mut system = String::new();
            let mut messages: Vec<Value> = Vec::new();
            for m in &req.messages {
                match m.role.as_str() {
                    "system" => {
                        if !system.is_empty() {
                            system.push('\n');
                        }
                        system.push_str(&m.content);
                    }
                    role => messages.push(json!({ "role": role, "content": m.content })),
                }
            }
            let mut body = json!({
                "model": req.model,
                "max_tokens": 4096,
                "messages": messages,
                "temperature": req.temperature,
            });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            let url = "https://api.anthropic.com/v1/messages";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| ProviderError::Request { name: "anthropic".into(), source: e })?;
            let resp = client
                .post(url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::Request { name: "anthropic".into(), source: e })?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ProviderError::Api {
                    name: "anthropic".into(),
                    status,
                    body: ProviderError::api_body(body),
                });
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Request { name: "anthropic".into(), source: e })?;
            // content is a list of blocks; join the text ones
            let content = v["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .concat()
                })
                .unwrap_or_default();
            if content.trim().is_empty() {
                return Err(ProviderError::EmptyCompletion { name: "anthropic".into() });
            }
            let usage = Usage {
                input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
                output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
            };
            Ok(ChatResponse { content, usage })
        })
    }
}
