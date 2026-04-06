//! Agent slash command handlers.

use std::fmt::Write;
use std::time::Instant;

use super::component::Chat;
use crate::action::Action;
use crate::agents::types::AgentSource;

/// Tracking entry for a running agent (used by /agents status).
pub(super) struct AgentStatusEntry {
    pub(super) max_turns: u32,
    pub(super) current_turn: u32,
    pub(super) started_at: Instant,
}

/// Record of a completed agent this session (used by /agents status).
pub(super) struct CompletedAgentInfo {
    pub(super) name: String,
    pub(super) turns_completed: u32,
    pub(super) max_turns: u32,
    pub(super) elapsed_secs: u64,
    pub(super) status: String,
}

/// Format seconds as a human-readable duration string.
fn format_duration(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl Chat {
    /// Handle /agents subcommands (per D-15).
    pub(super) fn handle_agents_command(&mut self, sub: Option<&str>) {
        match sub {
            None | Some("list") => self.agents_list(),
            Some(s) if s.starts_with("info ") => {
                let name = s.strip_prefix("info ").unwrap().trim();
                self.agents_info(name);
            }
            Some("status") => self.agents_status(),
            Some(s) if s.starts_with("cancel ") => {
                let rest = s.strip_prefix("cancel ").unwrap().trim();
                self.agents_cancel(rest);
            }
            Some(s) if s.starts_with("create ") => {
                let name = s.strip_prefix("create ").unwrap().trim();
                self.agents_create(name);
            }
            Some(other) => {
                self.add_system_message(format!(
                    "Unknown /agents subcommand: '{other}'. Use /agents for help."
                ));
            }
        }
    }

    /// List all available agents with name, description, and source tag (AGENTCMD-01 / D-07).
    fn agents_list(&mut self) {
        let agents = self.agent_registry.list();
        if agents.is_empty() {
            self.add_system_message(
                "No agents available. Use /agents create <name> to scaffold one.".to_string(),
            );
            return;
        }

        let mut output = String::from("Available agents:\n");
        let mut builtin_count = 0u32;
        let mut user_count = 0u32;
        let mut project_count = 0u32;

        for agent in &agents {
            let tag = match agent.source {
                AgentSource::BuiltIn => {
                    builtin_count += 1;
                    "[built-in]"
                }
                AgentSource::UserGlobal => {
                    user_count += 1;
                    "[user]"
                }
                AgentSource::ProjectLocal => {
                    project_count += 1;
                    "[project]"
                }
            };
            let desc = agent
                .frontmatter
                .description
                .as_deref()
                .unwrap_or("(no description)");
            let _ = writeln!(
                output,
                "  {:<25} {:<12} {}",
                agent.frontmatter.name, tag, desc
            );
        }

        // Summary line
        let total = agents.len();
        let mut parts = Vec::new();
        if builtin_count > 0 {
            parts.push(format!("{builtin_count} built-in"));
        }
        if user_count > 0 {
            parts.push(format!("{user_count} user"));
        }
        if project_count > 0 {
            parts.push(format!("{project_count} project"));
        }
        let _ = write!(output, "\n{total} agent(s) ({})", parts.join(", "));
        let _ = write!(output, "\nUse /agents info <name> for details.");

        self.add_system_message(output);
    }

    /// Show full agent configuration with system prompt preview (AGENTCMD-02 / D-08).
    fn agents_info(&mut self, name: &str) {
        if name.is_empty() {
            self.add_system_message("Usage: /agents info <name>".to_string());
            return;
        }

        let Some(agent) = self.agent_registry.get(name) else {
            self.add_system_message(format!(
                "Agent '{name}' not found. Use /agents to list available agents."
            ));
            return;
        };

        let fm = &agent.frontmatter;
        let tag = match agent.source {
            AgentSource::BuiltIn => "[built-in]",
            AgentSource::UserGlobal => "[user]",
            AgentSource::ProjectLocal => "[project]",
        };

        let allowed = if fm.allowed_tools.is_empty() {
            "(all)".to_string()
        } else {
            fm.allowed_tools.join(", ")
        };
        let denied = if fm.denied_tools.is_empty() {
            "(none)".to_string()
        } else {
            fm.denied_tools.join(", ")
        };
        let approval = fm
            .approval_mode
            .map_or_else(|| "(default)".to_string(), |m| format!("{m:?}"));

        // First 5 lines of system prompt
        let prompt_lines: Vec<&str> = agent.system_prompt.lines().take(5).collect();
        let prompt_preview = if agent.system_prompt.lines().count() > 5 {
            format!("{}\n...", prompt_lines.join("\n"))
        } else {
            prompt_lines.join("\n")
        };

        let output = format!(
            "Agent: {name} {tag}\n\
             Description:   {}\n\
             Model:         {}\n\
             Temperature:   {:.1}\n\
             Max turns:     {}\n\
             Max time:      {}m\n\
             Allowed tools: {allowed}\n\
             Denied tools:  {denied}\n\
             Approval mode: {approval}\n\
             \n\
             System prompt (first 5 lines):\n\
             {}",
            fm.description.as_deref().unwrap_or("(none)"),
            fm.model,
            fm.temperature,
            fm.max_turns,
            fm.max_time_minutes,
            prompt_preview,
        );

        self.add_system_message(output);
    }

    /// Show running and completed agents with turn progress and elapsed time (AGENTCMD-03 / D-09).
    fn agents_status(&mut self) {
        let mut output = String::new();

        let handles = self
            .agent_handles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let running_names: Vec<String> = handles.keys().cloned().collect();
        drop(handles);

        let completed_is_empty = self.completed_agent_log.is_empty();

        if running_names.is_empty() && completed_is_empty {
            self.add_system_message("No agents running or completed this session.".to_string());
            return;
        }

        if !running_names.is_empty() {
            let _ = writeln!(output, "Running agents:");
            for name in &running_names {
                if let Some(status) = self.agent_status.get(name.as_str()) {
                    let elapsed = status.started_at.elapsed().as_secs();
                    let _ = writeln!(
                        output,
                        "  {:<25} turn {}/{}    elapsed {}",
                        name,
                        status.current_turn,
                        status.max_turns,
                        format_duration(elapsed)
                    );
                } else {
                    let _ = writeln!(output, "  {name:<25} starting...");
                }
            }
        }

        if !completed_is_empty {
            let _ = writeln!(output, "\nCompleted agents (this session):");
            for info in &self.completed_agent_log {
                let _ = writeln!(
                    output,
                    "  {:<25} {} ({}/{} turns, {})",
                    info.name,
                    info.status,
                    info.turns_completed,
                    info.max_turns,
                    format_duration(info.elapsed_secs)
                );
            }
        }

        self.add_system_message(output);
    }

    /// Cancel a running agent with two-step confirmation (D-10, D-11, AGENTEXEC-05).
    fn agents_cancel(&mut self, rest: &str) {
        if let Some(name) = rest.strip_suffix(" confirm") {
            let name = name.trim().to_string();
            let mut handles = self
                .agent_handles
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((handle, partial_output)) = handles.remove(&name) {
                handle.abort();
                let partial = partial_output
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let turns_completed = self
                    .agent_status
                    .get(name.as_str())
                    .map_or(0, |s| s.current_turn);
                let max_turns = self
                    .agent_status
                    .get(name.as_str())
                    .map_or(0, |s| s.max_turns);
                let elapsed_secs = self
                    .agent_status
                    .get(name.as_str())
                    .map_or(0, |s| s.started_at.elapsed().as_secs());
                drop(handles);

                let result = crate::agents::executor::AgentResult::new(
                    name.clone(),
                    crate::agents::executor::AgentStatus::Cancelled,
                    turns_completed,
                    max_turns,
                    elapsed_secs,
                    partial,
                );

                if let Some(tx) = &self.session.action_tx {
                    let _ = tx.send(Action::AgentCompleted(result));
                }

                self.agent_status.remove(&name);
            } else {
                drop(handles);
                self.add_system_message(format!("Agent '{name}' is not running."));
            }
        } else {
            // Check if the agent is running before prompting for confirmation (per Pitfall 3)
            let name = rest.trim();
            let handles = self
                .agent_handles
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let is_running = handles.contains_key(name);
            drop(handles);

            if is_running {
                self.add_system_message(format!(
                    "Cancel '{name}'? Type: /agents cancel {name} confirm"
                ));
            } else {
                self.add_system_message(format!("Agent '{name}' is not running."));
            }
        }
    }

    /// Scaffold a new agent template at ~/.seval/agents/<name>.md (AGENTCMD-04 / D-12, D-13, D-14).
    fn agents_create(&mut self, name: &str) {
        if name.is_empty() {
            self.add_system_message("Usage: /agents create <name>".to_string());
            return;
        }

        // Check if agent already exists (per D-14)
        if let Some(existing) = self.agent_registry.get(name) {
            let path_hint = match existing.source {
                AgentSource::BuiltIn => "~/.seval/agents/default/".to_string(),
                AgentSource::UserGlobal => "~/.seval/agents/".to_string(),
                AgentSource::ProjectLocal => ".seval/agents/".to_string(),
            };
            self.add_system_message(format!(
                "Agent '{name}' already exists. Use a different name or edit the existing file at {path_hint}{name}.md"
            ));
            return;
        }

        // Determine output path (per D-12: user-global tier)
        let Some(base) = directories::BaseDirs::new() else {
            self.add_system_message("Cannot determine home directory.".to_string());
            return;
        };
        let dir = base.home_dir().join(".seval").join("agents");
        let path = dir.join(format!("{name}.md"));
        let path_display = path.display().to_string();
        let name_owned = name.to_string();

        // Template content (per D-13)
        let template = format!(
            r#"+++
name = "{name_owned}"
# description = "What this agent does"
model = "sonnet"
# temperature = 0.7
max_turns = 10
# max_time_minutes = 10
# allowed_tools = ["shell", "read", "grep"]
# denied_tools = []
# approval_mode = "yolo"
+++
# TODO: Write your system prompt here.
#
# This becomes the agent's system prompt. Use markdown freely.
# The agent will receive this as its preamble before each task.
"#
        );

        if let Some(tx) = &self.session.action_tx {
            let tx = tx.clone();
            tokio::task::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(&path, template)?;
                    Ok::<_, std::io::Error>(())
                })
                .await;
                match result {
                    Ok(Ok(())) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Created agent template at {path_display}\nEdit the file to customize your agent, then use /agents to verify it loads."
                        )));
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Failed to create agent template: {e}"
                        )));
                    }
                    Err(e) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Task error creating agent: {e}"
                        )));
                    }
                }
            });
        }
    }
}
