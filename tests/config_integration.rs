use std::path::PathBuf;

use clap::Parser;
use tempfile::TempDir;

use seval::cli::{Cli, Commands};
use seval::config::{
    save_config, AppConfig, ApprovalMode, AwsConfig, GlobalConfig, ProjectConfig,
    ProjectToolsConfig, ToolsConfig,
};

#[test]
fn global_config_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");

    let config = GlobalConfig {
        aws: AwsConfig {
            profile: Some("prod".to_string()),
            region: Some("us-west-2".to_string()),
            model: Some("claude-v3".to_string()),
        },
        tools: ToolsConfig {
            approval_mode: ApprovalMode::AutoEdit,
            deny_rules: vec!["rm -rf /".to_string()],
            ..ToolsConfig::default()
        },
        ..Default::default()
    };

    save_config(&config, &path).unwrap();
    let loaded: GlobalConfig =
        seval::config::load_config(&path).unwrap().expect("config should exist");
    assert_eq!(config, loaded);
}

#[test]
fn project_config_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("project.toml");

    let config = ProjectConfig {
        aws: Some(AwsConfig {
            profile: Some("dev".to_string()),
            region: None,
            model: None,
        }),
        tools: Some(ProjectToolsConfig {
            approval_mode: Some(ApprovalMode::Yolo),
            deny_rules: None,
        }),
    };

    save_config(&config, &path).unwrap();
    let loaded: ProjectConfig =
        seval::config::load_config(&path).unwrap().expect("config should exist");
    assert_eq!(config, loaded);
}

#[test]
fn load_from_paths_global_only_returns_correct_config() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("global.toml");
    let project_path = dir.path().join("nonexistent.toml");

    let global = GlobalConfig {
        aws: AwsConfig {
            profile: Some("myprofile".to_string()),
            region: Some("eu-west-1".to_string()),
            model: None,
        },
        tools: ToolsConfig {
            approval_mode: ApprovalMode::Plan,
            deny_rules: vec!["dangerous-cmd".to_string()],
            ..ToolsConfig::default()
        },
        ..Default::default()
    };
    save_config(&global, &global_path).unwrap();

    let config = AppConfig::load_from_paths(&global_path, &project_path).unwrap();
    assert_eq!(config.aws.profile.as_deref(), Some("myprofile"));
    assert_eq!(config.aws.region.as_deref(), Some("eu-west-1"));
    assert_eq!(config.tools.approval_mode, ApprovalMode::Plan);
    assert_eq!(config.tools.deny_rules, vec!["dangerous-cmd".to_string()]);
}

#[test]
fn load_from_paths_both_files_shows_project_overrides() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("global.toml");
    let project_path = dir.path().join("project.toml");

    let global = GlobalConfig {
        aws: AwsConfig {
            profile: Some("global-profile".to_string()),
            region: Some("us-east-1".to_string()),
            model: Some("model-a".to_string()),
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
            deny_rules: Some(vec!["project-rule".to_string()]),
        }),
    };
    save_config(&project, &project_path).unwrap();

    let config = AppConfig::load_from_paths(&global_path, &project_path).unwrap();
    assert_eq!(config.aws.profile.as_deref(), Some("project-profile"));
    assert_eq!(config.aws.region.as_deref(), Some("us-east-1")); // falls through
    assert_eq!(config.aws.model.as_deref(), Some("model-a")); // falls through
    assert_eq!(config.tools.approval_mode, ApprovalMode::Yolo);
    assert_eq!(config.tools.deny_rules, vec!["project-rule".to_string()]); // replaced
}

#[test]
fn save_config_creates_parent_directories() {
    let dir = TempDir::new().unwrap();
    let nested_path = dir.path().join("a").join("b").join("config.toml");

    let config = GlobalConfig::default();
    save_config(&config, &nested_path).unwrap();
    assert!(nested_path.exists());
}

#[test]
fn cli_no_args_parses_successfully() {
    let cli = Cli::try_parse_from(["seval"]).unwrap();
    assert!(cli.command.is_none());
    assert!(cli.profile.is_none());
    assert!(cli.region.is_none());
    assert!(cli.model.is_none());
    assert!(cli.approval_mode.is_none());
    assert!(cli.config.is_none());
}

#[test]
fn cli_parses_profile_and_region() {
    let cli =
        Cli::try_parse_from(["seval", "--profile", "myprofile", "--region", "us-west-2"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("myprofile"));
    assert_eq!(cli.region.as_deref(), Some("us-west-2"));
}

#[test]
fn cli_parses_init_subcommand() {
    let cli = Cli::try_parse_from(["seval", "init"]).unwrap();
    match cli.command {
        Some(Commands::Init { force }) => assert!(!force),
        _ => panic!("expected Init command"),
    }
}

#[test]
fn cli_parses_init_with_force() {
    let cli = Cli::try_parse_from(["seval", "init", "--force"]).unwrap();
    match cli.command {
        Some(Commands::Init { force }) => assert!(force),
        _ => panic!("expected Init command with force"),
    }
}

#[test]
fn cli_parses_approval_mode() {
    let cli = Cli::try_parse_from(["seval", "--approval-mode", "yolo"]).unwrap();
    assert_eq!(cli.approval_mode, Some(ApprovalMode::Yolo));
}

#[test]
fn cli_parses_model_flag() {
    let cli = Cli::try_parse_from(["seval", "--model", "anthropic.claude-sonnet-4-20250514"]).unwrap();
    assert_eq!(cli.model.as_deref(), Some("anthropic.claude-sonnet-4-20250514"));
}

#[test]
fn cli_parses_config_path() {
    let cli = Cli::try_parse_from(["seval", "--config", "/tmp/seval.toml"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("/tmp/seval.toml")));
}
