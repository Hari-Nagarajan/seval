//! Syntax highlighting for code blocks.
//!
//! Uses syntect to highlight code in various languages,
//! producing Ratatui `Line` elements for terminal rendering.

use std::sync::LazyLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Global syntax highlighter instance, loaded once and reused.
static HIGHLIGHTER: LazyLock<SyntaxHighlighter> = LazyLock::new(SyntaxHighlighter::new);

/// Language alias mappings for common shorthand names.
const LANG_ALIASES: &[(&str, &str)] = &[
    ("sh", "Bourne Again Shell (bash)"),
    ("bash", "Bourne Again Shell (bash)"),
    ("js", "JavaScript"),
    ("javascript", "JavaScript"),
    ("ts", "TypeScript"),
    ("typescript", "TypeScript"),
    ("py", "Python"),
    ("python", "Python"),
    ("yml", "YAML"),
    ("yaml", "YAML"),
    ("json", "JSON"),
    ("rust", "Rust"),
    ("rs", "Rust"),
    ("go", "Go"),
    ("c", "C"),
    ("cpp", "C++"),
    ("rb", "Ruby"),
    ("ruby", "Ruby"),
    ("toml", "TOML"),
    ("md", "Markdown"),
    ("markdown", "Markdown"),
    ("sql", "SQL"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("xml", "XML"),
    ("java", "Java"),
];

/// Holds loaded syntax definitions and theme for highlighting.
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl SyntaxHighlighter {
    /// Create a new highlighter with default syntaxes and themes.
    fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Find a syntax definition by language name, trying aliases first.
    fn find_syntax(&self, lang: &str) -> Option<&syntect::parsing::SyntaxReference> {
        // Try alias mapping first
        let lang_lower = lang.to_lowercase();
        for &(alias, canonical) in LANG_ALIASES {
            if alias == lang_lower
                && let Some(syn) = self.syntax_set.find_syntax_by_name(canonical)
            {
                return Some(syn);
            }
        }

        // Try by name
        if let Some(syn) = self.syntax_set.find_syntax_by_name(lang) {
            return Some(syn);
        }

        // Try by extension
        self.syntax_set.find_syntax_by_extension(lang)
    }
}

/// Highlight a code block with syntax coloring for the given language.
///
/// Returns a `Vec<Line>` with colored spans. If the language is not
/// recognized, falls back to plain text with a code style (gray text).
#[must_use]
pub fn highlight_code(code: &str, lang: &str) -> Vec<Line<'static>> {
    let highlighter = &*HIGHLIGHTER;

    let Some(syntax) = highlighter.find_syntax(lang) else {
        return highlight_plain(code);
    };

    let Some(theme) = highlighter.theme_set.themes.get("base16-ocean.dark") else {
        return highlight_plain(code);
    };

    let mut h = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line_str in code.lines() {
        let Ok(regions) = h.highlight_line(line_str, &highlighter.syntax_set) else {
            return highlight_plain(code);
        };

        let spans: Vec<Span<'static>> = regions
            .into_iter()
            .map(|(style, text)| {
                let fg = syntect_to_ratatui_color(style.foreground);
                let mut ratatui_style = Style::default().fg(fg);
                if style.font_style.contains(highlighting::FontStyle::BOLD) {
                    ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
                }
                if style.font_style.contains(highlighting::FontStyle::ITALIC) {
                    ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
                }
                if style
                    .font_style
                    .contains(highlighting::FontStyle::UNDERLINE)
                {
                    ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
                }
                Span::styled(text.to_string(), ratatui_style)
            })
            .collect();

        lines.push(Line::from(spans));
    }

    lines
}

/// Fallback for unknown languages: plain gray text on default background.
fn highlight_plain(code: &str) -> Vec<Line<'static>> {
    let style = Style::default().fg(Color::Rgb(200, 200, 200));
    code.lines()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

/// Convert a syntect RGBA color to a Ratatui `Color::Rgb`.
fn syntect_to_ratatui_color(color: highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_returns_non_empty() {
        let lines = highlight_code("fn main() {}", "rust");
        assert!(!lines.is_empty(), "rust highlighting should produce lines");
    }

    #[test]
    fn highlight_python_returns_non_empty() {
        let lines = highlight_code("print('hello')", "python");
        assert!(
            !lines.is_empty(),
            "python highlighting should produce lines"
        );
    }

    #[test]
    fn highlight_unknown_lang_falls_back() {
        let lines = highlight_code("some code here", "unknown-lang");
        assert!(!lines.is_empty(), "unknown lang should fall back to plain");
        // Plain fallback uses gray color
        let first = &lines[0];
        let span = &first.spans[0];
        assert_eq!(span.style.fg, Some(Color::Rgb(200, 200, 200)));
    }

    #[test]
    fn highlight_empty_code() {
        let lines = highlight_code("", "rust");
        // Empty string produces no lines (no iterations)
        // This is acceptable behavior
        assert!(lines.len() <= 1);
    }

    #[test]
    fn highlight_multiline_code() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code(code, "rust");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn lang_alias_sh() {
        let lines = highlight_code("echo hello", "sh");
        assert!(!lines.is_empty());
    }

    #[test]
    fn lang_alias_js() {
        let lines = highlight_code("console.log('hi')", "js");
        assert!(!lines.is_empty());
    }

    #[test]
    fn lang_alias_py() {
        let lines = highlight_code("x = 1", "py");
        assert!(!lines.is_empty());
    }

    #[test]
    fn lang_alias_yml() {
        let lines = highlight_code("key: value", "yml");
        assert!(!lines.is_empty());
    }
}
