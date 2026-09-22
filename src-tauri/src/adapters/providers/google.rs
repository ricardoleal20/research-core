// Google Gemini adapter (BYOK registry, AD-9): generateContent on the
// Generative Language API. The key travels as the `?key=` query parameter
// per Google's convention; usage comes from `usageMetadata`.

use serde_json::{json, Value};

use super::{ChatRequest, ChatResponse, ProviderClient, ProviderError, Usage};

pub struct Google {
    api_key: String,
}

impl Google {
    pub fn new(api_key: &str) -> Self {
        Self { api_key: api_key.to_string() }
    }
}

impl ProviderClient for Google {
    fn name(&self) -> &str {
        "google"
    }

    fn chat(
        &self,
        req: ChatRequest,
    ) -> super::BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            // system messages become `systemInstruction`; user/assistant map
            // to user/model contents.
            let mut system = String::new();
            let mut contents: Vec<Value> = Vec::new();
            for m in &req.messages {
                match m.role.as_str() {
                    "system" => {
                        if !system.is_empty() {
                            system.push('\n');
                        }
                        system.push_str(&m.content);
                    }
                    "assistant" => contents.push(json!({
                        "role": "model",
                        "parts": [{ "text": m.content }],
                    })),
                    role => contents.push(json!({
                        "role": role,
                        "parts": [{ "text": m.content }],
                    })),
                }
            }
            let mut body = json!({
                "contents": contents,
                "generationConfig": { "temperature": req.temperature },
            });
            if !system.is_empty() {
                body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
            }
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                req.model, self.api_key
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| ProviderError::Request { name: "google".into(), source: e })?;
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::Request { name: "google".into(), source: e })?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ProviderError::Api {
                    name: "google".into(),
                    status,
                    body: ProviderError::api_body(body),
                });
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Request { name: "google".into(), source: e })?;
            let content = v["candidates"][0]["content"]["parts"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p["text"].as_str())
                        .collect::<Vec<_>>()
                        .concat()
                })
                .unwrap_or_default();
            if content.trim().is_empty() {
                return Err(ProviderError::EmptyCompletion { name: "google".into() });
            }
            let usage = Usage {
                input_tokens: v["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0),
                output_tokens: v["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0),
            };
            Ok(ChatResponse { content, usage })
        })
    }
}
