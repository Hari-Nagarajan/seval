use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;
use rig::message::Message;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::action::Action;
use crate::approval::display::format_tool_display;
use crate::config::types::ApprovalMode;

/// Tool category for permission mode logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Read-only tools: read, grep, glob, ls, `web_fetch`, `web_search`.
    Read,
    /// File modification tools: write, edit.
    Write,
    /// Shell command execution.
    Shell,
}

/// Classify a tool name into a permission category.
///
/// Unknown tools default to `Shell` (most restrictive).
pub fn classify_tool(name: &str) -> ToolCategory {
    match name {
        "read" | "grep" | "glob" | "ls" | "web_fetch" | "web_search" | "save_memory"
        | "search_memory" => ToolCategory::Read,
        "write" | "edit" => ToolCategory::Write,
        // "process" and "shell" are both Shell-category (require approval in Default mode).
        _ => ToolCategory::Shell,
    }
}

/// Check if shell command args match any deny rule (substring match).
///
/// Returns the matching rule if found, `None` otherwise.
/// Only applies to shell tool args that contain a "command" JSON field.
pub fn matches_deny_rule(deny_rules: &[String], args_json: &str) -> Option<String> {
    let command = serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| v.get("command")?.as_str().map(String::from))?;

    deny_rules
        .iter()
        .find(|rule| command.contains(rule.as_str()))
        .cloned()
}

/// User's decision on a tool approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Approve this single tool call.
    Approve,
    /// Deny this single tool call.
    Deny,
    /// Approve all future calls of this tool type for the session.
    ApproveAll,
}

/// A tool approval request sent from the hook to the TUI.
pub struct ApprovalRequest {
    /// Name of the tool being invoked.
    pub tool_name: String,
    /// Raw JSON arguments.
    pub args_json: String,
    /// Human-readable formatted display.
    pub formatted_display: String,
    /// Channel to send the user's decision back to the hook.
    pub response_tx: oneshot::Sender<ApprovalDecision>,
}

/// Hook that intercepts tool calls for approval gating.
///
/// Implements Rig's `PromptHook` trait. Uses channels to communicate
/// with the TUI for interactive approval.
#[derive(Clone)]
pub struct ApprovalHook {
    /// Channel to send approval requests to the TUI.
    approval_tx: mpsc::UnboundedSender<ApprovalRequest>,
    /// Action channel for sending `ToolDenied` notifications.
    action_tx: mpsc::UnboundedSender<Action>,
    /// Current permission mode.
    mode: ApprovalMode,
    /// Deny rules for shell commands.
    deny_rules: Vec<String>,
    /// Set of tool names approved for "all of this type" this session.
    approved_all: Arc<Mutex<HashSet<String>>>,
    /// Turn counter for status bar display.
    turn_counter: Arc<AtomicUsize>,
    /// Maximum turns for display purposes.
    max_turns: usize,
    /// Optional allowlist of tool names for agent-scoped filtering.
    ///
    /// When `Some`, any tool not in the list is auto-skipped before the normal
    /// approval logic. Used by spawned agents to enforce `effective_tools`.
    /// Parent chat passes `None` (no change to existing behaviour).
    effective_tool_filter: Option<Vec<String>>,
}

impl ApprovalHook {
    /// Create a new approval hook.
    ///
    /// Pass `effective_tool_filter = None` for the parent chat (all tools
    /// allowed per the approval mode). Pass `Some(list)` for spawned agents
    /// to restrict execution to only the tools in `list`.
    pub fn new(
        mode: ApprovalMode,
        deny_rules: Vec<String>,
        approval_tx: mpsc::UnboundedSender<ApprovalRequest>,
        action_tx: mpsc::UnboundedSender<Action>,
        max_turns: usize,
        effective_tool_filter: Option<Vec<String>>,
    ) -> Self {
        Self {
            approval_tx,
            action_tx,
            mode,
            deny_rules,
            approved_all: Arc::new(Mutex::new(HashSet::new())),
            turn_counter: Arc::new(AtomicUsize::new(0)),
            max_turns,
            effective_tool_filter,
        }
    }

    /// Get a reference to the turn counter Arc (for sharing with status bar).
    pub fn turn_counter(&self) -> Arc<AtomicUsize> {
        self.turn_counter.clone()
    }

    /// Get the current turn count.
    pub fn turn_count(&self) -> usize {
        self.turn_counter.load(Ordering::Relaxed)
    }

    /// Get the deny rules slice.
    pub fn deny_rules(&self) -> &[String] {
        &self.deny_rules
    }

    /// Get max turns for display.
    pub fn max_turns_for_display(&self) -> usize {
        self.max_turns
    }

    /// Determine if a tool call should be auto-decided (without user prompt).
    ///
    /// Returns `Some(action)` if auto-decided, `None` if user approval needed.
    pub fn should_auto_decide(
        &self,
        tool_name: &str,
        args_json: &str,
    ) -> Option<ToolCallHookAction> {
        let category = classify_tool(tool_name);

        // Check deny rules first (shell only)
        if category == ToolCategory::Shell
            && let Some(rule) = matches_deny_rule(&self.deny_rules, args_json)
        {
            return Some(ToolCallHookAction::skip(format!(
                "Command blocked by deny rule: {rule}"
            )));
        }

        // Check permission mode
        match self.mode {
            ApprovalMode::Yolo => return Some(ToolCallHookAction::Continue),
            ApprovalMode::Plan => {
                return if category == ToolCategory::Read {
                    Some(ToolCallHookAction::Continue)
                } else {
                    Some(ToolCallHookAction::skip(
                        "Tool execution denied in Plan mode (read-only)",
                    ))
                };
            }
            ApprovalMode::AutoEdit => {
                if category == ToolCategory::Read || category == ToolCategory::Write {
                    return Some(ToolCallHookAction::Continue);
                }
                // Shell falls through to approval prompt
            }
            ApprovalMode::Default => {
                if category == ToolCategory::Read {
                    return Some(ToolCallHookAction::Continue);
                }
                // Write and Shell fall through to approval prompt
            }
        }

        // Not auto-decided
        None
    }
}

impl<M: CompletionModel> PromptHook<M> for ApprovalHook {
    async fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> ToolCallHookAction {
        // Check effective tool filter for agent-scoped restrictions.
        // If the filter is set and this tool is not in the allowed list, skip it.
        if let Some(ref filter) = self.effective_tool_filter
            && !filter.iter().any(|t| t == tool_name)
        {
            let reason = format!("Tool '{tool_name}' not available for this agent");
            let _ = self.action_tx.send(Action::ToolDenied {
                name: tool_name.to_string(),
                reason: reason.clone(),
            });
            return ToolCallHookAction::skip(reason);
        }

        // Check auto-decide first
        if let Some(action) = self.should_auto_decide(tool_name, args) {
            // If this is a skip (denial), notify the TUI
            if let ToolCallHookAction::Skip { reason } = &action {
                let _ = self.action_tx.send(Action::ToolDenied {
                    name: tool_name.to_string(),
                    reason: reason.clone(),
                });
            }
            return action;
        }

        // Check "approve all of this type" set
        {
            let approved = self.approved_all.lock().await;
            if approved.contains(tool_name) {
                return ToolCallHookAction::Continue;
            }
        }

        // Request user approval via channel
        let (response_tx, response_rx) = oneshot::channel();
        let request = ApprovalRequest {
            tool_name: tool_name.to_string(),
            args_json: args.to_string(),
            formatted_display: format_tool_display(tool_name, args),
            response_tx,
        };

        if self.approval_tx.send(request).is_err() {
            return ToolCallHookAction::skip("Approval channel closed");
        }

        match response_rx.await {
            Ok(ApprovalDecision::Approve) => ToolCallHookAction::Continue,
            Ok(ApprovalDecision::Deny) => ToolCallHookAction::skip("Tool execution denied by user"),
            Ok(ApprovalDecision::ApproveAll) => {
                let mut approved = self.approved_all.lock().await;
                approved.insert(tool_name.to_string());
                ToolCallHookAction::Continue
            }
            Err(_) => ToolCallHookAction::skip("Approval request cancelled"),
        }
    }

    async fn on_completion_call(&self, _prompt: &Message, _history: &[Message]) -> HookAction {
        self.turn_counter.fetch_add(1, Ordering::Relaxed);
        HookAction::cont()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- classify_tool tests ----

    #[test]
    fn test_classify_read_tools() {
        assert_eq!(classify_tool("read"), ToolCategory::Read);
        assert_eq!(classify_tool("grep"), ToolCategory::Read);
        assert_eq!(classify_tool("glob"), ToolCategory::Read);
        assert_eq!(classify_tool("ls"), ToolCategory::Read);
        assert_eq!(classify_tool("web_fetch"), ToolCategory::Read);
        assert_eq!(classify_tool("web_search"), ToolCategory::Read);
    }

    #[test]
    fn test_classify_write_tools() {
        assert_eq!(classify_tool("write"), ToolCategory::Write);
        assert_eq!(classify_tool("edit"), ToolCategory::Write);
    }

    #[test]
    fn test_classify_shell() {
        assert_eq!(classify_tool("shell"), ToolCategory::Shell);
    }

    #[test]
    fn test_classify_save_memory_as_read() {
        assert_eq!(classify_tool("save_memory"), ToolCategory::Read);
    }

    #[test]
    fn test_classify_unknown_defaults_to_shell() {
        assert_eq!(classify_tool("unknown_tool"), ToolCategory::Shell);
        assert_eq!(classify_tool(""), ToolCategory::Shell);
        assert_eq!(classify_tool("custom"), ToolCategory::Shell);
    }

    // ---- deny rule tests ----

    #[test]
    fn test_deny_rule_match() {
        let rules = vec!["rm -rf /".to_string()];
        let args = r#"{"command": "rm -rf /"}"#;
        assert_eq!(
            matches_deny_rule(&rules, args),
            Some("rm -rf /".to_string())
        );
    }

    #[test]
    fn test_deny_rule_no_match() {
        let rules = vec!["rm -rf /".to_string()];
        let args = r#"{"command": "ls -la"}"#;
        assert_eq!(matches_deny_rule(&rules, args), None);
    }

    #[test]
    fn test_deny_rule_substring_match() {
        let rules = vec!["rm -rf /".to_string()];
        let args = r#"{"command": "sudo rm -rf /home"}"#;
        assert_eq!(
            matches_deny_rule(&rules, args),
            Some("rm -rf /".to_string())
        );
    }

    #[test]
    fn test_deny_rule_no_command_field() {
        let rules = vec!["rm -rf /".to_string()];
        let args = r#"{"not_command": "rm -rf /"}"#;
        assert_eq!(matches_deny_rule(&rules, args), None);
    }

    #[test]
    fn test_deny_rule_invalid_json() {
        let rules = vec!["rm -rf /".to_string()];
        assert_eq!(matches_deny_rule(&rules, "not json"), None);
    }

    #[test]
    fn test_deny_rule_empty_rules() {
        let rules: Vec<String> = vec![];
        let args = r#"{"command": "rm -rf /"}"#;
        assert_eq!(matches_deny_rule(&rules, args), None);
    }

    // ---- permission mode tests (via should_auto_decide) ----

    fn make_hook(mode: ApprovalMode) -> ApprovalHook {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (atx, _arx) = mpsc::unbounded_channel();
        ApprovalHook::new(mode, vec![], tx, atx, 25, None)
    }

    fn make_hook_with_deny(mode: ApprovalMode, deny_rules: Vec<String>) -> ApprovalHook {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (atx, _arx) = mpsc::unbounded_channel();
        ApprovalHook::new(mode, deny_rules, tx, atx, 25, None)
    }

    // Yolo mode: all categories auto-approved
    #[test]
    fn test_yolo_approves_read() {
        let hook = make_hook(ApprovalMode::Yolo);
        let result = hook.should_auto_decide("read", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_yolo_approves_write() {
        let hook = make_hook(ApprovalMode::Yolo);
        let result = hook.should_auto_decide("write", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_yolo_approves_shell() {
        let hook = make_hook(ApprovalMode::Yolo);
        let result = hook.should_auto_decide("shell", r#"{"command": "ls"}"#);
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    // Plan mode: read auto-approved, write+shell denied
    #[test]
    fn test_plan_approves_read() {
        let hook = make_hook(ApprovalMode::Plan);
        let result = hook.should_auto_decide("read", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_plan_denies_write() {
        let hook = make_hook(ApprovalMode::Plan);
        let result = hook.should_auto_decide("write", "{}");
        assert!(matches!(result, Some(ToolCallHookAction::Skip { .. })));
    }

    #[test]
    fn test_plan_denies_shell() {
        let hook = make_hook(ApprovalMode::Plan);
        let result = hook.should_auto_decide("shell", r#"{"command": "ls"}"#);
        assert!(matches!(result, Some(ToolCallHookAction::Skip { .. })));
    }

    // AutoEdit mode: read+write auto-approved, shell falls through
    #[test]
    fn test_autoedit_approves_read() {
        let hook = make_hook(ApprovalMode::AutoEdit);
        let result = hook.should_auto_decide("read", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_autoedit_approves_write() {
        let hook = make_hook(ApprovalMode::AutoEdit);
        let result = hook.should_auto_decide("write", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_autoedit_falls_through_shell() {
        let hook = make_hook(ApprovalMode::AutoEdit);
        let result = hook.should_auto_decide("shell", r#"{"command": "ls"}"#);
        assert_eq!(result, None); // Falls through to user approval
    }

    // Default mode: read auto-approved, write+shell fall through
    #[test]
    fn test_default_approves_read() {
        let hook = make_hook(ApprovalMode::Default);
        let result = hook.should_auto_decide("read", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    #[test]
    fn test_default_falls_through_write() {
        let hook = make_hook(ApprovalMode::Default);
        let result = hook.should_auto_decide("write", "{}");
        assert_eq!(result, None);
    }

    #[test]
    fn test_default_falls_through_shell() {
        let hook = make_hook(ApprovalMode::Default);
        let result = hook.should_auto_decide("shell", r#"{"command": "ls"}"#);
        assert_eq!(result, None);
    }

    // Deny rules checked before permission mode
    #[test]
    fn test_deny_rule_overrides_yolo() {
        let hook = make_hook_with_deny(ApprovalMode::Yolo, vec!["rm -rf /".to_string()]);
        let result = hook.should_auto_decide("shell", r#"{"command": "rm -rf /"}"#);
        assert!(matches!(result, Some(ToolCallHookAction::Skip { .. })));
    }

    #[test]
    fn test_deny_rule_only_checks_shell() {
        // Even with deny rules, non-shell tools are unaffected
        let hook = make_hook_with_deny(ApprovalMode::Yolo, vec!["rm -rf /".to_string()]);
        let result = hook.should_auto_decide("read", "{}");
        assert_eq!(result, Some(ToolCallHookAction::Continue));
    }

    // Approve-all set test (async)
    #[tokio::test]
    async fn test_approve_all_set() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (atx, _arx) = mpsc::unbounded_channel();
        let hook = ApprovalHook::new(ApprovalMode::Default, vec![], tx, atx, 25, None);

        // Insert "shell" into approved_all set
        {
            let mut approved = hook.approved_all.lock().await;
            approved.insert("shell".to_string());
        }

        // Now on_tool_call should auto-approve "shell"
        let result =
            <ApprovalHook as PromptHook<rig_bedrock::completion::CompletionModel>>::on_tool_call(
                &hook,
                "shell",
                None,
                "test-id",
                r#"{"command": "ls"}"#,
            )
            .await;
        assert_eq!(result, ToolCallHookAction::Continue);

        // Nothing should have been sent to the channel
        assert!(rx.try_recv().is_err());
    }

    // Turn counter test
    #[test]
    fn test_turn_counter_initial() {
        let hook = make_hook(ApprovalMode::Default);
        assert_eq!(hook.turn_count(), 0);
    }

    #[test]
    fn test_max_turns_for_display() {
        let hook = make_hook(ApprovalMode::Default);
        assert_eq!(hook.max_turns_for_display(), 25);
    }

    // Hook is Clone + Send + Sync (compile-time check)
    #[test]
    fn test_hook_is_clone_send_sync() {
        fn assert_clone_send_sync<T: Clone + Send + Sync>() {}
        assert_clone_send_sync::<ApprovalHook>();
    }
}
