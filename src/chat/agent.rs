use crate::{
    chat::{
        wire::{Message, ChatRequest, ChatResponse},
        tools::{execute_tool, tool_defs},
    },
    model::CHAT_MODEL,
};
use reqwest::Client;
use anyhow::{Context, Result};
use serde_json::json;

const OLLAMA_CHAT_URL: &str = "http://localhost:11434/v1/chat/completions";

pub async fn agent_turn(client: &Client, messages: &mut Vec<Message>) -> Result<()> {
    let tools = tool_defs();

    loop {
        let req = ChatRequest {
            model: CHAT_MODEL,
            messages,
            tools: &tools,
            stream: false,
        };

        let resp: ChatResponse = client.post(OLLAMA_CHAT_URL)
            .json(&req)
            .send().await?
            .error_for_status()?
            .json().await?;

        let msg = resp.choices.into_iter().next()
            .context("no choices in response")?
            .message;
        messages.push(msg.clone());

        let calls = msg.tool_calls.unwrap_or_default();
        if calls.is_empty() {
            if let Some(content) = msg.content {
                if !content.is_empty() {
                    println!("\n{}\n", content);
                }
            }
            return Ok(());
        }

        for call in calls {
            println!("\t→ {}({})", call.function.name, call.function.arguments);
            let result = match execute_tool(client, &call.function.name, &call.function.arguments).await {
                Ok(s) => s,
                Err(e) => serde_json::to_string(&json!({ "error": e.to_string() })).unwrap(),
            };
            let preview: String = result.chars().take(200).collect();
            println!("\t← {}{}", preview, if result.len() > 200 { "…" } else { "" });

            messages.push(Message {
                role: "tool".to_string(),
                content: Some(result),
                tool_calls: None,
                tool_call_id: Some(call.id),
            })
        }
    }
}

