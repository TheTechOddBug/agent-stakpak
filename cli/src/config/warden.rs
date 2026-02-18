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
        // AWS — config read-only, SSO/STS cache writable for token refresh
        "~/.aws/config:/home/agent/.aws/config:ro".to_string(),
        "~/.aws/credentials:/home/agent/.aws/credentials:ro".to_string(),
        "~/.aws/sso:/home/agent/.aws/sso".to_string(),
        "~/.aws/cli:/home/agent/.aws/cli".to_string(),
        // GCP — credential files read-only, cache/logs/db writable for gcloud to function
        "~/.config/gcloud/active_config:/home/agent/.config/gcloud/active_config:ro".to_string(),
        "~/.config/gcloud/configurations:/home/agent/.config/gcloud/configurations:ro".to_string(),
        "~/.config/gcloud/application_default_credentials.json:/home/agent/.config/gcloud/application_default_credentials.json:ro".to_string(),
        "~/.config/gcloud/credentials.db:/home/agent/.config/gcloud/credentials.db:ro".to_string(),
        "~/.config/gcloud/access_tokens.db:/home/agent/.config/gcloud/access_tokens.db:ro".to_string(),
        "~/.config/gcloud/logs:/home/agent/.config/gcloud/logs".to_string(),
        "~/.config/gcloud/cache:/home/agent/.config/gcloud/cache".to_string(),
        // Azure — config read-only, MSAL token cache and session writable
        "~/.azure/config:/home/agent/.azure/config:ro".to_string(),
        "~/.azure/clouds.config:/home/agent/.azure/clouds.config:ro".to_string(),
        "~/.azure/azureProfile.json:/home/agent/.azure/azureProfile.json:ro".to_string(),
        "~/.azure/msal_token_cache.json:/home/agent/.azure/msal_token_cache.json".to_string(),
        "~/.azure/msal_http_cache.bin:/home/agent/.azure/msal_http_cache.bin".to_string(),
        "~/.azure/logs:/home/agent/.azure/logs".to_string(),
        // DigitalOcean & Kubernetes
        "~/.digitalocean:/home/agent/.digitalocean:ro".to_string(),
        "~/.kube:/home/agent/.kube:ro".to_string(),
        // SSH — config and keys read-only (useful for host aliases and remote connections)
        "~/.ssh:/home/agent/.ssh:ro".to_string(),
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
