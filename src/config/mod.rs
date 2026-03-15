//! Configuration module.
//!
//! Handles loading, merging, and validating application configuration from
//! global (~/.seval/config.toml) and project-local (.seval/config.toml) sources.

pub mod defaults;
pub mod types;

pub use types::{
    AppConfig, ApprovalMode, AwsConfig, BedrockConfig, GlobalConfig, OpenRouterConfig,
    ProjectConfig, ProjectToolsConfig, ProviderConfig, ProviderKind, ToolsConfig,
};

use std::fs;
use std::path::{Path, PathBuf};

/// Returns the path to the global configuration file (~/.seval/config.toml).
pub fn global_config_path() -> anyhow::Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    Ok(base.home_dir().join(".seval").join("config.toml"))
}

/// Returns the path to the project-local configuration file (.seval/config.toml).
pub fn project_config_path() -> PathBuf {
    PathBuf::from(".seval").join("config.toml")
}

/// Save a configuration to a TOML file, creating parent directories as needed.
pub fn save_config<T: serde::Serialize>(config: &T, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;

        // On Unix, set restrictive permissions on .seval directories under home.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(global_path) = global_config_path()
                && let Some(global_parent) = global_path.parent()
                && parent == global_parent
            {
                let perms = fs::Permissions::from_mode(0o700);
                let _ = fs::set_permissions(parent, perms);
            }
        }
    }
    let content = toml::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

/// Load a configuration from a TOML file. Returns None if the file doesn't exist.
pub fn load_config<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    let config: T = toml::from_str(&content)?;
    Ok(Some(config))
}

impl AppConfig {
    /// Load configuration from the default global and project paths.
    pub fn load() -> anyhow::Result<Self> {
        let global_path = global_config_path()?;
        let project_path = project_config_path();
        Self::load_from_paths(&global_path, &project_path)
    }

    /// Load configuration from explicit paths (for testing).
    pub fn load_from_paths(global: &Path, project: &Path) -> anyhow::Result<Self> {
        let global_config: Option<GlobalConfig> = load_config(global)?;
        let project_config: Option<ProjectConfig> = load_config(project)?;
        Ok(Self::merge(global_config, project_config))
    }

    /// Merge global and project configurations. Project overrides global where present.
    fn merge(global: Option<GlobalConfig>, project: Option<ProjectConfig>) -> Self {
        let global = global.unwrap_or_default();

        let Some(project) = project else {
            return Self {
                aws: global.aws,
                tools: global.tools,
                provider: global.provider,
                bedrock: global.bedrock,
                openrouter: global.openrouter,
                brave_api_key: global.brave_api_key,
            };
        };

        // Merge AWS config
        let aws = if let Some(proj_aws) = project.aws {
            AwsConfig {
                profile: proj_aws.profile.or(global.aws.profile),
                region: proj_aws.region.or(global.aws.region),
                model: proj_aws.model.or(global.aws.model),
            }
        } else {
            global.aws
        };

        // Merge tools config
        let tools = if let Some(proj_tools) = project.tools {
            ToolsConfig {
                approval_mode: proj_tools
                    .approval_mode
                    .unwrap_or(global.tools.approval_mode),
                deny_rules: proj_tools.deny_rules.unwrap_or(global.tools.deny_rules),
                max_turns: global.tools.max_turns,
            }
        } else {
            global.tools
        };

        Self {
            aws,
            tools,
            provider: global.provider,
            bedrock: global.bedrock,
            openrouter: global.openrouter,
            brave_api_key: global.brave_api_key,
        }
    }

    /// Check if a global configuration file exists.
    pub fn has_global_config() -> bool {
        global_config_path().map(|p| p.exists()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_mode_serializes_to_kebab_case() {
        // TOML requires a table wrapper; test via ToolsConfig which contains ApprovalMode.
        let modes = [
            (ApprovalMode::Default, "default"),
            (ApprovalMode::AutoEdit, "auto-edit"),
            (ApprovalMode::Plan, "plan"),
            (ApprovalMode::Yolo, "yolo"),
        ];
        for (mode, expected_str) in modes {
            let config = ToolsConfig {
                approval_mode: mode,
                deny_rules: vec![],
                ..ToolsConfig::default()
            };
            let serialized = toml::to_string_pretty(&config).unwrap();
            assert!(
                serialized.contains(&format!("approval_mode = \"{expected_str}\"")),
                "expected approval_mode = \"{expected_str}\" in:\n{serialized}"
            );
            let deserialized: ToolsConfig = toml::from_str(&serialized).unwrap();
            assert_eq!(deserialized.approval_mode, mode);
        }
    }

    #[test]
    fn global_config_round_trips_through_toml() {
        let config = GlobalConfig {
            aws: AwsConfig {
                profile: Some("prod".to_string()),
                region: Some("us-east-1".to_string()),
                model: Some("claude-v3".to_string()),
            },
            tools: ToolsConfig {
                approval_mode: ApprovalMode::AutoEdit,
                deny_rules: vec!["rm -rf /".to_string(), "chmod 777 /".to_string()],
                ..ToolsConfig::default()
            },
            provider: ProviderConfig::default(),
            bedrock: BedrockConfig::default(),
            openrouter: OpenRouterConfig::default(),
            brave_api_key: None,
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: GlobalConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn provider_kind_defaults_to_bedrock() {
        let kind = ProviderKind::default();
        assert_eq!(kind, ProviderKind::Bedrock);
    }

    #[test]
    fn provider_kind_serializes_as_kebab_case() {
        let config = ProviderConfig {
            active: ProviderKind::OpenRouter,
            model: None,
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(
            serialized.contains("\"open-router\""),
            "expected open-router in:\n{serialized}"
        );
    }

    #[test]
    fn global_config_with_provider_sections_round_trips() {
        let config = GlobalConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig::default(),
            provider: ProviderConfig {
                active: ProviderKind::OpenRouter,
                model: Some("anthropic/claude-sonnet-4-6".to_string()),
            },
            bedrock: BedrockConfig {
                access_key_id: Some("AKIATEST".to_string()),
                secret_access_key: Some("secret".to_string()),
                region: Some("us-east-1".to_string()),
            },
            openrouter: OpenRouterConfig {
                api_key: Some("sk-or-test".to_string()),
            },
            brave_api_key: None,
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: GlobalConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn project_config_partial_fields_deserializes() {
        let toml_str = r#"
[tools]
approval_mode = "yolo"
"#;
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.tools.unwrap().approval_mode,
            Some(ApprovalMode::Yolo)
        );
        assert!(config.aws.is_none());
    }

    #[test]
    fn merge_project_approval_mode_overrides_global() {
        let global = Some(GlobalConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig {
                approval_mode: ApprovalMode::Default,
                deny_rules: defaults::default_deny_rules(),
                ..ToolsConfig::default()
            },
            ..Default::default()
        });
        let project = Some(ProjectConfig {
            aws: None,
            tools: Some(ProjectToolsConfig {
                approval_mode: Some(ApprovalMode::Yolo),
                deny_rules: None,
            }),
        });
        let merged = AppConfig::merge(global, project);
        assert_eq!(merged.tools.approval_mode, ApprovalMode::Yolo);
    }

    #[test]
    fn merge_project_no_tools_falls_through_to_global() {
        let global = Some(GlobalConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig {
                approval_mode: ApprovalMode::Plan,
                deny_rules: vec!["test-rule".to_string()],
                ..ToolsConfig::default()
            },
            ..Default::default()
        });
        let project = Some(ProjectConfig {
            aws: None,
            tools: None,
        });
        let merged = AppConfig::merge(global, project);
        assert_eq!(merged.tools.approval_mode, ApprovalMode::Plan);
        assert_eq!(merged.tools.deny_rules, vec!["test-rule".to_string()]);
    }

    #[test]
    fn merge_project_deny_rules_replace_global() {
        let global = Some(GlobalConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig {
                approval_mode: ApprovalMode::Default,
                deny_rules: vec!["global-rule".to_string()],
                ..ToolsConfig::default()
            },
            ..Default::default()
        });
        let project = Some(ProjectConfig {
            aws: None,
            tools: Some(ProjectToolsConfig {
                approval_mode: None,
                deny_rules: Some(vec!["project-rule".to_string()]),
            }),
        });
        let merged = AppConfig::merge(global, project);
        assert_eq!(merged.tools.deny_rules, vec!["project-rule".to_string()]);
    }

    #[test]
    fn default_deny_rules_include_dangerous_commands() {
        let rules = defaults::default_deny_rules();
        assert!(rules.iter().any(|r| r == "rm -rf /"), "missing rm -rf /");
        assert!(
            rules.iter().any(|r| r == "chmod 777 /"),
            "missing chmod 777 /"
        );
        assert!(rules.iter().any(|r| r == "mkfs.*"), "missing mkfs.*");
    }

    #[test]
    fn global_config_path_ends_with_seval_config() {
        let path = global_config_path().unwrap();
        assert!(
            path.ends_with(".seval/config.toml"),
            "expected path ending in .seval/config.toml, got {path:?}"
        );
    }

    #[test]
    fn project_config_path_is_relative() {
        let path = project_config_path();
        assert_eq!(path, PathBuf::from(".seval/config.toml"));
    }

    #[test]
    fn load_from_paths_no_files_returns_defaults() {
        let config = AppConfig::load_from_paths(
            Path::new("/nonexistent/global"),
            Path::new("/nonexistent/project"),
        )
        .unwrap();
        assert_eq!(config.aws, AwsConfig::default());
        assert_eq!(config.tools.approval_mode, ApprovalMode::Default);
        assert!(!config.tools.deny_rules.is_empty());
    }

    #[test]
    fn load_from_paths_global_only() {
        let dir = tempfile::tempdir().unwrap();
        let global_path = dir.path().join("global.toml");
        let project_path = dir.path().join("nonexistent.toml");

        let global = GlobalConfig {
            aws: AwsConfig {
                profile: Some("myprofile".to_string()),
                region: None,
                model: None,
            },
            tools: ToolsConfig::default(),
            ..Default::default()
        };
        save_config(&global, &global_path).unwrap();

        let config = AppConfig::load_from_paths(&global_path, &project_path).unwrap();
        assert_eq!(config.aws.profile.as_deref(), Some("myprofile"));
    }

    #[test]
    fn load_from_paths_both_files_merges() {
        let dir = tempfile::tempdir().unwrap();
        let global_path = dir.path().join("global.toml");
        let project_path = dir.path().join("project.toml");

        let global = GlobalConfig {
            aws: AwsConfig {
                profile: Some("global-profile".to_string()),
                region: Some("us-east-1".to_string()),
                model: None,
            },
            tools: ToolsConfig {
                approval_mode: ApprovalMode::Default,
                deny_rules: vec!["global-rule".to_string()],
                ..ToolsConfig::default()
            },
            ..Default::default()
        };
        save_config(&global, &global_path).unwrap();

        let project = ProjectConfig {
            aws: Some(AwsConfig {
                profile: Some("project-profile".to_string()),
                region: None,
                model: None,
            }),
            tools: Some(ProjectToolsConfig {
                approval_mode: Some(ApprovalMode::Yolo),
                deny_rules: None,
            }),
        };
        save_config(&project, &project_path).unwrap();

        let config = AppConfig::load_from_paths(&global_path, &project_path).unwrap();
        assert_eq!(config.aws.profile.as_deref(), Some("project-profile"));
        assert_eq!(config.aws.region.as_deref(), Some("us-east-1"));
        assert_eq!(config.tools.approval_mode, ApprovalMode::Yolo);
        assert_eq!(config.tools.deny_rules, vec!["global-rule".to_string()]);
    }
}
