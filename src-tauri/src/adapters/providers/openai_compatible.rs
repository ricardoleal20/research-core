// OpenAI-compatible chat completions adapter: the registry's workhorse —
// covers openai, openrouter, custom endpoints, and local OpenAI-compatible
// servers (Ollama). The only place outside `mcp.rs` allowed to speak HTTP to
// an LLM provider (AD-9).

use serde_json::{json, Value};

use super::{ChatRequest, ChatResponse, ProviderClient, ProviderError, Usage};

pub struct OpenAiCompatible {
    name: String,
    base_url: String,
    api_key: String,
}

impl OpenAiCompatible {
    /// `name` is the attribution name; `base_url` the chat-completions root
    /// (e.g. https://api.openai.com/v1).
    pub fn new(name: &str, base_url: impl Into<String>, api_key: &str) -> Self {
        Self {
            name: name.to_string(),
            base_url: base_url.into(),
            api_key: api_key.to_string(),
        }
    }
}

impl ProviderClient for OpenAiCompatible {
    fn name(&self) -> &str {
        &self.name
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
            let body = json!({
                "model": req.model,
                "messages": messages,
                "temperature": req.temperature,
            });
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| ProviderError::Request { name: self.name.clone(), source: e })?;
            let resp = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::Request { name: self.name.clone(), source: e })?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ProviderError::Api {
                    name: self.name.clone(),
                    status,
                    body: ProviderError::api_body(body),
                });
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Request { name: self.name.clone(), source: e })?;
            let content = v["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| ProviderError::EmptyCompletion { name: self.name.clone() })?
                .to_string();
            let usage = Usage {
                input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            };
            Ok(ChatResponse { content, usage })
        })
    }
}
