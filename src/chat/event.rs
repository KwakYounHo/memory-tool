use crate::chat::usage::TurnUsage;

#[derive(Debug, Clone)]
pub enum ChatEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolResult {
        preview: String,
        trucated: bool,
    },
    Usage(TurnUsage),
    Done,
}
