use crate::{chat::event::ChatEvent, model::NUM_CTX};

#[derive(Default)]
pub(super) struct App {
    pub(super) input: String,
    pub(super) lines: Vec<String>,
    pub(super) streaming_line: String,
    pub(super) in_flight: bool,
}

impl App {
    pub fn submit(&mut self) -> Option<String> {
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

    pub fn apply_event(&mut self, event: ChatEvent) {
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
