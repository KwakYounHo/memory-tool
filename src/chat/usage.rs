#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TurnUsage {
    pub calls: usize,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub max_prompt_tokens: usize,
    pub max_total_tokens: usize,
}

impl TurnUsage {
    pub fn record(&mut self, usage: TokenUsage) {
        self.calls += 1;
        self.prompt_tokens += usage.prompt_tokens;
        self.completion_tokens += usage.completion_tokens;
        self.total_tokens += usage.total_tokens;
        self.max_prompt_tokens = self.max_prompt_tokens.max(usage.prompt_tokens);
        self.max_total_tokens = self.max_total_tokens.max(usage.total_tokens);
    }

    pub fn format_summary(&self, num_ctx: usize) -> String {
        let prompt_pct = if num_ctx == 0 {
            0.0
        } else {
            self.max_prompt_tokens as f64 / num_ctx as f64 * 100.0
        };

        let total_pct = if num_ctx == 0 {
            0.0
        } else {
            self.max_total_tokens as f64 / num_ctx as f64 * 100.0
        };

        format!(
            "usage: calls={} prompt_sum={} completion_sum={} total_sum={} prompt_peak={}/{} ({:.1}%) total_peak={}/{} ({:.1}%)",
            self.calls,
            self.prompt_tokens,
            self.completion_tokens,
            self.total_tokens,
            self.max_prompt_tokens,
            num_ctx,
            prompt_pct,
            self.max_total_tokens,
            num_ctx,
            total_pct,
        )
    }
}
