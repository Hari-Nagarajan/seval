//! Markdown-to-Ratatui renderer.
//!
//! Parses `CommonMark` markdown and converts it into styled Ratatui `Line`
//! elements for terminal display. Code blocks are syntax-highlighted
//! via the [`syntax`](super::syntax) module.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::syntax::highlight_code;

/// Render a markdown string into styled Ratatui `Line` elements.
///
/// Handles bold, italic, strikethrough, inline code, headings, lists
/// (ordered and unordered), links, and fenced code blocks with syntax
/// highlighting.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render_markdown(input: &str) -> Vec<Line<'static>> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];

    // Code block accumulation
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_content = String::new();

    // List tracking
    let mut list_stack: Vec<ListContext> = Vec::new();

    // Link URL (stored for potential future OSC 8 support)
    let mut link_url = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {
                // Start a new paragraph (add blank line if we already have content)
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
            }

            Event::Start(Tag::Heading { level, .. }) => {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                let hs = heading_style(level);
                style_stack.push(hs);
            }

            Event::End(TagEnd::Heading(_)) => {
                style_stack.pop();
                flush_spans(&mut current_spans, &mut lines);
            }

            Event::Start(Tag::Strong) => {
                let current = current_style(&style_stack);
                style_stack.push(current.add_modifier(Modifier::BOLD));
            }

            Event::Start(Tag::Emphasis) => {
                let current = current_style(&style_stack);
                style_stack.push(current.add_modifier(Modifier::ITALIC));
            }

            Event::Start(Tag::Strikethrough) => {
                let current = current_style(&style_stack);
                style_stack.push(current.add_modifier(Modifier::CROSSED_OUT));
            }

            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                style_stack.pop();
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_block_content.clear();
                code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }

            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                flush_spans(&mut current_spans, &mut lines);
                render_code_block(
                    &code_block_lang,
                    &code_block_content,
                    &mut lines,
                );
                code_block_content.clear();
                code_block_lang.clear();
            }

            Event::Start(Tag::List(first_number)) => {
                if list_stack.is_empty() && !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                list_stack.push(ListContext {
                    ordered: first_number.is_some(),
                    counter: first_number.unwrap_or(1),
                });
            }

            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }

            Event::Start(Tag::Item) => {
                flush_spans(&mut current_spans, &mut lines);
                push_list_prefix(&list_stack, &mut current_spans);
                if let Some(ctx) = list_stack.last_mut()
                    && ctx.ordered
                {
                    ctx.counter += 1;
                }
            }

            Event::Start(Tag::Link { dest_url, .. }) => {
                link_url = dest_url.to_string();
                let current = current_style(&style_stack);
                style_stack.push(
                    current
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }

            Event::End(TagEnd::Link) => {
                style_stack.pop();
                link_url.clear();
            }

            Event::Text(text) => {
                if in_code_block {
                    code_block_content.push_str(&text);
                } else {
                    let style = current_style(&style_stack);
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }

            Event::Code(code) => {
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(40, 40, 40));
                current_spans.push(Span::styled(format!("`{code}`"), code_style));
            }

            Event::SoftBreak => {
                if !in_code_block {
                    current_spans.push(Span::raw(" "));
                }
            }

            Event::End(TagEnd::Paragraph | TagEnd::Item) | Event::HardBreak => {
                flush_spans(&mut current_spans, &mut lines);
            }

            // Ignore other events (HTML, footnotes, etc.)
            _ => {}
        }
    }

    // Flush any remaining spans
    flush_spans(&mut current_spans, &mut lines);

    // Suppress unused variable warning; link_url reserved for future OSC 8 hyperlinks
    let _ = link_url;

    lines
}

/// List context for tracking ordered/unordered and counter.
struct ListContext {
    ordered: bool,
    counter: u64,
}

/// Get the current combined style from the style stack.
fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

/// Flush accumulated spans into a line and push to lines vec.
fn flush_spans(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

/// Render a code block with borders and syntax highlighting.
fn render_code_block(lang: &str, content: &str, lines: &mut Vec<Line<'static>>) {
    let lang_display = if lang.is_empty() { "code" } else { lang };
    let border_style = Style::default().fg(Color::DarkGray);

    // Top border with language label
    let top_border = format!("--- {lang_display} ---");
    lines.push(Line::from(Span::styled(top_border, border_style)));

    // Highlighted code lines
    let trimmed = content.trim_end_matches('\n');
    let highlighted = highlight_code(trimmed, lang);
    lines.extend(highlighted);

    // Bottom border
    lines.push(Line::from(Span::styled("---", border_style)));
}

/// Push a list item prefix (bullet or number) onto current spans.
fn push_list_prefix(list_stack: &[ListContext], spans: &mut Vec<Span<'static>>) {
    let indent = "  ".repeat(list_stack.len().saturating_sub(1));
    if let Some(ctx) = list_stack.last() {
        let prefix = if ctx.ordered {
            format!("{indent}{}. ", ctx.counter)
        } else {
            format!("{indent}  ")
        };
        spans.push(Span::styled(prefix, Style::default().fg(Color::Cyan)));
    }
}

/// Get a heading style based on level.
fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        let lines = render_markdown("");
        assert!(lines.is_empty());
    }

    #[test]
    fn plain_text_returns_single_line() {
        let lines = render_markdown("plain text");
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "plain text");
    }

    #[test]
    fn bold_text_has_bold_modifier() {
        let lines = render_markdown("hello **bold** world");
        assert_eq!(lines.len(), 1);
        let bold_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "bold")
            .expect("should have bold span");
        assert!(
            bold_span.style.add_modifier.contains(Modifier::BOLD),
            "bold span should have BOLD modifier"
        );
    }

    #[test]
    fn italic_text_has_italic_modifier() {
        let lines = render_markdown("*italic* text");
        assert_eq!(lines.len(), 1);
        let italic_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "italic")
            .expect("should have italic span");
        assert!(
            italic_span.style.add_modifier.contains(Modifier::ITALIC),
            "italic span should have ITALIC modifier"
        );
    }

    #[test]
    fn inline_code_has_style() {
        let lines = render_markdown("`inline code`");
        assert_eq!(lines.len(), 1);
        let code_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains("inline code"))
            .expect("should have code span");
        assert_eq!(code_span.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn h1_header_is_bold_cyan() {
        let lines = render_markdown("# Header");
        let header_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.as_ref() == "Header"))
            .expect("should have header line");
        let header_span = header_line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "Header")
            .unwrap();
        assert_eq!(header_span.style.fg, Some(Color::Cyan));
        assert!(header_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h2_header_is_bold_green() {
        let lines = render_markdown("## Sub Header");
        let header_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("Sub Header")))
            .expect("should have header line");
        let header_span = header_line
            .spans
            .iter()
            .find(|s| s.content.contains("Sub Header"))
            .unwrap();
        assert_eq!(header_span.style.fg, Some(Color::Green));
        assert!(header_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unordered_list_has_items() {
        let lines = render_markdown("- item 1\n- item 2");
        let item_lines: Vec<_> = lines
            .iter()
            .filter(|l| l.spans.iter().any(|s| s.content.contains("item")))
            .collect();
        assert!(
            item_lines.len() >= 2,
            "should have at least 2 list item lines, got {}",
            item_lines.len()
        );
    }

    #[test]
    fn ordered_list_has_numbers() {
        let lines = render_markdown("1. first\n2. second");
        let numbered: Vec<_> = lines
            .iter()
            .filter(|l| {
                l.spans
                    .iter()
                    .any(|s| s.content.contains("1.") || s.content.contains("2."))
            })
            .collect();
        assert!(!numbered.is_empty(), "should have numbered list items");
    }

    #[test]
    fn fenced_code_block_has_border_and_highlighting() {
        let input = "```rust\nfn main() {}\n```";
        let lines = render_markdown(input);
        let has_border = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("rust")));
        assert!(has_border, "should have border with language label");
        assert!(lines.len() >= 3, "should have border + code + border");
    }

    #[test]
    fn strikethrough_has_crossed_out() {
        let lines = render_markdown("~~struck~~");
        assert!(!lines.is_empty());
        let struck_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref() == "struck")
            .expect("should have struck span");
        assert!(
            struck_span
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT),
            "strikethrough should have CROSSED_OUT modifier"
        );
    }

    #[test]
    fn link_text_has_underline() {
        let lines = render_markdown("[link](https://example.com)");
        assert!(!lines.is_empty());
        let link_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref() == "link")
            .expect("should have link span");
        assert_eq!(link_span.style.fg, Some(Color::Blue));
        assert!(
            link_span
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }
}
