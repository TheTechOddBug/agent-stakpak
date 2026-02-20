use stakpak_api::{AgentProvider, models::ListRuleBook};

/// Capture startup project directory for server-side AGENTS.md/APPS.md discovery.
pub fn startup_project_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

/// Convert remote skills metadata payload into typed server context files.
///
/// The current API shape is `list_rulebooks`, but we treat entries as remote
/// skill descriptors in the runtime context pipeline.
pub fn map_remote_skills_to_context_files(
    entries: &[ListRuleBook],
) -> Vec<stakpak_server::ContextFile> {
    entries
        .iter()
        .map(|entry| {
            stakpak_server::ContextFile::new(
                format!("remote_skill:{}", entry.uri),
                format!("stakpak://{}", entry.uri),
                format!(
                    "<remote_skill>\nURI: {}\nDescription: {}\nTags: {}\n</remote_skill>",
                    entry.uri,
                    entry.description,
                    entry.tags.join(", ")
                ),
                stakpak_server::ContextPriority::High,
            )
        })
        .collect()
}

/// Load remote skills context for server sessions.
pub async fn load_remote_skills_context(
    client: &dyn AgentProvider,
) -> Result<Vec<stakpak_server::ContextFile>, String> {
    client
        .list_rulebooks()
        .await
        .map(|entries| map_remote_skills_to_context_files(&entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_api::models::RuleBookVisibility;

    #[test]
    fn maps_remote_skills_payload_to_context_file() {
        let files = map_remote_skills_to_context_files(&[ListRuleBook {
            id: "id_1".to_string(),
            uri: "stakpak://skills/k8s".to_string(),
            description: "Kubernetes ops".to_string(),
            visibility: RuleBookVisibility::Public,
            tags: vec!["kubernetes".to_string(), "ops".to_string()],
            created_at: None,
            updated_at: None,
        }]);

        assert_eq!(files.len(), 1);
        assert!(files[0].name.starts_with("remote_skill:"));
        assert!(files[0].content.contains("<remote_skill>"));
        assert!(files[0].content.contains("Kubernetes ops"));
    }
}
