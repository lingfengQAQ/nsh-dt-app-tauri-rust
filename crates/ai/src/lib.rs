use async_stream::stream;
use futures_util::{Stream, StreamExt};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{pin::Pin, time::Duration};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] nsh_core::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("unsupported provider `{0}`")]
    UnsupportedProvider(String),
    #[error("missing API key")]
    MissingApiKey,
    #[error("API request failed with status {status}: {body}")]
    ApiStatus {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("API response error: {0}")]
    ApiResponse(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    DeepSeek,
    Volcengine,
    Gemini,
    SiliconFlow,
    Custom,
}

impl Provider {
    pub fn parse(value: impl AsRef<str>) -> Option<Self> {
        let normalized = normalize_provider(value.as_ref());
        match normalized.as_str() {
            "openai" => Some(Self::OpenAI),
            "deepseek" => Some(Self::DeepSeek),
            "volcengine" | "volc" | "ark" | "doubao" | "huoshan" | "volcanicark" => {
                Some(Self::Volcengine)
            }
            "gemini" | "google" | "googleai" => Some(Self::Gemini),
            "siliconflow" | "silicon" | "guiji" => Some(Self::SiliconFlow),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatClientConfig {
    pub provider: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub timeout: Duration,
}

impl ChatClientConfig {
    pub fn from_ai_config(config: &nsh_core::AIConfig) -> Self {
        Self {
            provider: config.provider.clone(),
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            timeout: Duration::from_secs(config.timeout_secs),
        }
    }
}

impl Default for ChatClientConfig {
    fn default() -> Self {
        Self::from_ai_config(&nsh_core::AIConfig::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatStreamEvent {
    Delta(String),
    Done,
    Error(String),
}

pub type ChatEventStream = Pin<Box<dyn Stream<Item = ChatStreamEvent> + Send>>;

#[derive(Debug, Clone)]
pub struct ChatClient {
    http: Client,
    config: ChatClientConfig,
}

impl ChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self> {
        let http = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { http, config })
    }

    pub fn config(&self) -> &ChatClientConfig {
        &self.config
    }

    pub fn base_url(&self) -> Result<String> {
        resolve_base_url(&self.config.provider, self.config.base_url.as_deref())
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let api_key = self.config.api_key.as_deref().ok_or(Error::MissingApiKey)?;
        let url = endpoint_url(&self.base_url()?, "models")?;
        let response = self
            .http
            .get(url)
            .bearer_auth(api_key)
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity")
            .send()
            .await?;
        let status = response.status();
        let body = response_text(response).await?;
        if !status.is_success() {
            return Err(Error::ApiStatus { status, body });
        }

        let json = parse_json_body(&body)?;
        Ok(parse_models_json(&json))
    }

    pub async fn chat_once(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let api_key = self.config.api_key.as_deref().ok_or(Error::MissingApiKey)?;
        let url = endpoint_url(&self.base_url()?, "chat/completions")?;
        let options = ChatRequestOptions::resolve(
            &self.config.provider,
            self.config.base_url.as_deref(),
            &self.config.model,
        );
        let mut payload = serde_json::json!({
            "model": self.config.model,
            "messages": chat_payload_messages(options.use_volcengine_content_parts, messages),
            "stream": false,
        });
        apply_chat_options(
            &mut payload,
            &options,
            self.config.max_tokens,
            self.config.temperature,
        );

        let response = self
            .http
            .post(url)
            .bearer_auth(api_key)
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity")
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response_text(response).await?;
        if !status.is_success() {
            return Err(Error::ApiStatus { status, body });
        }

        let json = parse_json_body(&body)?;
        if let Some(error) = parse_api_error(&json) {
            return Err(Error::ApiResponse(error));
        }
        parse_chat_completion_json(&json).ok_or_else(|| {
            Error::ApiResponse(format!(
                "missing answer in response: {}",
                body_preview(&body)
            ))
        })
    }

    pub fn chat_stream(&self, messages: Vec<ChatMessage>) -> ChatEventStream {
        let client = self.clone();
        Box::pin(stream! {
            let api_key = match client.config.api_key.as_deref() {
                Some(api_key) => api_key.to_string(),
                None => {
                    yield ChatStreamEvent::Error(Error::MissingApiKey.to_string());
                    return;
                }
            };

            let url = match client.base_url().and_then(|base_url| endpoint_url(&base_url, "chat/completions")) {
                Ok(url) => url,
                Err(error) => {
                    yield ChatStreamEvent::Error(error.to_string());
                    return;
                }
            };

            let options = ChatRequestOptions::resolve(
                &client.config.provider,
                client.config.base_url.as_deref(),
                &client.config.model,
            );
            let mut payload = serde_json::json!({
                "model": client.config.model,
                "messages": chat_payload_messages(options.use_volcengine_content_parts, messages),
                "stream": true,
            });
            apply_chat_options(
                &mut payload,
                &options,
                client.config.max_tokens,
                client.config.temperature,
            );

            let response = match client
                .http
                .post(url)
                .bearer_auth(api_key)
                .header(header::ACCEPT, "text/event-stream")
                .header(header::ACCEPT_ENCODING, "identity")
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    yield ChatStreamEvent::Error(error.to_string());
                    return;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let body = response_text(response).await.unwrap_or_else(|error| error.to_string());
                yield ChatStreamEvent::Error(Error::ApiStatus { status, body }.to_string());
                return;
            }

            let mut chunks = response.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = chunks.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield ChatStreamEvent::Error(error.to_string());
                        return;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(newline) = buffer.find('\n') {
                    let line = buffer[..newline].trim_end_matches('\r').to_string();
                    buffer.drain(..=newline);
                    if let Some(event) = parse_sse_line(&line) {
                        let is_done = matches!(event, ChatStreamEvent::Done);
                        yield event;
                        if is_done {
                            return;
                        }
                    }
                }
            }

            if !buffer.trim().is_empty() {
                if let Some(event) = parse_sse_line(buffer.trim()) {
                    let is_done = matches!(event, ChatStreamEvent::Done);
                    yield event;
                    if is_done {
                        return;
                    }
                }
            }

            yield ChatStreamEvent::Done;
        })
    }
}

pub fn default_base_url(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::OpenAI => Some("https://api.openai.com/v1"),
        Provider::DeepSeek => Some("https://api.deepseek.com/v1"),
        Provider::Volcengine => Some("https://ark.cn-beijing.volces.com/api/v3"),
        Provider::Gemini => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        Provider::SiliconFlow => Some("https://api.siliconflow.cn/v1"),
        Provider::Custom => None,
    }
}

pub fn resolve_base_url(
    provider: impl AsRef<str>,
    custom_base_url: Option<&str>,
) -> Result<String> {
    if let Some(custom_base_url) = custom_base_url.filter(|url| !url.trim().is_empty()) {
        return Ok(trim_trailing_slash(custom_base_url));
    }

    let provider_name = provider.as_ref();
    let provider = Provider::parse(provider_name)
        .ok_or_else(|| Error::UnsupportedProvider(provider_name.to_string()))?;
    default_base_url(provider)
        .map(str::to_string)
        .ok_or_else(|| Error::UnsupportedProvider(provider_name.to_string()))
}

fn apply_chat_options(
    payload: &mut Value,
    options: &ChatRequestOptions,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
) {
    if let Some(max_tokens) = max_tokens {
        let key = if options.use_completion_token_cap {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        payload[key] = serde_json::json!(max_tokens);
    }
    if let Some(temperature) = temperature {
        payload["temperature"] = serde_json::json!(temperature);
    }
    if options.disable_thinking {
        payload["thinking"] = serde_json::json!({ "type": "disabled" });
    }
    if options.low_reasoning_effort {
        payload["reasoning_effort"] = serde_json::json!("low");
    }
}

fn chat_payload_messages(use_volcengine_content_parts: bool, messages: Vec<ChatMessage>) -> Value {
    Value::Array(
        messages
            .into_iter()
            .map(|message| {
                let content = if use_volcengine_content_parts && message.role == "user" {
                    serde_json::json!([
                        {
                            "type": "text",
                            "text": message.content,
                        }
                    ])
                } else {
                    Value::String(message.content)
                };

                serde_json::json!({
                    "role": message.role,
                    "content": content,
                })
            })
            .collect(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChatRequestOptions {
    use_volcengine_content_parts: bool,
    use_completion_token_cap: bool,
    disable_thinking: bool,
    low_reasoning_effort: bool,
}

impl ChatRequestOptions {
    fn resolve(provider: &str, base_url: Option<&str>, model: &str) -> Self {
        let parsed_provider = Provider::parse(provider);
        let normalized_url = base_url.unwrap_or_default().trim().to_ascii_lowercase();
        let is_volcengine = parsed_provider == Some(Provider::Volcengine)
            || normalized_url.contains("volces.com")
            || normalized_url.contains("volcengine.com");
        let is_deepseek = parsed_provider == Some(Provider::DeepSeek)
            || normalized_url.contains("api.deepseek.com");
        let is_gemini = parsed_provider == Some(Provider::Gemini)
            || normalized_url.contains("generativelanguage.googleapis.com");
        let is_openai =
            parsed_provider == Some(Provider::OpenAI) || normalized_url.contains("api.openai.com");
        let normalized_model = model.trim().to_ascii_lowercase();
        let is_openai_reasoning_model =
            is_openai && uses_openai_reasoning_controls(&normalized_model);

        Self {
            use_volcengine_content_parts: is_volcengine,
            use_completion_token_cap: is_volcengine || is_openai_reasoning_model,
            disable_thinking: is_volcengine || is_deepseek,
            low_reasoning_effort: is_gemini || is_openai_reasoning_model,
        }
    }
}

fn uses_openai_reasoning_controls(normalized_model: &str) -> bool {
    normalized_model.starts_with("gpt-5")
        || normalized_model.starts_with("o1")
        || normalized_model.starts_with("o3")
        || normalized_model.starts_with("o4")
}

async fn response_text(response: reqwest::Response) -> Result<String> {
    response.text().await.map_err(|error| {
        Error::ApiResponse(format!(
            "failed to read response body: {error}. Check the Base URL/model, and if this is a proxy or OpenAI-compatible provider, try disabling response compression."
        ))
    })
}

fn parse_json_body(body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|error| {
        Error::ApiResponse(format!(
            "invalid JSON response: {error}; body: {}",
            body_preview(body)
        ))
    })
}

fn body_preview(body: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 500;
    let preview: String = body.chars().take(MAX_PREVIEW_CHARS).collect();
    if body.chars().count() > MAX_PREVIEW_CHARS {
        format!("{preview}...")
    } else {
        preview
    }
}
pub fn parse_models_json(value: &Value) -> Vec<ModelInfo> {
    let candidates = value
        .get("data")
        .or_else(|| value.get("models"))
        .unwrap_or(value);

    match candidates {
        Value::Array(items) => items.iter().filter_map(parse_model_info).collect(),
        Value::Object(_) => parse_model_info(candidates).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn parse_model_info(value: &Value) -> Option<ModelInfo> {
    match value {
        Value::String(id) => Some(ModelInfo {
            id: id.clone(),
            name: None,
        }),
        Value::Object(map) => {
            let id = map
                .get("id")
                .or_else(|| map.get("model"))
                .or_else(|| map.get("name"))
                .and_then(Value::as_str)?
                .to_string();
            let name = map
                .get("name")
                .or_else(|| map.get("model"))
                .and_then(Value::as_str)
                .filter(|name| *name != id)
                .map(str::to_string);
            Some(ModelInfo { id, name })
        }
        _ => None,
    }
}

fn parse_api_error(value: &Value) -> Option<String> {
    value.get("error").map(|error| {
        error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("unknown API error")
            .to_string()
    })
}

pub fn parse_chat_completion_json(value: &Value) -> Option<String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| {
                    message
                        .get("content")
                        .and_then(value_to_text)
                        .or_else(|| message.get("reasoning_content").and_then(value_to_text))
                })
                .or_else(|| {
                    choice.get("delta").and_then(|delta| {
                        delta
                            .get("content")
                            .and_then(value_to_text)
                            .or_else(|| delta.get("reasoning_content").and_then(value_to_text))
                    })
                })
                .or_else(|| choice.get("text").and_then(value_to_text))
        })
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(ToOwned::to_owned)
}

fn value_to_text(value: &Value) -> Option<&str> {
    if let Some(text) = value.as_str() {
        return Some(text);
    }

    value.as_array().and_then(|items| {
        items.iter().find_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .or_else(|| item.get("content").and_then(Value::as_str))
        })
    })
}
fn parse_sse_line(line: &str) -> Option<ChatStreamEvent> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return None;
    }

    let data = line.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return Some(ChatStreamEvent::Done);
    }

    let value: Value = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(error) => return Some(ChatStreamEvent::Error(error.to_string())),
    };

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_else(|| error.as_str().unwrap_or("unknown stream error"));
        return Some(ChatStreamEvent::Error(message.to_string()));
    }

    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("delta")
                .and_then(|delta| {
                    delta
                        .get("content")
                        .or_else(|| delta.get("reasoning_content"))
                })
                .or_else(|| choice.get("text"))
        })
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
        .map(|content| ChatStreamEvent::Delta(content.to_string()))
}

fn endpoint_url(base_url: &str, path: &str) -> Result<String> {
    let base_url = format!("{}/", trim_trailing_slash(base_url));
    let url = nsh_core::parse_url(&base_url)?
        .join(path)
        .map_err(|source| nsh_core::Error::InvalidUrl {
            value: format!("{base_url}{path}"),
            source,
        })?;
    Ok(url.to_string())
}

fn trim_trailing_slash(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn normalize_provider(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_default_base_urls() {
        assert_eq!(
            resolve_base_url("openai", None).unwrap(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            resolve_base_url("DeepSeek", None).unwrap(),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            resolve_base_url("doubao", None).unwrap(),
            "https://ark.cn-beijing.volces.com/api/v3"
        );
        assert_eq!(
            resolve_base_url("gemini", None).unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert_eq!(
            resolve_base_url("silicon-flow", None).unwrap(),
            "https://api.siliconflow.cn/v1"
        );
    }

    #[test]
    fn custom_base_url_overrides_provider() {
        assert_eq!(
            resolve_base_url("custom", Some("https://example.com/v1/")).unwrap(),
            "https://example.com/v1"
        );
    }

    #[test]
    fn parses_openai_data_models() {
        let value = json!({
            "object": "list",
            "data": [
                {"id": "gpt-4o-mini", "object": "model"},
                {"model": "deepseek-chat", "name": "DeepSeek Chat"},
                "plain-model"
            ]
        });

        let models = parse_models_json(&value);

        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "gpt-4o-mini");
        assert_eq!(models[1].id, "deepseek-chat");
        assert_eq!(models[1].name.as_deref(), Some("DeepSeek Chat"));
        assert_eq!(models[2].id, "plain-model");
    }

    #[test]
    fn parses_models_key_and_name_fallback() {
        let value = json!({
            "models": [
                {"name": "named-only"},
                {"id": "id-and-name", "name": "Readable Name"},
                {"ignored": true}
            ]
        });

        let models = parse_models_json(&value);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "named-only");
        assert_eq!(models[0].name, None);
        assert_eq!(models[1].name.as_deref(), Some("Readable Name"));
    }

    #[test]
    fn parses_chat_completion_message() {
        let value = json!({
            "choices": [{"message": {"content": "answer-a"}}]
        });

        assert_eq!(
            parse_chat_completion_json(&value).as_deref(),
            Some("answer-a")
        );
    }

    #[test]
    fn volcengine_payload_uses_text_content_parts() {
        let messages = chat_payload_messages(true, vec![ChatMessage::user("hello")]);

        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn volcengine_payload_disables_thinking_and_uses_completion_cap() {
        let mut payload = serde_json::json!({
            "model": "doubao-test",
            "messages": [],
            "stream": false,
        });

        let options = ChatRequestOptions::resolve("doubao", None, "doubao-test");

        apply_chat_options(&mut payload, &options, Some(16), Some(0.0));

        assert_eq!(payload["thinking"]["type"], "disabled");
        assert_eq!(payload["max_completion_tokens"], 16);
        assert!(payload.get("max_tokens").is_none());
        assert_eq!(payload["temperature"], 0.0);
    }

    #[test]
    fn volcengine_base_url_is_treated_as_volcengine_request() {
        let options = ChatRequestOptions::resolve(
            "custom",
            Some("https://ark.cn-beijing.volces.com/api/v3"),
            "doubao-seed-2-0-lite-260215",
        );

        assert!(options.disable_thinking);
        assert!(options.use_completion_token_cap);
        assert!(options.use_volcengine_content_parts);
    }

    #[test]
    fn deepseek_payload_disables_thinking_without_completion_cap() {
        let mut payload = serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [],
            "stream": false,
        });
        let options = ChatRequestOptions::resolve("deepseek", None, "deepseek-v4-flash");

        apply_chat_options(&mut payload, &options, Some(16), Some(0.0));

        assert_eq!(payload["thinking"]["type"], "disabled");
        assert_eq!(payload["max_tokens"], 16);
        assert!(payload.get("max_completion_tokens").is_none());
    }

    #[test]
    fn gemini_payload_uses_low_reasoning_effort() {
        let mut payload = serde_json::json!({
            "model": "gemini-3-flash-preview",
            "messages": [],
            "stream": false,
        });
        let options = ChatRequestOptions::resolve("gemini", None, "gemini-3-flash-preview");

        apply_chat_options(&mut payload, &options, Some(16), Some(0.0));

        assert_eq!(payload["reasoning_effort"], "low");
        assert_eq!(payload["max_tokens"], 16);
        assert!(payload.get("thinking").is_none());
    }

    #[test]
    fn openai_gpt5_payload_uses_completion_cap_and_low_reasoning() {
        let mut payload = serde_json::json!({
            "model": "gpt-5.4-mini",
            "messages": [],
            "stream": false,
        });
        let options = ChatRequestOptions::resolve("openai", None, "gpt-5.4-mini");

        apply_chat_options(&mut payload, &options, Some(16), Some(0.0));

        assert_eq!(payload["max_completion_tokens"], 16);
        assert_eq!(payload["reasoning_effort"], "low");
        assert!(payload.get("max_tokens").is_none());
    }

    #[test]
    fn parses_stream_delta_done_and_error() {
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#),
            Some(ChatStreamEvent::Delta("hi".to_string()))
        );
        assert_eq!(parse_sse_line("data: [DONE]"), Some(ChatStreamEvent::Done));
        assert_eq!(
            parse_sse_line(r#"data: {"error":{"message":"bad"}}"#),
            Some(ChatStreamEvent::Error("bad".to_string()))
        );
    }
}
