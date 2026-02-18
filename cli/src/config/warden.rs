//! Warden (runtime security) configuration.

use serde::{Deserialize, Serialize};

/// Configuration for the Warden runtime security system.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WardenConfig {
    /// Whether warden is enabled
    pub enabled: bool,
    /// Volume mounts for the warden container
    #[serde(default)]
    pub volumes: Vec<String>,
}

/// Default volume mounts for the stakpak agent container.
///
/// Single source of truth for every path the container needs.
/// Used by `WardenConfig::readonly_profile()` and `prepare_volumes()`.
pub fn stakpak_agent_default_mounts() -> Vec<String> {
    vec![
        // Stakpak config & credentials
        "~/.stakpak/config.toml:/home/agent/.stakpak/config.toml:ro".to_string(),
        "~/.stakpak/auth.toml:/home/agent/.stakpak/auth.toml:ro".to_string(),
        "~/.stakpak/data/local.db:/home/agent/.stakpak/data/local.db".to_string(),
        "~/.agent-board/data.db:/home/agent/.agent-board/data.db".to_string(),
        // Working directory
        "./:/agent:ro".to_string(),
        "./.stakpak:/agent/.stakpak".to_string(),
        // Cloud provider credentials
        "~/.aws:/home/agent/.aws:ro".to_string(),
        "~/.config/gcloud:/home/agent/.config/gcloud:ro".to_string(),
        "~/.digitalocean:/home/agent/.digitalocean:ro".to_string(),
        "~/.azure:/home/agent/.azure:ro".to_string(),
        "~/.kube:/home/agent/.kube:ro".to_string(),
        // Aqua tool cache (named volume — persists downloaded CLIs across runs)
        "stakpak-aqua-cache:/home/agent/.local/share/aquaproj-aqua".to_string(),
    ]
}

impl WardenConfig {
    /// Create a readonly profile configuration for warden.
    pub(crate) fn readonly_profile() -> Self {
        WardenConfig {
            enabled: true,
            volumes: stakpak_agent_default_mounts(),
        }
    }
}
