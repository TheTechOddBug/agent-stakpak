use serde::{Deserialize, Serialize};

/// Shared API limits for caller-provided context payloads.
pub const MAX_CALLER_CONTEXT_ITEMS: usize = 32;
pub const MAX_CALLER_CONTEXT_NAME_CHARS: usize = 256;
pub const MAX_CALLER_CONTEXT_CONTENT_CHARS: usize = 50_000;

/// Structured caller-provided context injected into server session runs.
///
/// Used by HTTP clients (gateway/watch) and server request parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallerContextInput {
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}
