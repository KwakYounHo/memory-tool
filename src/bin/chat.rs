use anyhow::Result;
use memory_tool::{
    chat::{agent::agent_turn, wire::Message},
    model::CHAT_MODEL,
};
use reqwest::Client;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let mut messages: Vec<Message> = Vec::new();
    let stdin = io::stdin();

    println!("Chat with {}. Type 'exit' or Ctrl-D to quit", CHAT_MODEL);
    println!("Tools available: search_memory, add_memory, list_direectory, read_file\n");

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut buffer = String::new();
        if stdin.read_line(&mut buffer)? == 0 {
            println!();
            break;
        }
        let input = buffer.trim();
        if input.is_empty() {
            continue;
        }
        if input == "exit" {
            break;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Some(input.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });

        if let Err(e) = agent_turn(&client, &mut messages).await {
            eprintln!("error: {:#}", e);
        }
    }

    Ok(())
}
