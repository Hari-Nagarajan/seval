//! Chat input area for text entry.
//!
//! Manages input text, cursor position, and multi-line editing.
//! This is a data-only struct; rendering happens in the chat component.

/// Chat input area managing text, cursor, and multi-line editing.
#[derive(Debug, Clone)]
pub struct ChatInput {
    /// The input text content.
    text: String,
    /// Cursor byte position within `text`.
    cursor_byte: usize,
}

impl ChatInput {
    /// Create a new empty chat input.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_byte: 0,
        }
    }

    /// Insert a character at the current cursor position.
    pub fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    /// Insert a string at the current cursor position (e.g., for paste).
    ///
    /// Normalizes line endings: `\r\n` and bare `\r` are converted to `\n`
    /// so that multi-line pasted text displays correctly regardless of the
    /// terminal's line-ending convention.
    pub fn insert_str(&mut self, s: &str) {
        let normalized = normalize_line_endings(s);
        self.text.insert_str(self.cursor_byte, &normalized);
        self.cursor_byte += normalized.len();
    }

    /// Insert a newline at the current cursor position.
    pub fn insert_newline(&mut self) {
        self.insert('\n');
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        // Find the previous char boundary
        let prev = self.prev_char_boundary();
        self.text.drain(prev..self.cursor_byte);
        self.cursor_byte = prev;
    }

    /// Delete the character at the cursor (delete key).
    pub fn delete(&mut self) {
        if self.cursor_byte >= self.text.len() {
            return;
        }
        let next = self.next_char_boundary();
        self.text.drain(self.cursor_byte..next);
    }

    /// Move the cursor one character to the left.
    pub fn move_left(&mut self) {
        if self.cursor_byte > 0 {
            self.cursor_byte = self.prev_char_boundary();
        }
    }

    /// Move the cursor one character to the right.
    pub fn move_right(&mut self) {
        if self.cursor_byte < self.text.len() {
            self.cursor_byte = self.next_char_boundary();
        }
    }

    /// Move the cursor to the start of the current line.
    pub fn move_home(&mut self) {
        // Find the start of the current line
        let line_start = self.text[..self.cursor_byte]
            .rfind('\n')
            .map_or(0, |pos| pos + 1);
        self.cursor_byte = line_start;
    }

    /// Move the cursor to the end of the current line.
    pub fn move_end(&mut self) {
        // Find the end of the current line
        let line_end = self.text[self.cursor_byte..]
            .find('\n')
            .map_or(self.text.len(), |pos| self.cursor_byte + pos);
        self.cursor_byte = line_end;
    }

    /// Move the cursor up one line (multi-line navigation).
    pub fn move_up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let lines: Vec<&str> = self.text.split('\n').collect();
        let prev_line_len = lines[line - 1].len();
        let target_col = col.min(prev_line_len);

        // Calculate byte offset for target position
        let mut byte_offset = 0;
        for l in &lines[..line - 1] {
            byte_offset += l.len() + 1; // +1 for \n
        }
        // Add column offset respecting char boundaries
        byte_offset += char_byte_offset(lines[line - 1], target_col);
        self.cursor_byte = byte_offset;
    }

    /// Move the cursor down one line (multi-line navigation).
    pub fn move_down(&mut self) {
        let (line, col) = self.cursor_line_col();
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line >= lines.len() - 1 {
            return;
        }
        let next_line_len = lines[line + 1].len();
        let target_col = col.min(next_line_len);

        let mut byte_offset = 0;
        for l in &lines[..=line] {
            byte_offset += l.len() + 1; // +1 for \n
        }
        byte_offset += char_byte_offset(lines[line + 1], target_col);
        self.cursor_byte = byte_offset;
    }

    /// Get the current text content (used in tests).
    #[cfg(test)]
    #[must_use]
    pub fn content(&self) -> &str {
        &self.text
    }

    /// Submit the input: returns the content and clears the input.
    pub fn submit(&mut self) -> String {
        let content = std::mem::take(&mut self.text);
        self.cursor_byte = 0;
        content
    }

    /// Check if the input is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Get the cursor position as (line, column) for rendering.
    #[must_use]
    pub fn cursor_position(&self) -> (usize, usize) {
        self.cursor_line_col()
    }

    /// Split the input into lines for rendering.
    #[must_use]
    pub fn lines(&self) -> Vec<&str> {
        if self.text.is_empty() {
            vec![""]
        } else {
            self.text.split('\n').collect()
        }
    }

    /// Calculate line and column from current cursor byte position.
    fn cursor_line_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor_byte];
        let line = before.matches('\n').count();
        let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);
        let col = before[line_start..].chars().count();
        (line, col)
    }

    /// Find the byte position of the previous character boundary.
    fn prev_char_boundary(&self) -> usize {
        let mut pos = self.cursor_byte;
        if pos > 0 {
            pos -= 1;
            while pos > 0 && !self.text.is_char_boundary(pos) {
                pos -= 1;
            }
        }
        pos
    }

    /// Find the byte position of the next character boundary.
    fn next_char_boundary(&self) -> usize {
        let mut pos = self.cursor_byte + 1;
        while pos < self.text.len() && !self.text.is_char_boundary(pos) {
            pos += 1;
        }
        pos.min(self.text.len())
    }
}

impl Default for ChatInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize line endings to `\n`.
///
/// Converts `\r\n` (Windows) and bare `\r` (old Mac) to `\n` (Unix).
/// Terminal bracketed paste often delivers `\r` or `\r\n` for newlines.
fn normalize_line_endings(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\r') {
        // Replace \r\n first, then remaining bare \r
        std::borrow::Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Calculate the byte offset for a given character column in a string.
fn char_byte_offset(s: &str, char_col: usize) -> usize {
    s.char_indices()
        .nth(char_col)
        .map_or(s.len(), |(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_input() {
        let input = ChatInput::new();
        assert!(input.is_empty());
        assert_eq!(input.content(), "");
    }

    #[test]
    fn insert_char_adds_at_cursor() {
        let mut input = ChatInput::new();
        input.insert('a');
        assert_eq!(input.content(), "a");
        input.insert('b');
        assert_eq!(input.content(), "ab");
    }

    #[test]
    fn insert_str_adds_string() {
        let mut input = ChatInput::new();
        input.insert_str("hello");
        assert_eq!(input.content(), "hello");
    }

    #[test]
    fn backspace_removes_before_cursor() {
        let mut input = ChatInput::new();
        input.insert_str("abc");
        input.backspace();
        assert_eq!(input.content(), "ab");
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut input = ChatInput::new();
        input.backspace();
        assert_eq!(input.content(), "");
    }

    #[test]
    fn delete_removes_at_cursor() {
        let mut input = ChatInput::new();
        input.insert_str("abc");
        input.move_home();
        input.delete();
        assert_eq!(input.content(), "bc");
    }

    #[test]
    fn delete_at_end_does_nothing() {
        let mut input = ChatInput::new();
        input.insert_str("abc");
        input.delete();
        assert_eq!(input.content(), "abc");
    }

    #[test]
    fn move_left_right() {
        let mut input = ChatInput::new();
        input.insert_str("abc");
        input.move_left();
        input.insert('X');
        assert_eq!(input.content(), "abXc");
        input.move_right();
        input.insert('Y');
        assert_eq!(input.content(), "abXcY");
    }

    #[test]
    fn move_left_at_start_stays() {
        let mut input = ChatInput::new();
        input.insert('a');
        input.move_home();
        input.move_left(); // should not go before start
        input.insert('X');
        assert_eq!(input.content(), "Xa");
    }

    #[test]
    fn move_right_at_end_stays() {
        let mut input = ChatInput::new();
        input.insert('a');
        input.move_right(); // should stay at end
        input.insert('b');
        assert_eq!(input.content(), "ab");
    }

    #[test]
    fn home_end_navigation() {
        let mut input = ChatInput::new();
        input.insert_str("hello world");
        input.move_home();
        input.insert('X');
        assert_eq!(input.content(), "Xhello world");
        input.move_end();
        input.insert('Y');
        assert_eq!(input.content(), "Xhello worldY");
    }

    #[test]
    fn submit_returns_content_and_clears() {
        let mut input = ChatInput::new();
        input.insert_str("hello");
        let content = input.submit();
        assert_eq!(content, "hello");
        assert!(input.is_empty());
        assert_eq!(input.content(), "");
    }

    #[test]
    fn submit_slash_command() {
        let mut input = ChatInput::new();
        input.insert_str("/help");
        let content = input.submit();
        assert!(content.starts_with('/'));
        assert_eq!(content, "/help");
    }

    #[test]
    fn is_empty_checks_correctly() {
        let mut input = ChatInput::new();
        assert!(input.is_empty());
        input.insert('a');
        assert!(!input.is_empty());
    }

    #[test]
    fn utf8_multibyte_characters() {
        let mut input = ChatInput::new();
        // Insert multi-byte chars
        input.insert_str("cafe");
        input.backspace();
        input.insert('\u{0301}'); // combining accent
        // Should handle without panicking
        assert!(!input.is_empty());
    }

    #[test]
    fn utf8_emoji() {
        let mut input = ChatInput::new();
        input.insert_str("hello ");
        let before_len = input.content().len();
        input.insert('\u{1F600}'); // grinning face emoji (4 bytes)
        assert_eq!(input.content().len(), before_len + 4);
        input.backspace();
        assert_eq!(input.content(), "hello ");
    }

    #[test]
    fn insert_newline_creates_multiline() {
        let mut input = ChatInput::new();
        input.insert_str("line1");
        input.insert_newline();
        input.insert_str("line2");
        assert_eq!(input.content(), "line1\nline2");
        let lines = input.lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
    }

    #[test]
    fn cursor_position_tracking() {
        let mut input = ChatInput::new();
        assert_eq!(input.cursor_position(), (0, 0));
        input.insert_str("hello");
        assert_eq!(input.cursor_position(), (0, 5));
        input.insert_newline();
        assert_eq!(input.cursor_position(), (1, 0));
        input.insert_str("world");
        assert_eq!(input.cursor_position(), (1, 5));
    }

    #[test]
    fn move_up_down() {
        let mut input = ChatInput::new();
        input.insert_str("line1\nline2\nline3");
        // Cursor at end of line3
        assert_eq!(input.cursor_position(), (2, 5));
        input.move_up();
        assert_eq!(input.cursor_position(), (1, 5));
        input.move_up();
        assert_eq!(input.cursor_position(), (0, 5));
        input.move_up(); // should stay at line 0
        assert_eq!(input.cursor_position(), (0, 5));
        input.move_down();
        assert_eq!(input.cursor_position(), (1, 5));
    }

    #[test]
    fn lines_empty_input() {
        let input = ChatInput::new();
        let lines = input.lines();
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn default_trait() {
        let input = ChatInput::default();
        assert!(input.is_empty());
    }

    #[test]
    fn paste_normalizes_crlf() {
        let mut input = ChatInput::new();
        input.insert_str("line1\r\nline2\r\nline3");
        assert_eq!(input.content(), "line1\nline2\nline3");
        let lines = input.lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
        assert_eq!(lines[2], "line3");
    }

    #[test]
    fn paste_normalizes_bare_cr() {
        let mut input = ChatInput::new();
        input.insert_str("line1\rline2\rline3");
        assert_eq!(input.content(), "line1\nline2\nline3");
        let lines = input.lines();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn paste_normalizes_mixed_line_endings() {
        let mut input = ChatInput::new();
        input.insert_str("a\r\nb\rc\nd");
        assert_eq!(input.content(), "a\nb\nc\nd");
        assert_eq!(input.lines().len(), 4);
    }

    #[test]
    fn paste_multiline_text() {
        let mut input = ChatInput::new();
        input.insert_str("line1\nline2\nline3");
        assert_eq!(input.content(), "line1\nline2\nline3");
        let lines = input.lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
        assert_eq!(lines[2], "line3");
        // Cursor should be at end
        assert_eq!(input.cursor_position(), (2, 5));
    }

    #[test]
    fn paste_empty_string() {
        let mut input = ChatInput::new();
        input.insert_str("");
        assert!(input.is_empty());
        assert_eq!(input.cursor_position(), (0, 0));
    }

    #[test]
    fn paste_into_existing_text() {
        let mut input = ChatInput::new();
        input.insert_str("hello");
        // Cursor is at position 5 (end of "hello")
        input.insert_str(" world");
        assert_eq!(input.content(), "hello world");
    }
}
