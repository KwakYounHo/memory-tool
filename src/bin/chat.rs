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
    let stdin = io::stdin();

    println!(
        "Chat with {}. Each prompt runs as a stateless turn. Type 'exit' or Ctrl-D to quit",
        CHAT_MODEL
    );

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

        let mut messages = vec![Message {
            role: "user".to_string(),
            content: Some(input.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }];

        if let Err(e) = agent_turn(&client, &mut messages).await {
            eprintln!("error: {:#}", e);
        }
    }

    Ok(())
}
