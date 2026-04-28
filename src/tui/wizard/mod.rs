//! Interactive setup wizard component.
//!
//! A multi-step TUI wizard that guides users through initial configuration:
//! 1. Select tool approval mode
//! 2. View and edit deny rules
//! 3. Optionally create project-local config
//!
//! Implements the `Component` trait as an enum-driven state machine.

mod input;
mod rendering;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::widgets::{ListState, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::config::defaults;
use crate::config::{
    ApprovalMode, AwsConfig, BedrockConfig, GlobalConfig, OpenRouterConfig, ProjectConfig,
    ProviderConfig, ProviderKind, ToolsConfig, global_config_path, save_config,
};
use crate::tui::Component;

/// The current step in the wizard flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardStep {
    Provider,
    ApiKey,
    ModelSelect,
    ApprovalMode,
    DenyRules,
    ProjectInit,
    Complete,
}

impl WizardStep {
    /// Advance to the next step.
    pub(super) fn next(self) -> Self {
        match self {
            Self::Provider => Self::ApiKey,
            Self::ApiKey => Self::ModelSelect,
            Self::ModelSelect => Self::ApprovalMode,
            Self::ApprovalMode => Self::DenyRules,
            Self::DenyRules => Self::ProjectInit,
            Self::ProjectInit | Self::Complete => Self::Complete,
        }
    }

    /// Go back to the previous step.
    pub(super) fn prev(self) -> Self {
        match self {
            Self::Provider | Self::ApiKey => Self::Provider,
            Self::ModelSelect => Self::ApiKey,
            Self::ApprovalMode => Self::ModelSelect,
            Self::DenyRules => Self::ApprovalMode,
            Self::ProjectInit => Self::DenyRules,
            Self::Complete => Self::ProjectInit,
        }
    }

    /// Total number of steps.
    pub(super) const TOTAL: u8 = 6;

    /// Step number (1-indexed) for display.
    pub(super) fn number(self) -> u8 {
        match self {
            Self::Provider => 1,
            Self::ApiKey => 2,
            Self::ModelSelect => 3,
            Self::ApprovalMode => 4,
            Self::DenyRules => 5,
            Self::ProjectInit | Self::Complete => 6,
        }
    }

    /// Human-readable label for the current step.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Provider => "AI Provider",
            Self::ApiKey => "API Key",
            Self::ModelSelect => "Model",
            Self::ApprovalMode => "Tool Approval Mode",
            Self::DenyRules => "Deny Rules",
            Self::ProjectInit => "Project Config",
            Self::Complete => "Complete",
        }
    }
}

/// Descriptions for each approval mode shown in the wizard.
pub(super) const MODE_DESCRIPTIONS: [(&str, &str); 4] = [
    ("Plan", "Read-only mode, no tool execution"),
    ("Default", "Ask before write operations (recommended)"),
    (
        "Auto-Edit",
        "Auto-approve file edits, ask for shell commands",
    ),
    (
        "Yolo",
        "Approve everything automatically (use with caution)",
    ),
];

/// Provider options for the wizard.
pub(super) const PROVIDER_OPTIONS: [(&str, &str); 2] = [
    ("Bedrock", "AWS Bedrock (access key + secret key + region)"),
    ("OpenRouter", "OpenRouter API (single API key)"),
];

/// Common model choices per provider.
pub const BEDROCK_MODELS: [(&str, &str); 3] = [
    (
        "us.anthropic.claude-sonnet-4-20250514-v1:0",
        "Claude Sonnet 4 (recommended)",
    ),
    (
        "us.anthropic.claude-opus-4-20250514-v1:0",
        "Claude Opus 4 (highest quality)",
    ),
    (
        "us.anthropic.claude-haiku-4-5-20251001-v1:0",
        "Claude Haiku 4.5 (fastest)",
    ),
];

pub const OPENROUTER_MODELS: [(&str, &str); 12] = [
    (
        "anthropic/claude-sonnet-4-6",
        "Claude Sonnet 4.6 (recommended)",
    ),
    (
        "anthropic/claude-opus-4-6",
        "Claude Opus 4.6 (highest quality)",
    ),
    ("anthropic/claude-haiku-4-5", "Claude Haiku 4.5 (fastest)"),
    ("deepseek/deepseek-v3.2", "DeepSeek V3.2"),
    ("minimax/minimax-m2.5", "MiniMax M2.5"),
    ("moonshotai/kimi-k2.5", "Kimi K2.5"),
    ("z-ai/glm-5", "GLM 5"),
    ("openrouter/hunter-alpha", "Hunter Alpha (1M context, free)"),
    ("openrouter/healer-alpha", "Healer Alpha (free)"),
    (
        "nvidia/nemotron-3-super-120b-a12b:free",
        "Nemotron 3 Super 120B (free)",
    ),
    ("stepfun/step-3.5-flash:free", "Step 3.5 Flash (free)"),
    (
        "qwen/qwen3-next-80b-a3b-instruct:free",
        "Qwen3 Next 80B (free)",
    ),
];

pub const CHATGPT_MODELS: [(&str, &str); 1] = [("gpt-5.5", "GPT-5.5 (default)")];

/// Interactive setup wizard component.
pub struct Wizard {
    pub(super) step: WizardStep,
    // Provider step
    pub(super) provider_state: ListState,
    pub(super) selected_provider: ProviderKind,
    // Credentials step (varies by provider)
    /// For `OpenRouter`: single API key. For Bedrock: currently focused field.
    pub(super) api_key_input: String,
    pub(super) api_key_masked: bool,
    /// Bedrock-specific: access key ID.
    pub(super) bedrock_access_key: String,
    /// Bedrock-specific: secret access key.
    pub(super) bedrock_secret_key: String,
    /// Bedrock-specific: region.
    pub(super) bedrock_region: String,
    /// Which Bedrock credential field is active (0=access, 1=secret, 2=region).
    pub(super) bedrock_field_index: usize,
    // Model step
    pub(super) model_state: ListState,
    pub(super) selected_model: String,
    // Approval step
    pub(super) approval_mode_state: ListState,
    pub(super) selected_mode: ApprovalMode,
    pub(super) deny_rules: Vec<String>,
    pub(super) deny_rule_state: ListState,
    pub(super) editing_deny_rule: Option<String>,
    pub(super) is_adding_rule: bool,
    pub(super) create_project_config: bool,
    pub(super) action_tx: Option<UnboundedSender<Action>>,
}

impl Default for Wizard {
    fn default() -> Self {
        Self::new()
    }
}

impl Wizard {
    /// Create a new wizard with default settings.
    pub fn new() -> Self {
        let mut provider_state = ListState::default();
        provider_state.select(Some(0));

        let mut model_state = ListState::default();
        model_state.select(Some(0));

        let mut approval_mode_state = ListState::default();
        approval_mode_state.select(Some(1)); // Default mode selected

        let mut deny_rule_state = ListState::default();
        let deny_rules = defaults::default_deny_rules();
        if !deny_rules.is_empty() {
            deny_rule_state.select(Some(0));
        }

        Self {
            step: WizardStep::Provider,
            provider_state,
            selected_provider: ProviderKind::Bedrock,
            api_key_input: String::new(),
            api_key_masked: true,
            bedrock_access_key: String::new(),
            bedrock_secret_key: String::new(),
            bedrock_region: "us-east-1".to_string(),
            bedrock_field_index: 0,
            model_state,
            selected_model: BEDROCK_MODELS[0].0.to_string(),
            approval_mode_state,
            selected_mode: ApprovalMode::Default,
            deny_rules,
            deny_rule_state,
            editing_deny_rule: None,
            is_adding_rule: false,
            create_project_config: true,
            action_tx: None,
        }
    }

    /// Save configuration files on wizard completion.
    fn save_configs(&self) -> anyhow::Result<()> {
        let (bedrock, openrouter) = match self.selected_provider {
            ProviderKind::Bedrock => (
                BedrockConfig {
                    access_key_id: Some(self.bedrock_access_key.clone()),
                    secret_access_key: Some(self.bedrock_secret_key.clone()),
                    region: Some(self.bedrock_region.clone()),
                },
                OpenRouterConfig::default(),
            ),
            ProviderKind::OpenRouter => (
                BedrockConfig::default(),
                OpenRouterConfig {
                    api_key: Some(self.api_key_input.clone()),
                },
            ),
            ProviderKind::ChatGpt => (BedrockConfig::default(), OpenRouterConfig::default()),
        };

        // Build and save global config.
        let global = GlobalConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig {
                approval_mode: self.selected_mode,
                deny_rules: self.deny_rules.clone(),
                ..ToolsConfig::default()
            },
            provider: ProviderConfig {
                active: self.selected_provider,
                model: Some(self.selected_model.clone()),
            },
            bedrock,
            openrouter,
            brave_api_key: None,
        };
        let global_path = global_config_path()?;
        save_config(&global, &global_path)?;
        tracing::info!("Saved global config to {:?}", global_path);

        // Optionally create project-local config.
        if self.create_project_config {
            let project = ProjectConfig {
                aws: None,
                tools: None,
            };
            let project_path = crate::config::project_config_path();
            if let Some(parent) = project_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            save_config(&project, &project_path)?;
            tracing::info!("Saved project config to {:?}", project_path);
        }

        Ok(())
    }

    /// Map a list index to an `ApprovalMode` variant.
    pub(super) fn mode_from_index(index: usize) -> ApprovalMode {
        match index {
            0 => ApprovalMode::Plan,
            2 => ApprovalMode::AutoEdit,
            3 => ApprovalMode::Yolo,
            // 1 and any out-of-range index default to Default mode.
            _ => ApprovalMode::Default,
        }
    }

    /// Get model list for the currently selected provider.
    pub(super) fn models_for_provider(&self) -> &[(&str, &str)] {
        match self.selected_provider {
            ProviderKind::Bedrock => &BEDROCK_MODELS,
            ProviderKind::OpenRouter => &OPENROUTER_MODELS,
            ProviderKind::ChatGpt => &CHATGPT_MODELS,
        }
    }
}

impl Component for Wizard {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // Global Ctrl+C handling.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Some(Action::Quit));
        }

        Ok(match self.step {
            WizardStep::Provider => self.handle_provider_key(key),
            WizardStep::ApiKey => self.handle_api_key_key(key),
            WizardStep::ModelSelect => self.handle_model_select_key(key),
            WizardStep::ApprovalMode => self.handle_approval_mode_key(key),
            WizardStep::DenyRules => self.handle_deny_rules_key(key),
            WizardStep::ProjectInit => self.handle_project_init_key(key),
            WizardStep::Complete => None,
        })
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::WizardNext => {
                if self.step == WizardStep::ProjectInit {
                    // Final step: save configs and signal completion.
                    self.save_configs()?;
                    self.step = WizardStep::Complete;
                    return Ok(Some(Action::WizardComplete));
                }
                self.step = self.step.next();
                Ok(None)
            }
            Action::WizardBack => {
                self.step = self.step.prev();
                Ok(None)
            }
            Action::Paste(text) if self.step == WizardStep::ApiKey => {
                match self.selected_provider {
                    ProviderKind::Bedrock => {
                        self.active_bedrock_field().push_str(text.trim());
                    }
                    ProviderKind::OpenRouter => {
                        self.api_key_input.push_str(text.trim());
                    }
                    ProviderKind::ChatGpt => {}
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()> {
        let wizard_area = Self::centered_rect(area, 60, 20);

        // Split into header + content.
        let chunks =
            Layout::vertical([Constraint::Length(2), Constraint::Min(4)]).split(wizard_area);

        self.draw_header(frame, chunks[0]);

        match self.step {
            WizardStep::Provider => self.draw_provider(frame, chunks[1]),
            WizardStep::ApiKey => self.draw_api_key(frame, chunks[1]),
            WizardStep::ModelSelect => self.draw_model_select(frame, chunks[1]),
            WizardStep::ApprovalMode => self.draw_approval_mode(frame, chunks[1]),
            WizardStep::DenyRules => self.draw_deny_rules(frame, chunks[1]),
            WizardStep::ProjectInit => self.draw_project_init(frame, chunks[1]),
            WizardStep::Complete => {
                // Brief completion message (normally transitions immediately).
                let msg = Paragraph::new("Configuration saved! Starting Seval...")
                    .alignment(Alignment::Center);
                frame.render_widget(msg, chunks[1]);
            }
        }

        Ok(())
    }
}
