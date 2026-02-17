//! Ask User tool types — single source of truth for MCP schema, CLI, and TUI.
//!
//! These types carry both `serde` and `schemars` annotations so they can be
//! used directly in MCP tool definitions (schema generation) **and** for
//! runtime (de)serialization in the TUI / CLI.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request (LLM → tool)
// ---------------------------------------------------------------------------

/// Request payload for the `ask_user` tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AskUserRequest {
    #[schemars(
        description = "List of questions to ask the user. Each question has a label, question text, and options."
    )]
    pub questions: Vec<AskUserQuestion>,
}

/// A single question presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AskUserQuestion {
    #[schemars(description = "Short unique label for tab display (max ~15 chars recommended)")]
    pub label: String,
    #[schemars(description = "Full question text to display")]
    pub question: String,
    #[schemars(description = "Predefined answer options")]
    pub options: Vec<AskUserOption>,
    /// Whether to allow custom text input (default: true)
    #[serde(default = "default_true")]
    #[schemars(description = "Whether to allow custom text input (default: true)")]
    pub allow_custom: bool,
    /// Whether this question must be answered (default: true)
    #[serde(default = "default_true")]
    #[schemars(description = "Whether this question must be answered (default: true)")]
    pub required: bool,
}

/// A predefined answer option for a question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AskUserOption {
    #[schemars(description = "Value to return to LLM when selected")]
    pub value: String,
    #[schemars(description = "Display label for the option")]
    pub label: String,
    /// Optional description shown below the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Optional description shown below the label")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Response (tool → LLM)
// ---------------------------------------------------------------------------

/// User's answer to a single question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AskUserAnswer {
    /// Question label this answers.
    pub question_label: String,
    /// Selected option value OR custom text.
    pub answer: String,
    /// Whether this was a custom answer (typed by user).
    pub is_custom: bool,
}

/// Aggregated result of the `ask_user` tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AskUserResult {
    /// All answers provided by the user.
    pub answers: Vec<AskUserAnswer>,
    /// Whether the user completed all questions (false if cancelled).
    pub completed: bool,
    /// Reason for incompletion (if cancelled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_question_serialization() {
        let question = AskUserQuestion {
            label: "Environment".to_string(),
            question: "Which environment should I deploy to?".to_string(),
            options: vec![
                AskUserOption {
                    value: "dev".to_string(),
                    label: "Development".to_string(),
                    description: Some("For testing changes".to_string()),
                },
                AskUserOption {
                    value: "prod".to_string(),
                    label: "Production".to_string(),
                    description: None,
                },
            ],
            allow_custom: true,
            required: true,
        };

        let json = serde_json::to_string(&question).unwrap();
        assert!(json.contains("\"label\":\"Environment\""));
        assert!(json.contains("\"value\":\"dev\""));
        assert!(json.contains("\"description\":\"For testing changes\""));
        // description: None should be skipped
        assert!(!json.contains("\"description\":null"));
    }

    #[test]
    fn test_question_deserialization_with_defaults() {
        let json = r#"{
            "label": "Test",
            "question": "Is this a test?",
            "options": []
        }"#;

        let question: AskUserQuestion = serde_json::from_str(json).unwrap();
        assert_eq!(question.label, "Test");
        assert!(question.allow_custom, "allow_custom should default to true");
        assert!(question.required, "required should default to true");
    }

    #[test]
    fn test_question_deserialization_explicit_false() {
        let json = r#"{
            "label": "Test",
            "question": "Is this a test?",
            "options": [],
            "allow_custom": false,
            "required": false
        }"#;

        let question: AskUserQuestion = serde_json::from_str(json).unwrap();
        assert!(!question.allow_custom);
        assert!(!question.required);
    }

    #[test]
    fn test_answer_serialization() {
        let answer = AskUserAnswer {
            question_label: "Environment".to_string(),
            answer: "production".to_string(),
            is_custom: false,
        };

        let json = serde_json::to_string(&answer).unwrap();
        assert!(json.contains("\"question_label\":\"Environment\""));
        assert!(json.contains("\"answer\":\"production\""));
        assert!(json.contains("\"is_custom\":false"));
    }

    #[test]
    fn test_answer_custom_input() {
        let answer = AskUserAnswer {
            question_label: "Feedback".to_string(),
            answer: "User typed this custom response".to_string(),
            is_custom: true,
        };

        let json = serde_json::to_string(&answer).unwrap();
        assert!(json.contains("\"is_custom\":true"));
        assert!(json.contains("User typed this custom response"));
    }

    #[test]
    fn test_result_completed() {
        let result = AskUserResult {
            answers: vec![
                AskUserAnswer {
                    question_label: "q1".to_string(),
                    answer: "a1".to_string(),
                    is_custom: false,
                },
                AskUserAnswer {
                    question_label: "q2".to_string(),
                    answer: "custom answer".to_string(),
                    is_custom: true,
                },
            ],
            completed: true,
            reason: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"completed\":true"));
        // reason: None should be skipped
        assert!(!json.contains("\"reason\""));
        assert!(json.contains("\"question_label\":\"q1\""));
        assert!(json.contains("\"question_label\":\"q2\""));
    }

    #[test]
    fn test_result_cancelled() {
        let result = AskUserResult {
            answers: vec![],
            completed: false,
            reason: Some("User cancelled the question prompt.".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"completed\":false"));
        assert!(json.contains("\"reason\":\"User cancelled the question prompt.\""));
        assert!(json.contains("\"answers\":[]"));
    }

    #[test]
    fn test_result_deserialization() {
        let json = r#"{
            "answers": [
                {"question_label": "env", "answer": "dev", "is_custom": false}
            ],
            "completed": true
        }"#;

        let result: AskUserResult = serde_json::from_str(json).unwrap();
        assert!(result.completed);
        assert!(result.reason.is_none());
        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.answers[0].question_label, "env");
        assert_eq!(result.answers[0].answer, "dev");
        assert!(!result.answers[0].is_custom);
    }

    #[test]
    fn test_option_without_description() {
        let option = AskUserOption {
            value: "yes".to_string(),
            label: "Yes".to_string(),
            description: None,
        };

        let json = serde_json::to_string(&option).unwrap();
        // description should be omitted entirely when None
        assert!(!json.contains("description"));
        assert!(json.contains("\"value\":\"yes\""));
        assert!(json.contains("\"label\":\"Yes\""));
    }

    #[test]
    fn test_unicode_handling() {
        let question = AskUserQuestion {
            label: "言語".to_string(),
            question: "どの言語を使用しますか？".to_string(),
            options: vec![
                AskUserOption {
                    value: "ja".to_string(),
                    label: "日本語".to_string(),
                    description: Some("Japanese language".to_string()),
                },
                AskUserOption {
                    value: "emoji".to_string(),
                    label: "🚀 Rocket".to_string(),
                    description: Some("With emoji 🎉".to_string()),
                },
            ],
            allow_custom: true,
            required: true,
        };

        let json = serde_json::to_string(&question).unwrap();
        let parsed: AskUserQuestion = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.label, "言語");
        assert_eq!(parsed.question, "どの言語を使用しますか？");
        assert_eq!(parsed.options[0].label, "日本語");
        assert_eq!(parsed.options[1].label, "🚀 Rocket");
    }

    #[test]
    fn test_types_equality() {
        let q1 = AskUserQuestion {
            label: "Test".to_string(),
            question: "Question?".to_string(),
            options: vec![],
            allow_custom: true,
            required: true,
        };

        let q2 = q1.clone();
        assert_eq!(q1, q2);

        let a1 = AskUserAnswer {
            question_label: "Test".to_string(),
            answer: "answer".to_string(),
            is_custom: false,
        };

        let a2 = a1.clone();
        assert_eq!(a1, a2);

        let r1 = AskUserResult {
            answers: vec![a1],
            completed: true,
            reason: None,
        };

        let r2 = r1.clone();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_request_round_trip() {
        let request = AskUserRequest {
            questions: vec![AskUserQuestion {
                label: "Env".to_string(),
                question: "Which env?".to_string(),
                options: vec![AskUserOption {
                    value: "dev".to_string(),
                    label: "Dev".to_string(),
                    description: None,
                }],
                allow_custom: false,
                required: true,
            }],
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: AskUserRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, parsed);
    }
}
