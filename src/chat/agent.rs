use crate::{
    chat::{
        event::ChatEvent,
        stream::chat_once_streaming,
        tools::{execute_tool, tool_defs},
        usage::TurnUsage,
        wire::{ChatRequest, Message, StreamOptions},
    },
    model::CHAT_MODEL,
};
use anyhow::Result;
use reqwest::Client;
use serde_json::json;

pub async fn agent_turn<F>(client: &Client, messages: &mut Vec<Message>, mut emit: F) -> Result<()>
where
    F: FnMut(ChatEvent) -> Result<()>,
{
    let tools = tool_defs();
    let mut turn_usage = TurnUsage::default();

    loop {
        let req = ChatRequest {
            model: CHAT_MODEL,
            messages,
            tools: &tools,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let streaming_result = chat_once_streaming(client, &req, &mut emit).await?;
        if let Some(usage) = streaming_result.usage {
            turn_usage.record(usage);
        }
        let msg = streaming_result.message;
        messages.push(msg.clone());

        let calls = msg.tool_calls.unwrap_or_default();

        emit(ChatEvent::Newline)?;

        if calls.is_empty() {
            emit(ChatEvent::Usage(turn_usage.clone()))?;
            emit(ChatEvent::Done)?;
            return Ok(());
        }

        for call in calls {
            emit(ChatEvent::ToolCall {
                name: call.function.name.clone(),
                arguments: call.function.arguments.clone(),
            })?;
            let result =
                match execute_tool(client, &call.function.name, &call.function.arguments).await {
                    Ok(s) => s,
                    Err(e) => serde_json::to_string(&json!({ "error": e.to_string() })).unwrap(),
                };

            let preview: String = result.chars().take(200).collect();
            let truncated = result.chars().count() > 200;

            emit(ChatEvent::ToolResult { preview, truncated })?;

            messages.push(Message {
                role: "tool".to_string(),
                content: Some(result),
                tool_calls: None,
                tool_call_id: Some(call.id),
            })
        }
    }
}
