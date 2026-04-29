use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::io::{self, Write};

use crate::chat::{
    usage::TokenUsage,
    wire::{ChatRequest, Message, StreamChunk},
};
use crate::model::OLLAMA_CHAT_URL;

#[derive(Debug)]
pub struct StreamingChatResult {
    pub message: Message,
    pub usage: Option<TokenUsage>,
}

pub async fn chat_once_streaming(
    client: &Client,
    req: &ChatRequest<'_>,
) -> Result<StreamingChatResult> {
    let resp = client
        .post(OLLAMA_CHAT_URL)
        .json(req)
        .send()
        .await?
        .error_for_status()?;

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<ToolCallBuilder> = Vec::new();
    let mut finish_reason: Option<String> = None;
    let mut usage: Option<TokenUsage> = None;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        let text = String::from_utf8_lossy(&bytes);
        buf.push_str(&text);

        while let Some(line_end) = buf.find('\n') {
            let line = buf[..line_end].trim_end_matches('\r').to_string();
            buf.drain(..=line_end);

            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };

            if data == "[DONE]" {
                let built_tool_calls = tool_calls
                    .into_iter()
                    .map(ToolCallBuilder::build)
                    .collect::<Result<Vec<_>>>()?;

                return Ok(StreamingChatResult {
                    message: Message {
                        role: "assistant".to_string(),
                        content: if content.is_empty() {
                            None
                        } else {
                            Some(content)
                        },
                        tool_calls: if built_tool_calls.is_empty() {
                            None
                        } else {
                            Some(built_tool_calls)
                        },
                        tool_call_id: None,
                    },
                    usage,
                });
            }

            let parsed: StreamChunk = serde_json::from_str(data)
                .with_context(|| format!("parse stream chunk: {}", data))?;

            if let Some(u) = parsed.usage {
                usage = Some(TokenUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                });
            }

            let Some(choice) = parsed.choices.into_iter().next() else {
                continue;
            };

            if let Some(reasoning_delta) = choice.delta.reasoning {
                reasoning.push_str(&reasoning_delta);
                print!("{}", reasoning_delta);
                io::stdout().flush()?;
            }

            if let Some(content_delta) = choice.delta.content {
                if content.is_empty() && !content_delta.is_empty() {
                    println!();
                }

                content.push_str(&content_delta);
                print!("{}", content_delta);
                io::stdout().flush()?;
            }

            if let Some(deltas) = choice.delta.tool_calls {
                for delta in deltas {
                    while tool_calls.len() <= delta.index {
                        tool_calls.push(ToolCallBuilder::default());
                    }
                    tool_calls[delta.index].apply_delta(delta);
                }
            }

            if choice.finish_reason.is_some() {
                finish_reason = choice.finish_reason;
            }
        }
    }

    anyhow::bail!(
        "stream ended before [DONE], finish_reason={:?}, reasoning_len={}, content_len={}",
        finish_reason,
        reasoning.len(),
        content.len()
    )
}

#[derive(Debug, Default)]
struct ToolCallBuilder {
    id: String,
    kind: String,
    name: String,
    arguments: String,
}

impl ToolCallBuilder {
    fn apply_delta(&mut self, delta: crate::chat::wire::ToolCallDelta) {
        if let Some(id) = delta.id {
            self.id = id;
        }
        if let Some(kind) = delta.kind {
            self.kind = kind;
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                self.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                self.arguments.push_str(&arguments);
            }
        }
    }

    fn build(self) -> Result<crate::chat::wire::ToolCall> {
        if self.id.is_empty() {
            anyhow::bail!("streamed tool call missing id");
        }
        if self.name.is_empty() {
            anyhow::bail!("streamed tool call missing function name");
        }

        Ok(crate::chat::wire::ToolCall {
            id: self.id,
            kind: if self.kind.is_empty() {
                "function".to_string()
            } else {
                self.kind
            },
            function: crate::chat::wire::FunctionCall {
                name: self.name,
                arguments: self.arguments,
            },
        })
    }
}
