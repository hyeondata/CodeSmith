use anyhow::{Context, Result};
use codesmith_core::{AppSettings, ChatMessage};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

pub struct OpenAiClient {
    settings: AppSettings,
    http: reqwest::Client,
}

impl OpenAiClient {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            http: reqwest::Client::new(),
        }
    }

    pub async fn stream_chat(&self, messages: &[ChatMessage]) -> Result<Vec<String>> {
        let url = format!(
            "{}/chat/completions",
            self.settings.llm_base_url.trim_end_matches('/')
        );
        let request = ChatCompletionRequest {
            model: self.settings.llm_model.clone(),
            stream: true,
            messages: messages
                .iter()
                .map(|message| OpenAiMessage {
                    role: message.role.as_openai().to_string(),
                    content: message.content.clone(),
                })
                .collect(),
        };

        let mut builder = self.http.post(url).json(&request);
        if let Some(api_key) = self
            .settings
            .api_key
            .as_deref()
            .filter(|key| !key.is_empty())
        {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder.send().await?.error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut chunks = Vec::new();

        while let Some(bytes) = stream.next().await {
            pending.push_str(std::str::from_utf8(&bytes?)?);
            while let Some(index) = pending.find('\n') {
                let line = pending[..index].trim().to_string();
                pending = pending[index + 1..].to_string();
                if let Some(chunk) = parse_sse_line(&line)? {
                    chunks.push(chunk);
                }
            }
        }
        if !pending.trim().is_empty()
            && let Some(chunk) = parse_sse_line(pending.trim())?
        {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    pub async fn test_connection(&self) -> Result<()> {
        let url = format!(
            "{}/models",
            self.settings.llm_base_url.trim_end_matches('/')
        );
        let response = self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json::<ModelsResponse>()
            .await?;
        if !response
            .data
            .iter()
            .any(|model| model.id == self.settings.llm_model)
        {
            anyhow::bail!(
                "configured model '{}' was not found in local Ollama models",
                self.settings.llm_model
            );
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    stream: bool,
    messages: Vec<OpenAiMessage>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
}

fn parse_sse_line(line: &str) -> Result<Option<String>> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(None);
    }
    let chunk: StreamChunk = serde_json::from_str(data).context("parse streaming chunk")?;
    Ok(chunk
        .choices
        .into_iter()
        .filter_map(|choice| choice.delta.content)
        .next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesmith_core::{AppSettings, ChatMessage, ChatRole};
    use std::path::PathBuf;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_openai_chat_completion_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                         data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                         data: [DONE]\n\n",
                    ),
            )
            .mount(&server)
            .await;

        let client = OpenAiClient::new(AppSettings {
            llm_base_url: format!("{}/v1", server.uri()),
            llm_model: "local-model".to_string(),
            api_key: None,
            default_workspace: PathBuf::from("."),
            command_timeout_secs: 120,
        });

        let chunks = client
            .stream_chat(&[ChatMessage::new(ChatRole::User, "hi".to_string())])
            .await
            .expect("stream should work");

        assert_eq!(chunks.concat(), "hello");
    }

    #[tokio::test]
    async fn test_connection_fails_when_configured_model_is_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"object":"list","data":[{"id":"gemma4:e4b-mlx-bf16","object":"model"}]}"#,
            ))
            .mount(&server)
            .await;

        let client = OpenAiClient::new(AppSettings {
            llm_base_url: format!("{}/v1", server.uri()),
            llm_model: "qwen2.5-coder:7b".to_string(),
            api_key: None,
            default_workspace: PathBuf::from("."),
            command_timeout_secs: 120,
        });

        let error = client
            .test_connection()
            .await
            .expect_err("missing configured model should fail connection test");

        assert!(error.to_string().contains("qwen2.5-coder:7b"));
    }
}
