//! Input handling for the setup wizard.
//!
//! Contains all `handle_*_key` methods that process keyboard events
//! for each wizard step.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::config::ProviderKind;

use super::{MODE_DESCRIPTIONS, PROVIDER_OPTIONS, Wizard};

impl Wizard {
    /// Handle key events during the `Provider` step.
    pub(super) fn handle_provider_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.provider_state.selected().unwrap_or(0);
                let new = if i == 0 {
                    PROVIDER_OPTIONS.len() - 1
                } else {
                    i - 1
                };
                self.provider_state.select(Some(new));
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.provider_state.selected().unwrap_or(0);
                let new = (i + 1) % PROVIDER_OPTIONS.len();
                self.provider_state.select(Some(new));
                None
            }
            KeyCode::Enter => {
                let index = self.provider_state.selected().unwrap_or(0);
                self.selected_provider = if index == 0 {
                    ProviderKind::Bedrock
                } else {
                    ProviderKind::OpenRouter
                };
                // Reset model selection for the chosen provider.
                self.model_state.select(Some(0));
                self.selected_model = self.models_for_provider()[0].0.to_string();
                Some(Action::WizardNext)
            }
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Quit),
            _ => None,
        }
    }

    /// Get a mutable reference to the currently active Bedrock credential field.
    pub(super) fn active_bedrock_field(&mut self) -> &mut String {
        match self.bedrock_field_index {
            0 => &mut self.bedrock_access_key,
            1 => &mut self.bedrock_secret_key,
            _ => &mut self.bedrock_region,
        }
    }

    /// Handle key events during the `ApiKey` step.
    pub(super) fn handle_api_key_key(&mut self, key: KeyEvent) -> Option<Action> {
        match self.selected_provider {
            ProviderKind::Bedrock => self.handle_bedrock_creds_key(key),
            ProviderKind::OpenRouter => self.handle_openrouter_key_input(key),
            ProviderKind::ChatGpt => Some(Action::WizardNext),
        }
    }

    /// Handle key events for Bedrock credential fields.
    fn handle_bedrock_creds_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                // Cycle to next field.
                self.bedrock_field_index = (self.bedrock_field_index + 1) % 3;
                None
            }
            KeyCode::BackTab | KeyCode::Up => {
                // Cycle to previous field.
                self.bedrock_field_index = if self.bedrock_field_index == 0 {
                    2
                } else {
                    self.bedrock_field_index - 1
                };
                None
            }
            KeyCode::Enter => {
                if !self.bedrock_access_key.trim().is_empty()
                    && !self.bedrock_secret_key.trim().is_empty()
                    && !self.bedrock_region.trim().is_empty()
                {
                    Some(Action::WizardNext)
                } else {
                    // Move to next empty field.
                    if self.bedrock_access_key.trim().is_empty() {
                        self.bedrock_field_index = 0;
                    } else if self.bedrock_secret_key.trim().is_empty() {
                        self.bedrock_field_index = 1;
                    } else {
                        self.bedrock_field_index = 2;
                    }
                    None
                }
            }
            KeyCode::Backspace => {
                self.active_bedrock_field().pop();
                None
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.api_key_masked = !self.api_key_masked;
                None
            }
            KeyCode::Char(c) => {
                self.active_bedrock_field().push(c);
                None
            }
            KeyCode::Esc => Some(Action::WizardBack),
            _ => None,
        }
    }

    /// Handle key events for `OpenRouter` single API key input.
    fn handle_openrouter_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                if self.api_key_input.trim().is_empty() {
                    None
                } else {
                    Some(Action::WizardNext)
                }
            }
            KeyCode::Backspace => {
                self.api_key_input.pop();
                None
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.api_key_masked = !self.api_key_masked;
                None
            }
            KeyCode::Char(c) => {
                self.api_key_input.push(c);
                None
            }
            KeyCode::Esc => Some(Action::WizardBack),
            _ => None,
        }
    }

    /// Handle key events during the `ModelSelect` step.
    pub(super) fn handle_model_select_key(&mut self, key: KeyEvent) -> Option<Action> {
        let models = self.models_for_provider();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.model_state.selected().unwrap_or(0);
                let new = if i == 0 { models.len() - 1 } else { i - 1 };
                self.model_state.select(Some(new));
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.model_state.selected().unwrap_or(0);
                let new = (i + 1) % models.len();
                self.model_state.select(Some(new));
                None
            }
            KeyCode::Enter => {
                let index = self.model_state.selected().unwrap_or(0);
                self.selected_model = models[index].0.to_string();
                Some(Action::WizardNext)
            }
            KeyCode::Esc => Some(Action::WizardBack),
            _ => None,
        }
    }

    /// Handle key events during the `ApprovalMode` step.
    pub(super) fn handle_approval_mode_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.approval_mode_state.selected().unwrap_or(0);
                let new = if i == 0 {
                    MODE_DESCRIPTIONS.len() - 1
                } else {
                    i - 1
                };
                self.approval_mode_state.select(Some(new));
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.approval_mode_state.selected().unwrap_or(0);
                let new = (i + 1) % MODE_DESCRIPTIONS.len();
                self.approval_mode_state.select(Some(new));
                None
            }
            KeyCode::Enter => {
                let index = self.approval_mode_state.selected().unwrap_or(1);
                self.selected_mode = Self::mode_from_index(index);
                Some(Action::WizardNext)
            }
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Quit),
            _ => None,
        }
    }

    /// Handle key events during the `DenyRules` step.
    pub(super) fn handle_deny_rules_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.is_adding_rule {
            return self.handle_deny_rules_adding_key(key);
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.deny_rules.is_empty() {
                    let i = self.deny_rule_state.selected().unwrap_or(0);
                    let new = if i == 0 {
                        self.deny_rules.len() - 1
                    } else {
                        i - 1
                    };
                    self.deny_rule_state.select(Some(new));
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.deny_rules.is_empty() {
                    let i = self.deny_rule_state.selected().unwrap_or(0);
                    let new = (i + 1) % self.deny_rules.len();
                    self.deny_rule_state.select(Some(new));
                }
                None
            }
            KeyCode::Char('a') => {
                self.is_adding_rule = true;
                self.editing_deny_rule = Some(String::new());
                None
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(i) = self.deny_rule_state.selected()
                    && i < self.deny_rules.len()
                {
                    self.deny_rules.remove(i);
                    // Fix selection after removal.
                    if self.deny_rules.is_empty() {
                        self.deny_rule_state.select(None);
                    } else if i >= self.deny_rules.len() {
                        self.deny_rule_state.select(Some(self.deny_rules.len() - 1));
                    }
                }
                None
            }
            KeyCode::Enter | KeyCode::Tab => Some(Action::WizardNext),
            KeyCode::Esc => Some(Action::WizardBack),
            _ => None,
        }
    }

    /// Handle key events while adding a new deny rule.
    fn handle_deny_rules_adding_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                if let Some(rule) = self.editing_deny_rule.take() {
                    let trimmed = rule.trim().to_string();
                    if !trimmed.is_empty() {
                        self.deny_rules.push(trimmed);
                        let last = self.deny_rules.len() - 1;
                        self.deny_rule_state.select(Some(last));
                    }
                }
                self.is_adding_rule = false;
                None
            }
            KeyCode::Esc => {
                self.editing_deny_rule = None;
                self.is_adding_rule = false;
                None
            }
            KeyCode::Backspace => {
                if let Some(ref mut rule) = self.editing_deny_rule {
                    rule.pop();
                }
                None
            }
            KeyCode::Char(c) => {
                if let Some(ref mut rule) = self.editing_deny_rule {
                    rule.push(c);
                }
                None
            }
            _ => None,
        }
    }

    /// Handle key events during the `ProjectInit` step.
    pub(super) fn handle_project_init_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => Some(Action::WizardNext),
            KeyCode::Char('y' | 'Y') => {
                self.create_project_config = true;
                None
            }
            KeyCode::Char('n' | 'N') => {
                self.create_project_config = false;
                None
            }
            KeyCode::Esc => Some(Action::WizardBack),
            _ => None,
        }
    }
}
