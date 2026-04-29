use crate::{
    chat::{
        stream::chat_once_streaming,
        tools::{execute_tool, tool_defs},
        usage::TurnUsage,
        wire::{ChatRequest, Message, StreamOptions},
    },
    model::{CHAT_MODEL, NUM_CTX},
};
use anyhow::Result;
use reqwest::Client;
use serde_json::json;

pub async fn agent_turn(client: &Client, messages: &mut Vec<Message>) -> Result<()> {
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

        let streaming_result = chat_once_streaming(client, &req).await?;
        if let Some(usage) = streaming_result.usage {
            turn_usage.record(usage);
        }
        let msg = streaming_result.message;
        messages.push(msg.clone());

        let calls = msg.tool_calls.unwrap_or_default();

        println!();

        if calls.is_empty() {
            println!("{}", turn_usage.format_summary(NUM_CTX));
            return Ok(());
        }

        for call in calls {
            println!("\t→ {}({})", call.function.name, call.function.arguments);
            let result =
                match execute_tool(client, &call.function.name, &call.function.arguments).await {
                    Ok(s) => s,
                    Err(e) => serde_json::to_string(&json!({ "error": e.to_string() })).unwrap(),
                };
            let preview: String = result.chars().take(200).collect();
            println!(
                "\t← {}{}",
                preview,
                if result.len() > 200 { "…" } else { "" }
            );

            messages.push(Message {
                role: "tool".to_string(),
                content: Some(result),
                tool_calls: None,
                tool_call_id: Some(call.id),
            })
        }
    }
}
