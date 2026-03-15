//! Tools module.
//!
//! Implements built-in tools (shell, file ops, grep, glob, etc.) that the AI
//! can invoke during agentic execution.

pub mod edit;
pub mod glob;
pub mod grep;
pub mod ls;
pub mod read;
pub mod shell;
pub mod web_fetch;
pub mod web_search;
pub mod write;

pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use ls::LsTool;
pub use read::ReadTool;
pub use shell::ShellTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;
pub use write::WriteTool;

/// Truncate output to fit within `max_bytes`, preserving head and tail.
///
/// If the output fits within the limit, it is returned as-is. Otherwise,
/// approximately 80% of the budget goes to the head and 20% to the tail,
/// with a marker indicating how many bytes were truncated.
pub fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }

    let marker_template = "\n\n... [ bytes truncated] ...\n\n";
    // Reserve enough for the marker plus a generous digit count
    let marker_overhead = marker_template.len() + 20;

    if max_bytes <= marker_overhead {
        // Not enough room for any content plus marker; just return head
        let boundary = output.floor_char_boundary(max_bytes);
        return output[..boundary].to_string();
    }

    let budget = max_bytes - marker_overhead;
    let head_budget = budget * 4 / 5; // ~80%
    let tail_budget = budget - head_budget; // ~20%

    let head_end = output.floor_char_boundary(head_budget);
    let tail_start = output.ceil_char_boundary(output.len() - tail_budget);

    let truncated_bytes = output.len() - head_end - (output.len() - tail_start);
    let marker = format!("\n\n... [{truncated_bytes} bytes truncated] ...\n\n");

    let mut result = String::with_capacity(head_end + marker.len() + (output.len() - tail_start));
    result.push_str(&output[..head_end]);
    result.push_str(&marker);
    result.push_str(&output[tail_start..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_string_unchanged() {
        let input = "short";
        assert_eq!(truncate_output(input, 100), "short");
    }

    #[test]
    fn test_exact_limit_unchanged() {
        let input = "hello";
        assert_eq!(truncate_output(input, 5), "hello");
    }

    #[test]
    fn test_truncation_has_marker() {
        let input = "a".repeat(1000);
        let result = truncate_output(&input, 200);
        assert!(result.len() <= 200, "result len {} > 200", result.len());
        assert!(result.contains("bytes truncated"));
    }

    #[test]
    fn test_truncation_preserves_head_and_tail() {
        let input: String = (0..1000).map(|i| char::from(b'A' + (i % 26) as u8)).collect();
        let result = truncate_output(&input, 200);
        // Head should start with same chars
        assert!(result.starts_with(&input[..10]));
        // Tail should end with same chars
        assert!(result.ends_with(&input[input.len() - 10..]));
    }

    #[test]
    fn test_truncation_80_20_ratio() {
        let input = "a".repeat(10_000);
        let result = truncate_output(&input, 500);
        let marker_pos = result.find("... [").expect("marker present");
        let after_marker = result.rfind("] ...").expect("marker end");
        let head_len = marker_pos;
        let tail_len = result.len() - after_marker - "] ...\n\n".len();
        // Head should be roughly 4x tail (80/20 split)
        assert!(
            head_len > tail_len * 2,
            "head {head_len} should be much larger than tail {tail_len}"
        );
    }

    #[test]
    fn test_utf8_multibyte_no_panic() {
        // Each emoji is 4 bytes
        let input = "\u{1F600}".repeat(100); // 400 bytes
        let result = truncate_output(&input, 50);
        // Should not panic and should be valid UTF-8
        assert!(result.len() <= 50 || result.contains("bytes truncated"));
    }
}
