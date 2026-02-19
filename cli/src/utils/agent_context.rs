use crate::utils::agents_md::{AgentsMdInfo, format_agents_md_for_context};
use crate::utils::apps_md::{AppsMdInfo, format_apps_md_for_context};
use crate::utils::local_context::LocalContext;
use stakpak_api::models::ListRuleBook;

#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Pre-formatted local context string. Snapshotted once at construction;
    /// does not refresh on subsequent injections (by design — avoids blocking
    /// filesystem walks on every message).
    pub local_context_formatted: Option<String>,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub agents_md: Option<AgentsMdInfo>,
    pub apps_md: Option<AppsMdInfo>,
}

impl AgentContext {
    pub async fn from_parts(
        local_context: Option<LocalContext>,
        rulebooks: Option<Vec<ListRuleBook>>,
        agents_md: Option<AgentsMdInfo>,
        apps_md: Option<AppsMdInfo>,
    ) -> Self {
        let local_context_formatted = if let Some(ref ctx) = local_context {
            ctx.format_display().await.ok()
        } else {
            None
        };

        Self {
            local_context_formatted,
            rulebooks,
            agents_md,
            apps_md,
        }
    }

    pub fn update_rulebooks(&mut self, rulebooks: Option<Vec<ListRuleBook>>) {
        self.rulebooks = rulebooks;
    }

    pub fn enrich_prompt(
        &self,
        user_input: &str,
        is_first_message: bool,
        force_context: bool,
    ) -> String {
        if !is_first_message && !force_context {
            return user_input.to_string();
        }

        let mut result = user_input.to_string();

        if let Some(ref formatted) = self.local_context_formatted {
            result = format!(
                "{}\n<local_context>\n{}\n</local_context>",
                result, formatted
            );
        }

        if let Some(ref rulebooks) = self.rulebooks
            && !rulebooks.is_empty()
        {
            let rulebooks_text = format_rulebooks(rulebooks);
            result = format!("{}\n<rulebooks>\n{}\n</rulebooks>", result, rulebooks_text);
        }

        if is_first_message {
            if let Some(ref agents_md) = self.agents_md {
                let agents_text = format_agents_md_for_context(agents_md);
                result = format!("{}\n<agents_md>\n{}\n</agents_md>", result, agents_text);
            }

            if let Some(ref apps_md) = self.apps_md {
                let apps_text = format_apps_md_for_context(apps_md);
                result = format!("{}\n<apps_md>\n{}\n</apps_md>", result, apps_text);
            }
        }

        result
    }
}

fn format_rulebooks(rulebooks: &[ListRuleBook]) -> String {
    format!(
        "# My Rule Books:\n\n{}",
        rulebooks
            .iter()
            .map(|rulebook| {
                let text = rulebook.to_text();
                let mut lines = text.lines();
                let mut result = String::new();
                if let Some(first) = lines.next() {
                    result.push_str(&format!("  - {}", first));
                    for line in lines {
                        result.push_str(&format!("\n    {}", line));
                    }
                }
                result
            })
            .collect::<Vec<String>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_agents_md() -> AgentsMdInfo {
        AgentsMdInfo {
            content: "## Setup\n- Run tests".to_string(),
            path: PathBuf::from("/project/AGENTS.md"),
        }
    }

    fn make_apps_md() -> AppsMdInfo {
        AppsMdInfo {
            content: "## My App\n- Port 8080".to_string(),
            path: PathBuf::from("/project/APPS.md"),
        }
    }

    fn make_rulebooks() -> Vec<ListRuleBook> {
        vec![ListRuleBook {
            id: "rb_test_001".to_string(),
            uri: "stakpak://test/rulebook.md".to_string(),
            description: "Test rulebook".to_string(),
            visibility: stakpak_api::models::RuleBookVisibility::Public,
            tags: vec!["test".to_string()],
            created_at: None,
            updated_at: None,
        }]
    }

    fn make_context(
        local_context_formatted: Option<&str>,
        rulebooks: Option<Vec<ListRuleBook>>,
        agents_md: Option<AgentsMdInfo>,
        apps_md: Option<AppsMdInfo>,
    ) -> AgentContext {
        AgentContext {
            local_context_formatted: local_context_formatted.map(String::from),
            rulebooks,
            agents_md,
            apps_md,
        }
    }

    #[test]
    fn enrich_prompt_first_message_full_context() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Hello agent", true, false);

        assert!(result.starts_with("Hello agent"));
        assert!(result.contains("<local_context>"));
        assert!(result.contains("Machine: test"));
        assert!(result.contains("<rulebooks>"));
        assert!(result.contains("Test rulebook"));
        assert!(result.contains("<agents_md>"));
        assert!(result.contains("<apps_md>"));
    }

    #[test]
    fn enrich_prompt_not_first_message_returns_unchanged() {
        let ctx = make_context(
            Some("# System Details"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Follow-up question", false, false);
        assert_eq!(result, "Follow-up question");
    }

    #[test]
    fn enrich_prompt_force_context_injects_local_and_rulebooks_only() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Updated question", false, true);

        assert!(result.contains("<local_context>"));
        assert!(result.contains("<rulebooks>"));
        assert!(!result.contains("<agents_md>"));
        assert!(!result.contains("<apps_md>"));
    }

    #[test]
    fn enrich_prompt_empty_context_returns_input_unchanged() {
        let ctx = make_context(None, None, None, None);
        let result = ctx.enrich_prompt("Hello", true, false);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn enrich_prompt_skips_empty_rulebooks_block() {
        let ctx = make_context(None, Some(vec![]), None, None);
        let result = ctx.enrich_prompt("Hello", true, false);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn update_rulebooks_replaces_rulebooks() {
        let mut ctx = make_context(None, None, None, None);
        assert!(ctx.rulebooks.is_none());

        ctx.update_rulebooks(Some(make_rulebooks()));
        assert!(ctx.rulebooks.is_some());
    }
}
