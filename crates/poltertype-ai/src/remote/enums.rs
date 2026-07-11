//! Supported remote LLM providers.

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Ollama,
    Custom,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "anthropic" => Self::Anthropic,
            "openai" => Self::OpenAi,
            "ollama" => Self::Ollama,
            "custom-openai-compatible" | "custom" => Self::Custom,
            _ => return None,
        })
    }
}
