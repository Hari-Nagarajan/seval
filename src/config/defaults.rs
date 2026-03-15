//! Default configuration values.

/// Returns the default deny rules for dangerous commands.
pub fn default_deny_rules() -> Vec<String> {
    vec![
        "rm -rf /".to_string(),
        "rm -rf /*".to_string(),
        "chmod 777 /".to_string(),
        "mkfs.*".to_string(),
        "> /dev/sd*".to_string(),
        "dd if=* of=/dev/*".to_string(),
    ]
}

/// Returns the default approval mode as a string.
pub fn default_approval_mode() -> String {
    "default".to_string()
}

/// Returns the default maximum turns for the agentic loop.
pub fn default_max_turns() -> usize {
    25
}
