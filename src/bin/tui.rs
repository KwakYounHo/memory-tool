use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use memory_tool::{
    chat::{agent::agent_turn, event::ChatEvent, wire::Message},
    model::NUM_CTX,
};
use ratatui::{
    Terminal,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use reqwest::Client;
use std::{io, time::Duration};
use tokio::sync::mpsc;

#[derive(Default)]
struct App {
    input: String,
    lines: Vec<String>,
    streaming_line: String,
    in_flight: bool,
}

impl App {
    fn submit(&mut self) -> Option<String> {
        let input = self.input.trim().to_string();
        self.input.clear();

        if input.is_empty() {
            return None;
        }

        if input == "/exit" {
            return Some(input);
        }

        self.lines.push(format!("> {input}"));
        Some(input)
    }

    fn apply_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::ReasoningDelta(text) | ChatEvent::ContentDelta(text) => {
                if self.streaming_line.is_empty() {
                    self.lines.push(String::new());
                }

                self.streaming_line.push_str(&text);

                if let Some(last) = self.lines.last_mut() {
                    *last = self.streaming_line.clone();
                }
            }
            ChatEvent::ToolCall { name, arguments } => {
                self.lines.push(format!("→ {name}({arguments})"));
            }
            ChatEvent::ToolResult { preview, truncated } => {
                self.lines
                    .push(format!("← {}{}", preview, if truncated { "…" } else { "" }));
            }
            ChatEvent::Usage(usage) => {
                self.lines.push(usage.format_summary(NUM_CTX));
            }
            ChatEvent::Newline => {
                if !self.streaming_line.is_empty() {
                    self.streaming_line.clear();
                }
            }
            ChatEvent::Done => {
                self.in_flight = false;
                self.streaming_line.clear();
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    let client = Client::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
    let mut app = App::default();

    loop {
        while let Ok(event) = rx.try_recv() {
            app.apply_event(event);
        }

        terminal.draw(|frame| {
            let area = frame.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ])
                .split(area);

            let transcript_text = app.lines.join("\n");

            let transcript = Paragraph::new(transcript_text)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title("Chat"));

            let input_box = Paragraph::new(app.input.as_str())
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL).title("Input"));

            let status = Paragraph::new("/exit, Ctrl-C, Ctrl-D: quit");

            frame.render_widget(transcript, chunks[0]);
            frame.render_widget(input_box, chunks[1]);
            frame.render_widget(status, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Enter if !app.in_flight => {
                        if let Some(input) = app.submit() {
                            if input == "/exit" {
                                break;
                            }

                            app.in_flight = true;

                            let client = client.clone();
                            let tx = tx.clone();

                            tokio::spawn(async move {
                                let mut messages = vec![Message {
                                    role: "user".to_string(),
                                    content: Some(input),
                                    tool_calls: None,
                                    tool_call_id: None,
                                }];

                                let result = agent_turn(&client, &mut messages, |event| {
                                    tx.send(event)?;
                                    Ok(())
                                })
                                .await;

                                if let Err(error) = result {
                                    let _ = tx.send(ChatEvent::ToolResult {
                                        preview: format!("error: {error:#}"),
                                        truncated: false,
                                    });
                                    let _ = tx.send(ChatEvent::Done);
                                }
                            });
                        }
                    }
                    KeyCode::Backspace if !app.in_flight => {
                        app.input.pop();
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char(ch) if !app.in_flight => {
                        app.input.push(ch);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
