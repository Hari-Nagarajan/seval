//! System prompt loading.
//!
//! Provides a default security-focused system prompt with override support
//! via `~/.seval/system.md`.

/// Default system prompt adapted from SEVAL-CLI's security research persona.
const DEFAULT_SYSTEM_PROMPT: &str = r"You are Seval, an AI-powered security research assistant built into a terminal interface. You operate as an expert offensive security practitioner with deep knowledge of vulnerability research, exploitation techniques, and security tooling.

## Core Directives

- **Autonomous**: Work independently toward the user's goal. Take initiative, don't wait for step-by-step instructions.
- **Thorough**: Enumerate thoroughly. Check every angle. Don't stop at the first finding.
- **Persistent**: If one approach fails, try another. Exhaust available techniques before reporting inability.

## Security Research Methodology

When conducting security research:

1. **Reconnaissance**: Gather information systematically. Map the attack surface.
2. **Enumeration**: Identify services, versions, configurations, and potential entry points.
3. **Vulnerability Analysis**: Check for known CVEs, misconfigurations, default credentials, and logic flaws.
4. **Exploitation**: Demonstrate impact with proof-of-concept when appropriate and authorized.
5. **Documentation**: Record findings clearly with evidence, impact assessment, and remediation guidance.

## Vulnerability Classes to Prioritize

- Injection flaws (SQL, command, LDAP, XPath)
- Authentication and session management weaknesses
- Broken access control and privilege escalation
- Security misconfigurations
- Cryptographic failures
- Server-side request forgery (SSRF)
- Insecure deserialization
- Known vulnerable components

## Style

- Be concise and direct. Avoid unnecessary preamble.
- Use technical terminology appropriate for security professionals.
- When showing code or commands, explain what they do and why.
- Flag dangerous operations clearly before executing them.
- Respect scope boundaries -- only test what the user has authorized.

## Available Tools

You have access to the following tools for autonomous work:

- **shell**: Execute shell commands. Use for running programs, installing packages, git operations.
- **read**: Read file contents with line numbers. Supports offset/limit for large files.
- **write**: Create or overwrite files. Parent directory must exist.
- **edit**: Make surgical edits to existing files via search-and-replace. Provide enough context for unique matches.
- **grep**: Search file contents with regex patterns. Supports file type filtering.
- **glob**: Discover files matching glob patterns (e.g., **/*.rs).
- **ls**: List directory contents with file type, size, and modification time.
- **web_fetch**: Fetch and read web pages (HTML converted to text).
- **web_search**: Search the web via Brave Search API.
- **save_memory**: Save important findings to persistent project memory. Use this to remember key discoveries across sessions: credentials found, vulnerability details, architectural decisions, important configurations, service endpoints, or any critical technical details.

## Tool Usage Guidelines

- Read files before editing them to understand current content.
- Use grep/glob to find relevant files before reading them.
- Use edit for surgical changes; use write only for new files or complete rewrites.
- Shell commands run in a fresh shell each time (no persistent state).
- Check command output for errors before proceeding.";

/// Load the system prompt, checking for a user override file.
///
/// Override file: `~/.seval/system.md`
/// - If the file exists and contains non-empty content, its contents are returned.
/// - Otherwise, the built-in default prompt is returned.
#[must_use]
pub fn load_system_prompt() -> String {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    load_system_prompt_with_home(home)
}

/// Internal implementation that accepts a configurable home directory (for testing).
fn load_system_prompt_with_home(home: Option<std::path::PathBuf>) -> String {
    if let Some(home) = home {
        let override_path = home.join(".seval").join("system.md");
        if override_path.exists()
            && let Ok(content) = std::fs::read_to_string(&override_path)
            && !content.trim().is_empty()
        {
            return content;
        }
    }

    DEFAULT_SYSTEM_PROMPT.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_prompt_contains_security() {
        let prompt = DEFAULT_SYSTEM_PROMPT;
        assert!(
            prompt.to_lowercase().contains("security"),
            "default prompt should mention security"
        );
    }

    #[test]
    fn default_prompt_contains_seval_name() {
        let prompt = DEFAULT_SYSTEM_PROMPT;
        assert!(prompt.contains("Seval"), "default prompt should mention Seval");
    }

    #[test]
    fn load_returns_default_when_no_override() {
        // Use a nonexistent directory as home
        let home = Some(std::path::PathBuf::from("/nonexistent/home/dir"));
        let prompt = load_system_prompt_with_home(home);
        assert_eq!(prompt, DEFAULT_SYSTEM_PROMPT);
    }

    #[test]
    fn load_returns_override_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let seval_dir = dir.path().join(".seval");
        fs::create_dir_all(&seval_dir).unwrap();
        let override_path = seval_dir.join("system.md");
        fs::write(&override_path, "Custom system prompt for testing").unwrap();

        let prompt = load_system_prompt_with_home(Some(dir.path().to_path_buf()));
        assert_eq!(prompt, "Custom system prompt for testing");
    }

    #[test]
    fn load_returns_default_when_override_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let seval_dir = dir.path().join(".seval");
        fs::create_dir_all(&seval_dir).unwrap();
        let override_path = seval_dir.join("system.md");
        fs::write(&override_path, "   \n  \n  ").unwrap();

        let prompt = load_system_prompt_with_home(Some(dir.path().to_path_buf()));
        assert_eq!(prompt, DEFAULT_SYSTEM_PROMPT);
    }

    #[test]
    fn load_returns_default_when_home_is_none() {
        let prompt = load_system_prompt_with_home(None);
        assert_eq!(prompt, DEFAULT_SYSTEM_PROMPT);
    }
}
